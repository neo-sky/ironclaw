use ironclaw_host_api::{
    InstallationState, LifecyclePackageRef, LifecycleSearchExtensionSummary, LifecycleSkillSummary,
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
        installed_tools: Vec<String>,
        installed_skills: Vec<String>,
    },
    Installed {
        kind: IronHubEntryKind,
        name: String,
        activation: IronHubActivation,
        read_back: IronHubReadBack,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum IronHubActivation {
    NotRequested,
    NotApplicable,
    Active,
    Blocked {
        phase: InstallationState,
        blockers: Vec<&'static str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum IronHubReadBack {
    Confirmed { version: Option<String> },
    Missing,
}

impl IronHubResponse {
    pub(crate) fn catalog(
        tools: Vec<LifecycleSearchExtensionSummary>,
        skills: Vec<LifecycleSkillSummary>,
        installed_tools: Vec<String>,
        installed_skills: Vec<String>,
    ) -> Self {
        Self {
            package_ref: None,
            installed: false,
            message: None,
            payload: IronHubPayload::Catalog {
                count: tools.len() + skills.len(),
                tools,
                skills,
                installed_tools,
                installed_skills,
            },
        }
    }

    pub(crate) fn installed(
        package_ref: LifecyclePackageRef,
        kind: IronHubEntryKind,
        name: String,
        message: String,
        activation: IronHubActivation,
        read_back: IronHubReadBack,
    ) -> Self {
        Self {
            package_ref: Some(package_ref),
            installed: true,
            message: Some(message),
            payload: IronHubPayload::Installed {
                kind,
                name,
                activation,
                read_back,
            },
        }
    }
}
