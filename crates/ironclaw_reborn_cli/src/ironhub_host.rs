use std::sync::Arc;

use ironclaw_host_api::{CapabilityId, InvocationId, ResourceScope};
use ironclaw_ironhub::catalog::{
    IronHubCommand, IronHubCommandError, ironhub_install_error, ironhub_invalid_input,
};
use ironclaw_ironhub::response::IronHubResponse;
use ironclaw_ironhub::service::IronHubService;
use ironclaw_reborn_composition::RebornRuntime;
use ironclaw_webui::PublicRouteMount;

const CATALOG_FETCH_CAPABILITY_ID: &str = "builtin.ironhub_fetch";
const SHARED_KEY_ENV: &str = "IRONHUB_AGENT_SHARED_KEY";

pub(crate) async fn execute_catalog_command(
    runtime: &RebornRuntime,
    command: IronHubCommand,
) -> Result<IronHubResponse, IronHubCommandError> {
    let egress = runtime
        .runtime_http_egress()
        .ok_or_else(|| ironhub_install_error("runtime http egress is unavailable"))?;
    let scope = ResourceScope::local_default(runtime.owner_user_id().clone(), InvocationId::new())
        .map_err(|error| ironhub_invalid_input(error.to_string()))?;
    let capability_id = CapabilityId::new(CATALOG_FETCH_CAPABILITY_ID)
        .map_err(|error| ironhub_invalid_input(error.to_string()))?;
    let service = IronHubService::new_with_runtime_egress(
        runtime.skill_management(),
        runtime.extension_management(),
        egress,
        capability_id,
        scope,
        ironclaw_ironhub::default_artifact_hosts(),
    );
    service.execute(command).await
}

pub(crate) fn link_route_mount(runtime: &RebornRuntime) -> Option<PublicRouteMount> {
    let shared_key = std::env::var(SHARED_KEY_ENV)
        .ok()
        .and_then(|raw| ironclaw_ironhub::IronhubSharedKey::new(raw.trim()).ok())?;
    let egress = runtime.runtime_http_egress()?;
    let link = ironclaw_ironhub::IronhubLinkServiceImpl::new(
        runtime.skill_management(),
        runtime.extension_management(),
        egress,
        shared_key,
        ironclaw_ironhub::default_artifact_hosts(),
    )
    .ok()?;
    Some(ironclaw_ironhub::link_route::link_route_mount(
        Arc::new(link),
        runtime.owner_user_id().clone(),
    ))
}
