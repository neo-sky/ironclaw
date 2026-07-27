use ironclaw_host_api::{
    LifecyclePackageRef, LifecycleSearchExtensionSummary, LifecycleSkillSummary,
};

use crate::catalog::IronHubEntryKind;

#[derive(Debug, Clone, serde::Serialize)]
pub struct IronHubResponse {
    pub package_ref: Option<LifecyclePackageRef>,
    pub installed: bool,
    pub message: Option<String>,
    pub payload: IronHubPayload,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum IronHubPayload {
    Catalog {
        count: usize,
        tools: Vec<LifecycleSearchExtensionSummary>,
        skills: Vec<LifecycleSkillSummary>,
    },
    Installed {
        kind: IronHubEntryKind,
        name: String,
    },
}

impl IronHubResponse {
    pub(crate) fn catalog(
        tools: Vec<LifecycleSearchExtensionSummary>,
        skills: Vec<LifecycleSkillSummary>,
    ) -> Self {
        Self {
            package_ref: None,
            installed: false,
            message: None,
            payload: IronHubPayload::Catalog {
                count: tools.len() + skills.len(),
                tools,
                skills,
            },
        }
    }

    pub(crate) fn installed(
        package_ref: LifecyclePackageRef,
        kind: IronHubEntryKind,
        name: String,
        message: String,
    ) -> Self {
        Self {
            package_ref: Some(package_ref),
            installed: true,
            message: Some(message),
            payload: IronHubPayload::Installed { kind, name },
        }
    }
}
