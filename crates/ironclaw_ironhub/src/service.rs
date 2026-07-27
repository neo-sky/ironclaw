use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

use chrono::{DateTime, Utc};
use ironclaw_common::hashing::sha256_hex;
use ironclaw_extension_host::ExtensionLifecycleManager;
use ironclaw_host_api::{
    CapabilityId, LifecyclePackageKind, NetworkMethod, ResourceScope, RuntimeHttpEgress,
    RuntimeHttpEgressError, RuntimeHttpEgressRequest, RuntimeHttpEgressResponse, RuntimeKind,
};
use ironclaw_skills::ScopedSkillManagementPort;
use tokio::sync::Mutex as AsyncMutex;

use crate::catalog::{
    IronHubArtifact, IronHubArtifactHosts, IronHubCommandError, IronHubDefaultArtifactHosts,
    IronHubEntryKind, IronHubInstallOptions, IronHubManifest, IronHubProvenance, classify,
    classify_gate_and_digest, entry_matches, ironhub_catalog_error, ironhub_install_error,
    ironhub_package_ref as package_ref, network_policy_for_url, skill_summary, tool_summary,
    validate_artifact, validate_artifact_url, validate_hub_name,
};
use crate::model::{
    DEFAULT_IRONHUB_MANIFEST_URL, IronHubCommand, MANIFEST_CACHE_MAX_ENTRIES, MANIFEST_CACHE_TTL,
    MAX_MANIFEST_BYTES, MAX_METADATA_BYTES, MAX_SIGNED_MANIFEST_BYTES, MAX_WASM_BYTES,
};
use crate::package::ironhub_tool_bundle_zip;
use crate::response::IronHubResponse;

struct CachedManifest {
    manifest: Arc<IronHubManifest>,
    fetched_at: Instant,
}

static MANIFEST_CACHE: LazyLock<std::sync::Mutex<HashMap<String, CachedManifest>>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static MANIFEST_FETCH_LOCKS: LazyLock<std::sync::Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static MANIFEST_LAST_SEEN: LazyLock<std::sync::Mutex<HashMap<String, DateTime<Utc>>>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static INSTALL_LOCKS: LazyLock<std::sync::Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

struct IronHubEgress {
    egress: Arc<dyn RuntimeHttpEgress>,
    capability_id: CapabilityId,
}

impl IronHubEgress {
    fn capability_id(&self) -> CapabilityId {
        self.capability_id.clone()
    }

    async fn execute(
        &self,
        request: RuntimeHttpEgressRequest,
    ) -> Result<RuntimeHttpEgressResponse, RuntimeHttpEgressError> {
        self.egress.execute(request).await // safety: HTTP egress, not a database operation
    }
}

pub struct IronHubService {
    skill_management: Arc<ScopedSkillManagementPort>,
    extension_manager: Arc<ExtensionLifecycleManager>,
    egress: IronHubEgress,
    scope: ResourceScope,
    manifest_url: String,
    hosts: IronHubArtifactHosts,
    default_hosts: IronHubDefaultArtifactHosts,
    manifest_verify_keys: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone)]
pub struct IronHubCatalogSource {
    pub manifest_url: String,
    pub manifest_verify_keys: &'static [(&'static str, &'static str)],
}

impl IronHubService {
    pub fn new_with_runtime_egress(
        skill_management: Arc<ScopedSkillManagementPort>,
        extension_manager: Arc<ExtensionLifecycleManager>,
        runtime_http_egress: Arc<dyn RuntimeHttpEgress>,
        capability_id: CapabilityId,
        scope: ResourceScope,
        default_hosts: IronHubDefaultArtifactHosts,
    ) -> Self {
        Self::new(
            skill_management,
            extension_manager,
            IronHubEgress {
                egress: runtime_http_egress,
                capability_id,
            },
            scope,
            default_hosts,
        )
    }

    fn new(
        skill_management: Arc<ScopedSkillManagementPort>,
        extension_manager: Arc<ExtensionLifecycleManager>,
        egress: IronHubEgress,
        scope: ResourceScope,
        default_hosts: IronHubDefaultArtifactHosts,
    ) -> Self {
        let manifest_url = resolve_manifest_url();
        Self {
            skill_management,
            extension_manager,
            egress,
            scope,
            hosts: artifact_hosts(&manifest_url, default_hosts.clone()),
            default_hosts,
            manifest_url,
            manifest_verify_keys: crate::model::MANIFEST_VERIFY_KEYS,
        }
    }

    pub fn with_catalog_source(mut self, source: IronHubCatalogSource) -> Self {
        self.manifest_url = source.manifest_url;
        self.manifest_verify_keys = source.manifest_verify_keys;
        self.hosts = artifact_hosts(&self.manifest_url, self.default_hosts.clone());
        self
    }

    pub async fn execute(
        &self,
        command: IronHubCommand,
    ) -> Result<IronHubResponse, IronHubCommandError> {
        match command {
            IronHubCommand::Search { query } => self.search(&query).await,
            IronHubCommand::List { kind } => self.list(kind).await,
            IronHubCommand::Info { name, kind } => self.info(&name, kind).await,
            IronHubCommand::Install { name, options } => self.install(&name, options).await,
        }
    }

    async fn search(&self, query: &str) -> Result<IronHubResponse, IronHubCommandError> {
        let manifest = self.fetch_manifest_cached(&self.manifest_url).await?;
        let query = query.trim().to_ascii_lowercase();
        let tools = manifest
            .tools
            .iter()
            .filter(|entry| entry_matches(&entry.name, &entry.description, &query))
            .map(tool_summary)
            .collect::<Result<Vec<_>, _>>()?;
        let skills = manifest
            .skills
            .iter()
            .filter(|entry| entry_matches(&entry.name, &entry.description, &query))
            .map(skill_summary)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(IronHubResponse::catalog(tools, skills))
    }

    async fn list(
        &self,
        kind: Option<IronHubEntryKind>,
    ) -> Result<IronHubResponse, IronHubCommandError> {
        let manifest = self.fetch_manifest_cached(&self.manifest_url).await?;
        let tools = match kind {
            Some(IronHubEntryKind::Skill) => Vec::new(),
            _ => manifest
                .tools
                .iter()
                .map(tool_summary)
                .collect::<Result<Vec<_>, _>>()?,
        };
        let skills = match kind {
            Some(IronHubEntryKind::Tool) => Vec::new(),
            _ => manifest
                .skills
                .iter()
                .map(skill_summary)
                .collect::<Result<Vec<_>, _>>()?,
        };
        Ok(IronHubResponse::catalog(tools, skills))
    }

    async fn info(
        &self,
        name: &str,
        hint: Option<IronHubEntryKind>,
    ) -> Result<IronHubResponse, IronHubCommandError> {
        validate_hub_name(name)?;
        let manifest = self.fetch_manifest_cached(&self.manifest_url).await?;
        let kind = classify(&manifest, name, hint)?;
        let response = match kind {
            IronHubEntryKind::Tool => {
                let tool = manifest
                    .find_tool(name)
                    .ok_or_else(|| ironhub_catalog_error("tool not found"))?;
                IronHubResponse::catalog(vec![tool_summary(tool)?], Vec::new())
            }
            IronHubEntryKind::Skill => {
                let skill = manifest
                    .find_skill(name)
                    .ok_or_else(|| ironhub_catalog_error("skill not found"))?;
                IronHubResponse::catalog(Vec::new(), vec![skill_summary(skill)?])
            }
        };
        Ok(response)
    }

    async fn install(
        &self,
        name: &str,
        options: IronHubInstallOptions,
    ) -> Result<IronHubResponse, IronHubCommandError> {
        validate_hub_name(name)?;
        let manifest = match options.private_manifest_url.as_deref() {
            Some(private_url) => Arc::new(self.download_and_verify_manifest(private_url).await?),
            None => self.fetch_manifest_cached(&self.manifest_url).await?,
        };
        let (kind, provenance, artifact_digest) =
            classify_gate_and_digest(&manifest, name, options.kind, &options)?;
        let lock_key = format!("{}:{name}", kind.as_str());
        let lock = install_lock(&lock_key);
        let _guard = lock.lock().await;
        match kind {
            IronHubEntryKind::Skill => {
                let entry = manifest
                    .find_skill(name)
                    .ok_or_else(|| ironhub_catalog_error("skill not found"))?;
                let content = self
                    .download_verified(&entry.skill_md, MAX_METADATA_BYTES)
                    .await?;
                let content = String::from_utf8(content).map_err(|error| {
                    ironhub_install_error(format!("skill markdown is not UTF-8: {error}"))
                })?;
                let result = self
                    .skill_management
                    .install_for_scope(self.scope.clone(), Some(&entry.name), &content)
                    .await
                    .map_err(|error| ironhub_install_error(error.to_string()))?;
                let message = install_message(
                    IronHubEntryKind::Skill,
                    name,
                    entry.version.as_str(),
                    provenance,
                    &artifact_digest,
                );
                Ok(IronHubResponse::installed(
                    package_ref(LifecyclePackageKind::Skill, &result.name)?,
                    IronHubEntryKind::Skill,
                    result.name,
                    message,
                ))
            }
            IronHubEntryKind::Tool => {
                let entry = manifest
                    .find_tool(name)
                    .ok_or_else(|| ironhub_catalog_error("tool not found"))?;
                let wasm = self.download_verified(&entry.wasm, MAX_WASM_BYTES).await?;
                let _capabilities = self
                    .download_verified(&entry.capabilities, MAX_METADATA_BYTES)
                    .await?;
                let bundle = ironhub_tool_bundle_zip(entry, &wasm)?;
                self.extension_manager
                    .import_bundle(bundle)
                    .await
                    .map_err(|error| ironhub_install_error(error.to_string()))?;
                let tool_ref = package_ref(LifecyclePackageKind::Extension, &entry.name)?;
                self.extension_manager
                    .install(tool_ref.clone(), &self.scope.user_id)
                    .await
                    .map_err(|error| ironhub_install_error(error.to_string()))?;
                let message = install_message(
                    IronHubEntryKind::Tool,
                    name,
                    entry.version.as_str(),
                    provenance,
                    &artifact_digest,
                );
                Ok(IronHubResponse::installed(
                    tool_ref,
                    IronHubEntryKind::Tool,
                    entry.name.clone(),
                    message,
                ))
            }
        }
    }

    async fn fetch_manifest_cached(
        &self,
        url: &str,
    ) -> Result<Arc<IronHubManifest>, IronHubCommandError> {
        let now = Instant::now();
        if let Some(hit) = manifest_cache_get(url, now) {
            return Ok(hit);
        }
        let fetch_lock = manifest_fetch_lock(url);
        let _fetch_guard = fetch_lock.lock().await;
        let now = Instant::now();
        if let Some(hit) = manifest_cache_get(url, now) {
            return Ok(hit);
        }
        let manifest = Arc::new(self.download_and_verify_manifest(url).await?);
        manifest_cache_put(url, Arc::clone(&manifest), now);
        Ok(manifest)
    }

    async fn download_and_verify_manifest(
        &self,
        url: &str,
    ) -> Result<IronHubManifest, IronHubCommandError> {
        validate_artifact_url("hub-manifest", "manifest_url", url, &self.hosts)?;
        let envelope = self.download_url(url, MAX_SIGNED_MANIFEST_BYTES).await?;
        let verified_manifest = crate::signature::verify_signed_manifest_with_keys(
            &envelope,
            self.manifest_verify_keys,
        );
        let bytes = verified_manifest.map_err(|reason| {
            ironhub_catalog_error(format!("signed manifest verification failed: {reason}"))
        })?;
        if bytes.len() > usize::try_from(MAX_MANIFEST_BYTES).unwrap_or(usize::MAX) {
            return Err(ironhub_catalog_error("manifest exceeds size cap"));
        }
        let manifest: IronHubManifest = serde_json::from_slice(&bytes)
            .map_err(|error| ironhub_catalog_error(format!("manifest parse failed: {error}")))?;
        enforce_manifest_monotonic(url, &manifest)?;
        Ok(manifest)
    }

    async fn download_verified(
        &self,
        artifact: &IronHubArtifact,
        max_bytes: u64,
    ) -> Result<Vec<u8>, IronHubCommandError> {
        validate_artifact(artifact, max_bytes, &self.hosts)?;
        let bytes = self.download_url(&artifact.url, max_bytes).await?;
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(&artifact.sha256) {
            return Err(ironhub_install_error(format!(
                "checksum mismatch for {}: expected {}, got {}",
                artifact.url, artifact.sha256, actual
            )));
        }
        Ok(bytes)
    }

    async fn download_url(
        &self,
        url: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, IronHubCommandError> {
        let request = RuntimeHttpEgressRequest {
            runtime: RuntimeKind::FirstParty,
            scope: self.scope.clone(),
            capability_id: self.egress.capability_id(),
            method: NetworkMethod::Get,
            url: url.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
            network_policy: network_policy_for_url(url, max_bytes, &self.hosts)?,
            credential_injections: Vec::new(),
            response_body_limit: Some(max_bytes),
            save_body_to: None,
            timeout_ms: Some(30_000),
        };
        let response = self.egress.execute(request).await.map_err(|error| {
            // safety: HTTP egress, not a database operation
            ironhub_catalog_error(error.stable_runtime_reason().to_string())
        })?;
        if !(200..300).contains(&response.status) {
            return Err(ironhub_catalog_error(format!(
                "download returned HTTP {}",
                response.status
            )));
        }
        if response.body.len() > usize::try_from(max_bytes).unwrap_or(usize::MAX) {
            return Err(ironhub_catalog_error("download exceeds size cap"));
        }
        Ok(response.body)
    }
}

fn resolve_manifest_url() -> String {
    std::env::var("IRONHUB_MANIFEST_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_IRONHUB_MANIFEST_URL.to_string())
}

fn artifact_hosts(
    manifest_url: &str,
    default_hosts: IronHubDefaultArtifactHosts,
) -> IronHubArtifactHosts {
    let catalog_host = url::Url::parse(manifest_url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string));
    IronHubArtifactHosts::new(catalog_host, operator_artifact_hosts(), default_hosts)
}

fn operator_artifact_hosts() -> Vec<String> {
    // silent-ok: an unset operator allowlist means no extra artifact hosts.
    std::env::var("IRONHUB_EXTRA_ARTIFACT_HOSTS")
        .unwrap_or_default()
        .split(',')
        .map(str::to_string)
        .collect()
}

fn manifest_cache_get(url: &str, now: Instant) -> Option<Arc<IronHubManifest>> {
    let guard = MANIFEST_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = guard.get(url)?;
    (now.duration_since(entry.fetched_at) <= MANIFEST_CACHE_TTL)
        .then(|| Arc::clone(&entry.manifest))
}

fn manifest_cache_put(url: &str, manifest: Arc<IronHubManifest>, now: Instant) {
    let mut guard = MANIFEST_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.len() >= MANIFEST_CACHE_MAX_ENTRIES && !guard.contains_key(url) {
        guard.retain(|_, entry| now.duration_since(entry.fetched_at) <= MANIFEST_CACHE_TTL);
        if guard.len() >= MANIFEST_CACHE_MAX_ENTRIES
            && let Some(victim) = guard.keys().next().cloned()
        {
            guard.remove(&victim);
        }
    }
    guard.insert(
        url.to_string(),
        CachedManifest {
            manifest,
            fetched_at: now,
        },
    );
}

fn manifest_fetch_lock(url: &str) -> Arc<AsyncMutex<()>> {
    let mut guard = MANIFEST_FETCH_LOCKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .entry(url.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

fn enforce_manifest_monotonic(
    url: &str,
    manifest: &IronHubManifest,
) -> Result<(), IronHubCommandError> {
    let generated_at = DateTime::parse_from_rfc3339(&manifest.generated_at)
        .map_err(|error| {
            ironhub_catalog_error(format!("manifest generated_at is not RFC3339: {error}"))
        })?
        .with_timezone(&Utc);
    let key = manifest_replay_key(url, manifest);
    let mut guard = MANIFEST_LAST_SEEN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(previous) = guard.get(&key)
        && generated_at < *previous
    {
        return Err(ironhub_catalog_error(format!(
            "signed manifest replay rejected: generated_at {} is older than last seen {}",
            generated_at.to_rfc3339(),
            previous.to_rfc3339()
        )));
    }
    guard.insert(key, generated_at);
    Ok(())
}

fn manifest_replay_key(url: &str, manifest: &IronHubManifest) -> String {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_default();
    format!("{}|{host}|{}", host.len(), manifest.repo)
}

fn install_lock(key: &str) -> Arc<AsyncMutex<()>> {
    let mut guard = INSTALL_LOCKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .entry(key.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

fn install_message(
    kind: IronHubEntryKind,
    name: &str,
    version: &str,
    provenance: IronHubProvenance,
    artifact_digest: &str,
) -> String {
    format!(
        "installed {} '{}' {} from IronHub; provenance={}, artifact_digest={}",
        kind.as_str(),
        name,
        version,
        provenance.as_wire(),
        artifact_digest
    )
}
