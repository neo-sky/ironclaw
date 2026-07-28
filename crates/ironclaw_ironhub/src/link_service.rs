use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ironclaw_extension_host::{
    ExtensionLifecycleManager, RuntimeCredentialAccountSelectionService,
};
use ironclaw_host_api::{CapabilityId, InvocationId, ResourceScope, RuntimeHttpEgress, UserId};
use ironclaw_skills::ScopedSkillManagementPort;

use crate::agent_link::{IronhubSharedKey, install_payload, register_payload, verify_signature};
use crate::catalog::{IronHubCommandError, IronHubDefaultArtifactHosts, IronHubInstallOptions};
use crate::link::{
    IronhubInstallDeliveryRequest, IronhubInstallDeliveryResult, IronhubLinkError,
    IronhubLinkService, IronhubRegisterRequest,
};
use crate::model::IronHubCommand;
use crate::service::{IronHubCatalogSource, IronHubService};

const MAX_TIMESTAMP_DRIFT_SECS: u64 = 300;
const INSTALL_CAPABILITY_ID: &str = "builtin.ironhub_install";

static SEEN_INSTALL_NONCES: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct IronhubLinkServiceImpl {
    skill_management: Arc<ScopedSkillManagementPort>,
    extension_manager: Arc<ExtensionLifecycleManager>,
    runtime_http_egress: Arc<dyn RuntimeHttpEgress>,
    shared_key: IronhubSharedKey,
    install_capability: CapabilityId,
    default_artifact_hosts: IronHubDefaultArtifactHosts,
    credential_accounts: Arc<dyn RuntimeCredentialAccountSelectionService>,
    catalog_source: Option<IronHubCatalogSource>,
}

impl IronhubLinkServiceImpl {
    pub fn new(
        skill_management: Arc<ScopedSkillManagementPort>,
        extension_manager: Arc<ExtensionLifecycleManager>,
        runtime_http_egress: Arc<dyn RuntimeHttpEgress>,
        shared_key: IronhubSharedKey,
        default_artifact_hosts: IronHubDefaultArtifactHosts,
        credential_accounts: Arc<dyn RuntimeCredentialAccountSelectionService>,
    ) -> Result<Self, IronhubLinkError> {
        Ok(Self {
            skill_management,
            extension_manager,
            runtime_http_egress,
            shared_key,
            install_capability: CapabilityId::new(INSTALL_CAPABILITY_ID).map_err(|error| {
                IronhubLinkError::Install {
                    reason: error.to_string(),
                }
            })?,
            default_artifact_hosts,
            credential_accounts,
            catalog_source: None,
        })
    }

    pub fn with_catalog_source(mut self, source: IronHubCatalogSource) -> Self {
        self.catalog_source = Some(source);
        self
    }

    fn install_service(&self, user_id: UserId) -> Result<IronHubService, IronhubLinkError> {
        let scope = ResourceScope::local_default(user_id, InvocationId::new()).map_err(internal)?;
        let service = IronHubService::new_with_runtime_egress(
            Arc::clone(&self.skill_management),
            Arc::clone(&self.extension_manager),
            Arc::clone(&self.runtime_http_egress),
            self.install_capability.clone(),
            scope,
            self.default_artifact_hosts.clone(),
            Arc::clone(&self.credential_accounts),
        );
        let service = match &self.catalog_source {
            Some(source) => service.with_catalog_source(source.clone()),
            None => service,
        };
        Ok(service)
    }
}

fn internal(error: impl std::fmt::Display) -> IronhubLinkError {
    IronhubLinkError::Install {
        reason: error.to_string(),
    }
}

fn map_install_error(error: IronHubCommandError) -> IronhubLinkError {
    match error {
        IronHubCommandError::InvalidInput { reason } | IronHubCommandError::Catalog { reason } => {
            IronhubLinkError::InvalidInput { reason }
        }
        other => IronhubLinkError::Install {
            reason: other.to_string(),
        },
    }
}

fn timestamp_fresh(ts: u64) -> bool {
    let Ok(ts) = i64::try_from(ts) else {
        // silent-ok: a timestamp beyond i64::MAX is never a live unix time; reject it.
        return false;
    };
    chrono::Utc::now().timestamp().abs_diff(ts) <= MAX_TIMESTAMP_DRIFT_SECS
}

fn reject_replayed_nonce(nonce: &str) -> Result<(), IronhubLinkError> {
    // Keep nonces for twice the drift window so a hub clock ahead of ours
    // cannot leave a request fresh after its nonce is evicted.
    let ttl = Duration::from_secs(MAX_TIMESTAMP_DRIFT_SECS * 2);
    let now = Instant::now();
    let mut seen = SEEN_INSTALL_NONCES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    seen.retain(|_, recorded| now.duration_since(*recorded) < ttl);
    if seen.contains_key(nonce) {
        return Err(IronhubLinkError::Replay);
    }
    seen.insert(nonce.to_string(), now);
    Ok(())
}

#[async_trait]
impl IronhubLinkService for IronhubLinkServiceImpl {
    async fn register(&self, request: IronhubRegisterRequest) -> Result<(), IronhubLinkError> {
        if !timestamp_fresh(request.ts) {
            return Err(IronhubLinkError::StaleTimestamp);
        }
        // replay-ok: register has no local side effect, so an idempotent retry is
        // harmless; single-use is enforced on deliver_install only.
        if verify_signature(&self.shared_key, &register_payload(&request), &request.sig) {
            Ok(())
        } else {
            Err(IronhubLinkError::InvalidSignature)
        }
    }

    async fn deliver_install(
        &self,
        user_id: UserId,
        request: IronhubInstallDeliveryRequest,
    ) -> Result<IronhubInstallDeliveryResult, IronhubLinkError> {
        if !timestamp_fresh(request.ts) {
            return Err(IronhubLinkError::StaleTimestamp);
        }
        if !verify_signature(&self.shared_key, &install_payload(&request), &request.sig) {
            return Err(IronhubLinkError::InvalidSignature);
        }
        reject_replayed_nonce(&request.nonce)?;

        let options = IronHubInstallOptions {
            kind: None,
            force: false,
            acknowledge_unverified: false,
            expected_version: Some(request.version),
            expected_artifact_digest: Some(request.artifact_digest),
            private_manifest_url: request.private_manifest_url,
            activate: false,
        };
        let response = self
            .install_service(user_id)?
            .execute(IronHubCommand::Install {
                name: request.slug.clone(),
                options,
            })
            .await
            .map_err(map_install_error)?;

        Ok(IronhubInstallDeliveryResult {
            installed: response.installed,
            slug: request.slug,
            message: response.message.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_fresh_accepts_now_and_rejects_stale() {
        assert!(timestamp_fresh(chrono::Utc::now().timestamp() as u64));
        assert!(!timestamp_fresh(1));
    }

    #[test]
    fn timestamp_fresh_rejects_out_of_range_timestamps_without_panicking() {
        assert!(!timestamp_fresh(u64::MAX));
        assert!(!timestamp_fresh(i64::MAX as u64 + 1));
    }

    #[test]
    fn map_install_error_classifies_invalid_and_catalog_as_invalid_input() {
        assert!(matches!(
            map_install_error(IronHubCommandError::InvalidInput {
                reason: "bad".to_string()
            }),
            IronhubLinkError::InvalidInput { .. }
        ));
        assert!(matches!(
            map_install_error(IronHubCommandError::Catalog {
                reason: "bad".to_string()
            }),
            IronhubLinkError::InvalidInput { .. }
        ));
    }

    #[test]
    fn reject_replayed_nonce_rejects_second_use() {
        let nonce = "nonce-replay-unit-unique";
        assert!(reject_replayed_nonce(nonce).is_ok());
        assert!(matches!(
            reject_replayed_nonce(nonce),
            Err(IronhubLinkError::Replay)
        ));
    }
}
