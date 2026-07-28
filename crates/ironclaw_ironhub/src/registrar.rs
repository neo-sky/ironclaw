use std::sync::Arc;

use ironclaw_extension_host::{
    CapabilityManifest, ExtensionError, FirstPartyCapabilityRegistry, FirstPartyHandlerRegistrar,
    FirstPartyRegistrarContext,
};
use ironclaw_host_api::HostApiError;

use crate::capabilities::{capability_manifests, insert_handlers};
use crate::default_artifact_hosts;

pub struct IronhubHandlerRegistrar;

impl FirstPartyHandlerRegistrar for IronhubHandlerRegistrar {
    fn register(
        &self,
        registry: &mut FirstPartyCapabilityRegistry,
        context: &FirstPartyRegistrarContext,
    ) -> Result<(), HostApiError> {
        insert_handlers(
            registry,
            Arc::clone(&context.skill_management),
            Arc::clone(&context.extension_management),
            Arc::clone(&context.credential_accounts),
            default_artifact_hosts(),
        )
    }

    fn capability_manifests(&self) -> Result<Vec<CapabilityManifest>, ExtensionError> {
        capability_manifests()
    }
}
