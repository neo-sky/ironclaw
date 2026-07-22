use std::sync::Arc;

use async_trait::async_trait;
use ironclaw_product_workflow::{
    IronHubCatalogService, IronHubCommand, IronHubCommandError, LifecycleProductResponse,
};

use crate::factory::RebornServices;

use super::service::execute_reborn_ironhub_command;

/// Composition-side wiring for the product-owned IronHub catalog port. Domain
/// policy (signature verification, artifact validation, lifecycle transitions)
/// is reached through the services this holds; this type only adapts the
/// product port onto them.
pub struct RebornIronHubCatalogService {
    services: Arc<RebornServices>,
}

impl RebornIronHubCatalogService {
    pub fn new(services: Arc<RebornServices>) -> Self {
        Self { services }
    }
}

#[async_trait]
impl IronHubCatalogService for RebornIronHubCatalogService {
    async fn execute(
        &self,
        command: IronHubCommand,
    ) -> Result<LifecycleProductResponse, IronHubCommandError> {
        execute_reborn_ironhub_command(self.services.as_ref(), command).await
    }
}
