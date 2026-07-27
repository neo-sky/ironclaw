pub mod agent_link;
pub mod capabilities;
pub mod catalog;
pub mod link;
pub mod link_route;
pub mod link_service;
pub mod model;
pub mod package;
pub mod registrar;
pub mod render;
pub mod response;
pub mod service;
pub mod signature;

pub fn default_artifact_hosts() -> catalog::IronHubDefaultArtifactHosts {
    catalog::IronHubDefaultArtifactHosts {
        hosts: [
            "hub.ironclaw.com",
            "github.com",
            "objects.githubusercontent.com",
            "github-releases.githubusercontent.com",
            "raw.githubusercontent.com",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        suffixes: vec![".githubusercontent.com".to_string()],
    }
}

pub use agent_link::IronhubSharedKey;
pub use link::{
    IronhubInstallDeliveryRequest, IronhubInstallDeliveryResult, IronhubLinkError,
    IronhubLinkService, IronhubRegisterRequest,
};
pub use link_service::IronhubLinkServiceImpl;
