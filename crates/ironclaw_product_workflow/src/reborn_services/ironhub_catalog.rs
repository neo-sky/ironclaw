use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::ProductWorkflowError;
use crate::lifecycle::LifecycleProductResponse;

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
    #[error("IronHub lifecycle failed: {0}")]
    Product(#[from] ProductWorkflowError),
}

/// Product-facing IronHub catalog lifecycle: discovery, inspection, and
/// install. Host composition supplies the implementation that reaches the
/// signed catalog and the extension lifecycle; product callers and adapters
/// depend only on this port.
#[async_trait]
pub trait IronHubCatalogService: Send + Sync {
    async fn execute(
        &self,
        command: IronHubCommand,
    ) -> Result<LifecycleProductResponse, IronHubCommandError>;
}
