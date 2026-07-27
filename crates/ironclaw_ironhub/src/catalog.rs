use ironclaw_host_api::{NetworkPolicy, NetworkScheme, NetworkTargetPattern, sha256_digest_token};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use ironclaw_host_api::{
    LifecycleExtensionRuntimeKind, LifecycleExtensionSource, LifecycleExtensionSummary,
    LifecyclePackageId, LifecyclePackageKind, LifecyclePackageRef, LifecycleSearchExtensionSummary,
    LifecycleSkillSource, LifecycleSkillSummary,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IronHubEntryKind {
    Tool,
    Skill,
}

impl IronHubEntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IronHubProvenance {
    #[serde(alias = "repo")]
    Official,
    Trusted,
    Verified,
    Private,
    #[default]
    #[serde(alias = "community")]
    New,
}

impl IronHubProvenance {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Trusted => "trusted",
            Self::Verified => "verified",
            Self::Private => "private",
            Self::New => "new",
        }
    }

    pub fn is_community_unverified(self) -> bool {
        matches!(self, Self::New)
    }

    pub fn trust_label(self) -> &'static str {
        match self {
            Self::Official => "NEAR-vetted (official)",
            Self::Trusted => "community, trusted publisher",
            Self::Verified => "community, verified publisher",
            Self::Private => "private (your organization)",
            Self::New => "UNVERIFIED community (new author)",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IronHubManifest {
    pub version: String,
    pub generated_at: String,
    pub release_tag: String,
    pub repo: String,
    #[serde(default)]
    pub tools: Vec<IronHubToolEntry>,
    #[serde(default)]
    pub skills: Vec<IronHubSkillEntry>,
}

impl IronHubManifest {
    pub fn find_tool(&self, name: &str) -> Option<&IronHubToolEntry> {
        self.tools.iter().find(|entry| entry.name == name)
    }

    pub fn find_skill(&self, name: &str) -> Option<&IronHubSkillEntry> {
        self.skills.iter().find(|entry| entry.name == name)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IronHubToolEntry {
    pub name: String,
    pub crate_name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provenance: IronHubProvenance,
    pub wasm: IronHubArtifact,
    pub capabilities: IronHubArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IronHubSkillEntry {
    pub name: String,
    #[serde(default)]
    pub trunk: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provenance: IronHubProvenance,
    pub skill_md: IronHubArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IronHubArtifact {
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IronHubInstallOptions {
    pub kind: Option<IronHubEntryKind>,
    pub force: bool,
    pub acknowledge_unverified: bool,
    pub expected_version: Option<String>,
    pub expected_artifact_digest: Option<String>,
    pub private_manifest_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IronHubCommand {
    Search {
        query: String,
    },
    List {
        kind: Option<IronHubEntryKind>,
    },
    Info {
        name: String,
        kind: Option<IronHubEntryKind>,
    },
    Install {
        name: String,
        options: IronHubInstallOptions,
    },
}

#[derive(Debug, Error)]
pub enum IronHubCommandError {
    #[error("IronHub is available only for local-dev Reborn services")]
    LocalRuntimeUnavailable,
    #[error("IronHub runtime HTTP egress is unavailable")]
    RuntimeHttpEgressUnavailable,
    #[error("invalid IronHub input: {reason}")]
    InvalidInput { reason: String },
    #[error("IronHub catalog failed: {reason}")]
    Catalog { reason: String },
    #[error("IronHub install failed: {reason}")]
    Install { reason: String },
    #[error("IronHub lifecycle failed: {reason}")]
    Lifecycle { reason: String },
}

pub fn ironhub_invalid_input(error: impl std::fmt::Display) -> IronHubCommandError {
    IronHubCommandError::InvalidInput {
        reason: error.to_string(),
    }
}

pub fn ironhub_catalog_error(reason: impl Into<String>) -> IronHubCommandError {
    IronHubCommandError::Catalog {
        reason: reason.into(),
    }
}

pub fn ironhub_install_error(reason: impl Into<String>) -> IronHubCommandError {
    IronHubCommandError::Install {
        reason: reason.into(),
    }
}

pub fn ironhub_product_error(error: impl std::fmt::Display) -> IronHubCommandError {
    IronHubCommandError::Lifecycle {
        reason: error.to_string(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct IronHubDefaultArtifactHosts {
    pub hosts: Vec<String>,
    pub suffixes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct IronHubArtifactHosts {
    catalog_host: Option<String>,
    operator_hosts: Vec<String>,
    defaults: IronHubDefaultArtifactHosts,
}

impl IronHubArtifactHosts {
    pub fn new(
        catalog_host: Option<String>,
        operator_hosts: Vec<String>,
        defaults: IronHubDefaultArtifactHosts,
    ) -> Self {
        Self {
            catalog_host,
            operator_hosts: operator_hosts
                .into_iter()
                .map(|host| host.trim().to_ascii_lowercase())
                .filter(|host| !host.is_empty() && !host_is_disallowed_target(host))
                .collect(),
            defaults: IronHubDefaultArtifactHosts {
                hosts: normalize_hosts(defaults.hosts),
                suffixes: normalize_hosts(defaults.suffixes),
            },
        }
    }

    fn allows(&self, host: &str) -> bool {
        self.defaults
            .hosts
            .iter()
            .any(|allowed| host.eq_ignore_ascii_case(allowed))
            || self
                .defaults
                .suffixes
                .iter()
                .any(|suffix| host.to_ascii_lowercase().ends_with(suffix.as_str()))
            || self
                .operator_hosts
                .iter()
                .any(|allowed| host.eq_ignore_ascii_case(allowed))
            || self
                .catalog_host
                .as_deref()
                .is_some_and(|allowed| host.eq_ignore_ascii_case(allowed))
    }
}

fn normalize_hosts(hosts: Vec<String>) -> Vec<String> {
    hosts
        .into_iter()
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect()
}

pub fn classify_gate_and_digest(
    manifest: &IronHubManifest,
    name: &str,
    hint: Option<IronHubEntryKind>,
    options: &IronHubInstallOptions,
) -> Result<(IronHubEntryKind, IronHubProvenance, String), IronHubCommandError> {
    let kind = classify(manifest, name, hint)?;
    let (version, provenance, artifact_digest) = match kind {
        IronHubEntryKind::Tool => {
            let entry = manifest
                .find_tool(name)
                .ok_or_else(|| ironhub_catalog_error("tool not found"))?;
            (
                entry.version.as_str(),
                entry.provenance,
                tool_artifact_digest(entry),
            )
        }
        IronHubEntryKind::Skill => {
            let entry = manifest
                .find_skill(name)
                .ok_or_else(|| ironhub_catalog_error("skill not found"))?;
            (
                entry.version.as_str(),
                entry.provenance,
                skill_artifact_digest(entry),
            )
        }
    };
    let provenance = if options.private_manifest_url.is_some() {
        IronHubProvenance::Private
    } else if matches!(provenance, IronHubProvenance::Private) {
        return Err(ironhub_invalid_input(format!(
            "catalog entry '{name}' claims private provenance but was not installed from a private manifest"
        )));
    } else {
        provenance
    };
    if let Some(expected) = &options.expected_version
        && expected != version
    {
        return Err(IronHubCommandError::InvalidInput {
            reason: format!(
                "catalog version for '{name}' changed: expected {expected}, current {version}"
            ),
        });
    }
    if let Some(expected) = &options.expected_artifact_digest
        && !expected.eq_ignore_ascii_case(&artifact_digest)
    {
        return Err(IronHubCommandError::InvalidInput {
            reason: format!(
                "artifact digest for '{name}' changed: expected {expected}, current {artifact_digest}"
            ),
        });
    }
    if provenance.is_community_unverified() && !options.acknowledge_unverified {
        return Err(IronHubCommandError::InvalidInput {
            reason: format!(
                "'{name}' is UNVERIFIED community content (trust tier: {}). Re-run with acknowledgement to install at your own risk.",
                provenance.as_wire()
            ),
        });
    }
    Ok((kind, provenance, artifact_digest))
}

pub fn classify(
    manifest: &IronHubManifest,
    name: &str,
    hint: Option<IronHubEntryKind>,
) -> Result<IronHubEntryKind, IronHubCommandError> {
    let in_tools = manifest.find_tool(name).is_some();
    let in_skills = manifest.find_skill(name).is_some();
    match (hint, in_tools, in_skills) {
        (Some(IronHubEntryKind::Tool), true, _) => Ok(IronHubEntryKind::Tool),
        (Some(IronHubEntryKind::Tool), false, _) => Err(ironhub_invalid_input(format!(
            "'{name}' is not a tool in this IronHub catalog"
        ))),
        (Some(IronHubEntryKind::Skill), _, true) => Ok(IronHubEntryKind::Skill),
        (Some(IronHubEntryKind::Skill), _, false) => Err(ironhub_invalid_input(format!(
            "'{name}' is not a skill in this IronHub catalog"
        ))),
        (None, true, false) => Ok(IronHubEntryKind::Tool),
        (None, false, true) => Ok(IronHubEntryKind::Skill),
        (None, true, true) => Err(ironhub_invalid_input(format!(
            "'{name}' exists as both a tool and a skill; specify a kind"
        ))),
        (None, false, false) => Err(ironhub_invalid_input(format!(
            "'{name}' is not in this IronHub catalog"
        ))),
    }
}

fn tool_artifact_digest(entry: &IronHubToolEntry) -> String {
    sha256_digest_token(format!("{}:{}", entry.wasm.sha256, entry.capabilities.sha256).as_bytes())
}

fn skill_artifact_digest(entry: &IronHubSkillEntry) -> String {
    sha256_digest_token(entry.skill_md.sha256.as_bytes())
}

pub fn validate_artifact(
    artifact: &IronHubArtifact,
    max_bytes: u64,
    hosts: &IronHubArtifactHosts,
) -> Result<(), IronHubCommandError> {
    validate_artifact_url("artifact", "url", &artifact.url, hosts)?;
    if artifact.size_bytes > max_bytes {
        return Err(IronHubCommandError::Catalog {
            reason: format!("artifact exceeds {max_bytes} byte cap"),
        });
    }
    if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(IronHubCommandError::Catalog {
            reason: "artifact sha256 must be 64 hex characters".to_string(),
        });
    }
    Ok(())
}

pub fn validate_artifact_url(
    manifest_name: &str,
    field: &'static str,
    url: &str,
    hosts: &IronHubArtifactHosts,
) -> Result<(), IronHubCommandError> {
    let parsed = url::Url::parse(url).map_err(|error| IronHubCommandError::Catalog {
        reason: format!("{manifest_name}.{field} invalid URL: {error}"),
    })?;
    if parsed.scheme() != "https" {
        return Err(IronHubCommandError::Catalog {
            reason: format!("{manifest_name}.{field} must use https"),
        });
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| IronHubCommandError::Catalog {
            reason: format!("{manifest_name}.{field} host is missing"),
        })?;
    if host_is_disallowed_target(host) || !hosts.allows(host) {
        return Err(IronHubCommandError::Catalog {
            reason: format!("{manifest_name}.{field} host '{host}' is not allowed"),
        });
    }
    Ok(())
}

pub fn network_policy_for_url(
    url: &str,
    max_bytes: u64,
    hosts: &IronHubArtifactHosts,
) -> Result<NetworkPolicy, IronHubCommandError> {
    validate_artifact_url("download", "url", url, hosts)?;
    let parsed = url::Url::parse(url).map_err(|error| IronHubCommandError::Catalog {
        reason: format!("invalid URL: {error}"),
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ironhub_catalog_error("URL host is missing"))?;
    Ok(NetworkPolicy {
        allowed_targets: vec![NetworkTargetPattern {
            scheme: Some(NetworkScheme::Https),
            host_pattern: host.to_ascii_lowercase(),
            port: parsed.port(),
        }],
        deny_private_ip_ranges: true,
        max_egress_bytes: Some(max_bytes),
    })
}

fn host_is_disallowed_target(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let ip_form = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    if ip_form.parse::<std::net::IpAddr>().is_ok() || host == "localhost" {
        return true;
    }
    const INTERNAL_SUFFIXES: &[&str] = &[
        ".localhost",
        ".local",
        ".internal",
        ".intranet",
        ".lan",
        ".home",
        ".corp",
        ".private",
    ];
    INTERNAL_SUFFIXES
        .iter()
        .any(|suffix| host.ends_with(suffix))
        || !host.contains('.')
}

pub fn validate_hub_name(name: &str) -> Result<(), IronHubCommandError> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_');
    if valid {
        Ok(())
    } else {
        Err(ironhub_invalid_input(
            "name must be non-empty and contain only lowercase letters, digits, '-', '_'",
        ))
    }
}

pub fn tool_summary(
    entry: &IronHubToolEntry,
) -> Result<LifecycleSearchExtensionSummary, IronHubCommandError> {
    Ok(LifecycleSearchExtensionSummary {
        summary: LifecycleExtensionSummary {
            package_ref: ironhub_package_ref(LifecyclePackageKind::Extension, &entry.name)?,
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: format!("{} [{}]", entry.description, entry.provenance.trust_label()),
            source: LifecycleExtensionSource::HostBundled,
            runtime_kind: LifecycleExtensionRuntimeKind::WasmTool,
            surface_kinds: Vec::new(),
            visible_capability_ids: vec![format!("{}.invoke", entry.name)],
            visible_read_only_capability_ids: Vec::new(),
            credential_requirements: Vec::new(),
            channel_directions: None,
            channel_connection: None,
            channel_presentation: None,
            onboarding: None,
        },
        installation_phase: None,
    })
}

pub fn skill_summary(
    entry: &IronHubSkillEntry,
) -> Result<LifecycleSkillSummary, IronHubCommandError> {
    Ok(LifecycleSkillSummary {
        name: LifecyclePackageId::new(entry.name.clone()).map_err(ironhub_product_error)?,
        version: entry.version.clone(),
        description: format!("{} [{}]", entry.description, entry.provenance.trust_label()),
        source: LifecycleSkillSource::User,
        keywords: Vec::new(),
        tags: Vec::new(),
        requires_skills: Vec::new(),
    })
}

pub fn entry_matches(name: &str, description: &str, query: &str) -> bool {
    query.is_empty()
        || name.to_ascii_lowercase().contains(query)
        || description.to_ascii_lowercase().contains(query)
}

pub fn ironhub_package_ref(
    kind: LifecyclePackageKind,
    id: &str,
) -> Result<LifecyclePackageRef, IronHubCommandError> {
    LifecyclePackageRef::new(kind, id).map_err(ironhub_product_error)
}

#[cfg(test)]
mod tests {
    fn test_default_hosts() -> IronHubDefaultArtifactHosts {
        IronHubDefaultArtifactHosts {
            hosts: vec!["hub.ironclaw.com".to_string()],
            suffixes: vec![".githubusercontent.com".to_string()],
        }
    }

    use ironclaw_host_api::sha256_digest_token;

    use super::*;

    const GENERATED_AT: &str = "2026-01-01T00:00:00Z";

    fn artifact(sha: &str) -> IronHubArtifact {
        IronHubArtifact {
            url: "https://hub.ironclaw.com/entry".to_string(),
            size_bytes: 1,
            sha256: sha.to_string(),
        }
    }

    fn skill_manifest(name: &str, provenance: IronHubProvenance) -> IronHubManifest {
        IronHubManifest {
            version: "1".to_string(),
            generated_at: GENERATED_AT.to_string(),
            release_tag: "test".to_string(),
            repo: "nearai/ironhub".to_string(),
            tools: Vec::new(),
            skills: vec![IronHubSkillEntry {
                name: name.to_string(),
                trunk: String::new(),
                version: "0.1.0".to_string(),
                description: String::new(),
                provenance,
                skill_md: artifact(&"c".repeat(64)),
            }],
        }
    }

    #[test]
    fn missing_provenance_defaults_to_unverified() {
        let manifest: IronHubManifest = serde_json::from_str(
            r#"{"version":"1","generated_at":"2026-01-01T00:00:00Z","release_tag":"test","repo":"nearai/ironhub","skills":[{"name":"community-skill","version":"0.1.0","skill_md":{"url":"https://hub.ironclaw.com/s","size_bytes":1,"sha256":"cc"}}]}"#,
        )
        .expect("manifest parses");
        assert_eq!(manifest.skills[0].provenance, IronHubProvenance::New);
    }

    #[test]
    fn unverified_install_requires_acknowledgement() {
        let manifest = skill_manifest("community-skill", IronHubProvenance::New);
        let blocked = classify_gate_and_digest(
            &manifest,
            "community-skill",
            Some(IronHubEntryKind::Skill),
            &IronHubInstallOptions::default(),
        )
        .expect_err("unverified content requires acknowledgement");
        assert!(blocked.to_string().contains("UNVERIFIED community content"));

        let allowed = classify_gate_and_digest(
            &manifest,
            "community-skill",
            Some(IronHubEntryKind::Skill),
            &IronHubInstallOptions {
                acknowledge_unverified: true,
                ..IronHubInstallOptions::default()
            },
        )
        .expect("acknowledged unverified content can proceed");
        assert_eq!(allowed.1, IronHubProvenance::New);
    }

    #[test]
    fn private_provenance_requires_private_manifest_source() {
        let manifest = skill_manifest("org-skill", IronHubProvenance::Private);
        let rejected = classify_gate_and_digest(
            &manifest,
            "org-skill",
            Some(IronHubEntryKind::Skill),
            &IronHubInstallOptions::default(),
        )
        .expect_err("private provenance needs a private manifest source");
        assert!(rejected.to_string().contains("claims private provenance"));

        let allowed = classify_gate_and_digest(
            &manifest,
            "org-skill",
            Some(IronHubEntryKind::Skill),
            &IronHubInstallOptions {
                private_manifest_url: Some("https://hub.ironclaw.com/org/manifest".to_string()),
                ..IronHubInstallOptions::default()
            },
        )
        .expect("a private manifest source allows private provenance");
        assert_eq!(allowed.1, IronHubProvenance::Private);
    }

    #[test]
    fn artifact_digest_binds_both_tool_artifacts() {
        let tool = IronHubToolEntry {
            name: "web".to_string(),
            crate_name: "web-tool".to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            provenance: IronHubProvenance::Official,
            wasm: artifact(&"a".repeat(64)),
            capabilities: artifact(&"b".repeat(64)),
        };
        assert_eq!(
            tool_artifact_digest(&tool),
            sha256_digest_token(format!("{}:{}", "a".repeat(64), "b".repeat(64)).as_bytes())
        );
    }

    #[test]
    fn internal_hosts_are_never_valid_artifact_targets() {
        for host in [
            "localhost",
            "10.0.0.1",
            "[::1]",
            "service.internal",
            "box.lan",
            "single-label",
        ] {
            assert!(host_is_disallowed_target(host), "{host}");
        }
        assert!(!host_is_disallowed_target("hub.ironclaw.com"));
    }

    #[test]
    fn operator_hosts_widen_the_allowlist_but_cannot_reach_internal_targets() {
        let hosts = IronHubArtifactHosts::new(
            Some("catalog.example.com".to_string()),
            vec![
                "Artifacts.Example.Com".to_string(),
                "localhost".to_string(),
                "db.internal".to_string(),
                "  ".to_string(),
            ],
            test_default_hosts(),
        );

        validate_artifact_url("m", "url", "https://artifacts.example.com/a.wasm", &hosts)
            .expect("operator host is allowed, case-insensitively");
        validate_artifact_url("m", "url", "https://catalog.example.com/a.wasm", &hosts)
            .expect("catalog host is pinned in");
        validate_artifact_url("m", "url", "https://hub.ironclaw.com/a.wasm", &hosts)
            .expect("built-in host stays allowed");

        for rejected in [
            "https://localhost/a.wasm",
            "https://db.internal/a.wasm",
            "https://elsewhere.example.com/a.wasm",
        ] {
            validate_artifact_url("m", "url", rejected, &hosts)
                .expect_err(&format!("{rejected} must be rejected"));
        }
    }

    #[test]
    fn artifact_urls_must_be_https() {
        let hosts = IronHubArtifactHosts::new(
            Some("hub.ironclaw.com".to_string()),
            Vec::new(),
            test_default_hosts(),
        );
        let error = validate_artifact_url("m", "url", "http://hub.ironclaw.com/a.wasm", &hosts)
            .expect_err("plaintext http is rejected");
        assert!(error.to_string().contains("must use https"));
    }

    #[test]
    fn network_policy_pins_the_resolved_host_and_denies_private_ranges() {
        let hosts = IronHubArtifactHosts::new(
            Some("hub.ironclaw.com".to_string()),
            Vec::new(),
            test_default_hosts(),
        );
        let policy = network_policy_for_url("https://hub.ironclaw.com/a.wasm", 1024, &hosts)
            .expect("policy builds for an allowed host");
        assert_eq!(policy.allowed_targets.len(), 1);
        assert_eq!(policy.allowed_targets[0].host_pattern, "hub.ironclaw.com");
        assert!(policy.deny_private_ip_ranges);
        assert_eq!(policy.max_egress_bytes, Some(1024));
    }

    #[test]
    fn hub_names_reject_path_and_scheme_characters() {
        validate_hub_name("web_search-2").expect("plain lowercase names are valid");
        for rejected in ["", "../etc", "Web", "a/b", "a:b", "a b"] {
            validate_hub_name(rejected).expect_err(rejected);
        }
    }
}
