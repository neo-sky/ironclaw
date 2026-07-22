use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use ironclaw_extensions::{
    CapabilityManifest, CapabilityVisibility, ExtensionError, ExtensionPackage,
};
use ironclaw_host_api::{
    CapabilityId, CapabilityProfileSchemaRef, EffectKind, HostPortId, OriginGateMatrix,
    PermissionMode, ResourceEstimate, ResourceProfile, ResourceUsage, RuntimeDispatchErrorKind,
};
use ironclaw_host_runtime::{
    FirstPartyCapabilityError, FirstPartyCapabilityHandler, FirstPartyCapabilityRegistry,
    FirstPartyCapabilityRequest, FirstPartyCapabilityResult,
};
use serde::Deserialize;

use crate::extension_host::extension_lifecycle::RebornLocalExtensionManagementPort;
use crate::extension_host::lifecycle::RebornLocalSkillManagementPort;

use super::model::{
    IRONHUB_CAPABILITY_IDS, IRONHUB_INFO_CAPABILITY_ID, IRONHUB_INSTALL_CAPABILITY_ID,
    IRONHUB_SEARCH_CAPABILITY_ID, IronHubCommand, IronHubCommandError, IronHubEntryKind,
    IronHubInstallOptions,
};
use super::service::IronHubService;

pub(crate) fn extend_builtin_first_party_package(
    mut package: ExtensionPackage,
) -> Result<ExtensionPackage, ExtensionError> {
    package
        .manifest
        .capabilities
        .extend(capability_manifests()?);
    ExtensionPackage::from_manifest(package.manifest, package.root)
}

pub(crate) fn insert_handlers(
    registry: &mut FirstPartyCapabilityRegistry,
    skill_management: Arc<RebornLocalSkillManagementPort>,
    extension_management: Arc<RebornLocalExtensionManagementPort>,
) -> Result<(), ironclaw_host_api::HostApiError> {
    let handler = Arc::new(IronHubCapabilityHandler {
        skill_management,
        extension_management,
    });
    for capability_id in IRONHUB_CAPABILITY_IDS {
        registry.insert_handler(CapabilityId::new(capability_id)?, handler.clone());
    }
    Ok(())
}

fn capability_manifests() -> Result<Vec<CapabilityManifest>, ExtensionError> {
    Ok(vec![
        capability_manifest(
            IRONHUB_SEARCH_CAPABILITY_ID,
            "Search the signed IronHub catalog for tools and skills",
            vec![EffectKind::Network],
            PermissionMode::Allow,
        )?,
        capability_manifest(
            IRONHUB_INFO_CAPABILITY_ID,
            "Inspect one signed IronHub catalog entry",
            vec![EffectKind::Network],
            PermissionMode::Allow,
        )?,
        capability_manifest(
            IRONHUB_INSTALL_CAPABILITY_ID,
            "Install a tool or skill from the signed IronHub catalog into Reborn local-dev state",
            vec![EffectKind::Network, EffectKind::WriteFilesystem],
            PermissionMode::Ask,
        )?,
    ])
}

fn capability_manifest(
    id: &str,
    description: &str,
    effects: Vec<EffectKind>,
    default_permission: PermissionMode,
) -> Result<CapabilityManifest, ExtensionError> {
    let schema_name = id.strip_prefix("builtin.").unwrap_or(id).replace('.', "-");
    Ok(CapabilityManifest {
        id: CapabilityId::new(id)?,
        implements: Vec::new(),
        description: description.to_string(),
        effects,
        network_targets: Vec::new(),
        default_permission,
        visibility: CapabilityVisibility::Model,
        input_schema_ref: CapabilityProfileSchemaRef::new(format!(
            "schemas/builtin/{schema_name}.input.v1.json"
        ))?,
        output_schema_ref: CapabilityProfileSchemaRef::new(format!(
            "schemas/builtin/{schema_name}.output.v1.json"
        ))?,
        prompt_doc_ref: None,
        required_host_ports: vec![HostPortId::new("host.runtime.http_egress")?],
        runtime_credentials: Vec::new(),
        resource_profile: Some(ResourceProfile {
            default_estimate: ResourceEstimate {
                wall_clock_ms: Some(1_000),
                output_bytes: Some(32 * 1024),
                ..ResourceEstimate::default()
            },
            hard_ceiling: None,
        }),
        origin_gate_matrix: Some(OriginGateMatrix::builtin_loop_run_seed(id)),
    })
}

struct IronHubCapabilityHandler {
    skill_management: Arc<RebornLocalSkillManagementPort>,
    extension_management: Arc<RebornLocalExtensionManagementPort>,
}

#[derive(Debug, Deserialize)]
struct SearchInput {
    #[serde(default)]
    query: String,
}

#[derive(Debug, Deserialize)]
struct InfoInput {
    name: String,
    #[serde(default)]
    kind: Option<IronHubEntryKind>,
}

#[derive(Debug, Deserialize)]
struct InstallInput {
    name: String,
    #[serde(default)]
    kind: Option<IronHubEntryKind>,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    expected_version: Option<String>,
    #[serde(default)]
    expected_artifact_digest: Option<String>,
}

#[async_trait]
impl FirstPartyCapabilityHandler for IronHubCapabilityHandler {
    async fn dispatch(
        &self,
        request: FirstPartyCapabilityRequest,
    ) -> Result<FirstPartyCapabilityResult, FirstPartyCapabilityError> {
        let started = Instant::now();
        let Some(runtime_http_egress) = request.services.runtime_http_egress.clone() else {
            return Err(FirstPartyCapabilityError::new(
                RuntimeDispatchErrorKind::Executor,
            ));
        };
        let capability_id = request.capability_id.clone();
        let command = model_invoked_command(capability_id.as_str(), request.input)?;
        let service = IronHubService::new_with_runtime_egress(
            Arc::clone(&self.skill_management),
            Arc::clone(&self.extension_management),
            runtime_http_egress,
            capability_id,
            request.scope,
        );
        let response = service.execute(command).await.map_err(capability_error)?;
        let output = serde_json::to_value(response)
            .map_err(|_| FirstPartyCapabilityError::new(RuntimeDispatchErrorKind::OutputDecode))?;
        Ok(FirstPartyCapabilityResult::new(
            output,
            ResourceUsage {
                wall_clock_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                ..ResourceUsage::default()
            },
        ))
    }
}

/// Maps a model-invoked capability request onto an [`IronHubCommand`]. Model
/// callers reach the public catalog only: they cannot name a private manifest
/// and cannot waive the unverified-provenance gate, so both options are fixed
/// here rather than read from the model-supplied input.
fn model_invoked_command(
    capability_id: &str,
    input: serde_json::Value,
) -> Result<IronHubCommand, FirstPartyCapabilityError> {
    match capability_id {
        IRONHUB_SEARCH_CAPABILITY_ID => {
            let input: SearchInput = parse_capability_input(input)?;
            Ok(IronHubCommand::Search { query: input.query })
        }
        IRONHUB_INFO_CAPABILITY_ID => {
            let input: InfoInput = parse_capability_input(input)?;
            Ok(IronHubCommand::Info {
                name: input.name,
                kind: input.kind,
            })
        }
        IRONHUB_INSTALL_CAPABILITY_ID => {
            let input: InstallInput = parse_capability_input(input)?;
            Ok(IronHubCommand::Install {
                name: input.name,
                options: IronHubInstallOptions {
                    kind: input.kind,
                    force: input.force,
                    acknowledge_unverified: false,
                    expected_version: input.expected_version,
                    expected_artifact_digest: input.expected_artifact_digest,
                    private_manifest_url: None,
                },
            })
        }
        _ => Err(FirstPartyCapabilityError::new(
            RuntimeDispatchErrorKind::UndeclaredCapability,
        )),
    }
}

fn parse_capability_input<T>(input: serde_json::Value) -> Result<T, FirstPartyCapabilityError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(input)
        .map_err(|_| FirstPartyCapabilityError::new(RuntimeDispatchErrorKind::InputEncode))
}

fn capability_error(error: IronHubCommandError) -> FirstPartyCapabilityError {
    let kind = match error {
        IronHubCommandError::InvalidInput { .. } => RuntimeDispatchErrorKind::InputEncode,
        IronHubCommandError::LocalRuntimeUnavailable
        | IronHubCommandError::RuntimeHttpEgressUnavailable => RuntimeDispatchErrorKind::Executor,
        IronHubCommandError::Catalog { .. }
        | IronHubCommandError::Install { .. }
        | IronHubCommandError::Product(_) => RuntimeDispatchErrorKind::OperationFailed,
    };
    FirstPartyCapabilityError::new(kind)
}

#[cfg(test)]
mod tests {
    use ironclaw_host_api::{OriginGatePolicy, UNGATED_LOOP_RUN_CAPABILITIES};

    use super::*;

    /// Mirrors `extension_lifecycle_capabilities_declare_behavior_neutral_origin_gate_matrix`.
    /// No IronHub capability is read-only enough for the reviewed Ungated
    /// allowlist: search and info reach the network, install also writes the
    /// filesystem. All three gate for `LoopRun`; Product/Automation are
    /// deny-by-default until a reviewed ingress slice declares a producer.
    #[test]
    fn ironhub_capabilities_declare_behavior_neutral_origin_gate_matrix() {
        let manifests = capability_manifests().expect("ironhub capability manifests build");
        assert_eq!(manifests.len(), IRONHUB_CAPABILITY_IDS.len());
        for manifest in &manifests {
            let matrix = manifest
                .origin_gate_matrix
                .as_ref()
                .unwrap_or_else(|| panic!("{} must declare an origin_gate_matrix", manifest.id));
            assert_eq!(
                matrix.product,
                OriginGatePolicy::Forbidden,
                "{}",
                manifest.id
            );
            assert_eq!(
                matrix.automation,
                OriginGatePolicy::Forbidden,
                "{}",
                manifest.id
            );
            assert_eq!(
                matrix.loop_run,
                OriginGatePolicy::GatedUnlessGranted,
                "{}",
                manifest.id
            );
        }
        for id in IRONHUB_CAPABILITY_IDS {
            assert!(
                !UNGATED_LOOP_RUN_CAPABILITIES.contains(&id),
                "{id} must not be in the Ungated allowlist"
            );
        }
    }

    /// The model-invoked install path must not be able to reach a private
    /// manifest or waive the unverified-provenance gate, even when the model
    /// supplies those fields. Both are host-fixed; only the signed deep-link
    /// and the operator CLI may set them.
    #[test]
    fn model_invoked_install_cannot_reach_private_manifest_or_waive_verification() {
        let command = model_invoked_command(
            IRONHUB_INSTALL_CAPABILITY_ID,
            serde_json::json!({
                "name": "some-tool",
                "force": true,
                "private_manifest_url": "https://hub.ironclaw.com/private/manifest.json",
                "acknowledge_unverified": true,
            }),
        )
        .expect("install input maps");

        let IronHubCommand::Install { name, options } = command else {
            panic!("install capability must map to an install command");
        };
        assert_eq!(name, "some-tool");
        assert!(options.force, "force is model-settable");
        assert_eq!(
            options.private_manifest_url, None,
            "model input must not reach a private manifest"
        );
        assert!(
            !options.acknowledge_unverified,
            "model input must not waive the unverified-provenance gate"
        );
    }

    #[test]
    fn model_invoked_search_and_info_map_to_catalog_reads() {
        let search = model_invoked_command(
            IRONHUB_SEARCH_CAPABILITY_ID,
            serde_json::json!({ "query": "wasm" }),
        )
        .expect("search input maps");
        assert!(matches!(search, IronHubCommand::Search { query } if query == "wasm"));

        let info = model_invoked_command(
            IRONHUB_INFO_CAPABILITY_ID,
            serde_json::json!({ "name": "some-skill", "kind": "skill" }),
        )
        .expect("info input maps");
        assert!(matches!(
            info,
            IronHubCommand::Info { name, kind }
                if name == "some-skill" && kind == Some(IronHubEntryKind::Skill)
        ));
    }

    #[test]
    fn model_invoked_unknown_capability_is_rejected() {
        let error =
            model_invoked_command("builtin.ironhub_not_a_capability", serde_json::json!({}))
                .expect_err("unknown capability id must be rejected");
        assert_eq!(
            error.kind(),
            Some(RuntimeDispatchErrorKind::UndeclaredCapability)
        );
    }

    #[test]
    fn model_invoked_malformed_input_is_rejected() {
        let error = model_invoked_command(
            IRONHUB_INSTALL_CAPABILITY_ID,
            serde_json::json!({ "force": true }),
        )
        .expect_err("install input without a name must be rejected");
        assert_eq!(error.kind(), Some(RuntimeDispatchErrorKind::InputEncode));
    }
}
