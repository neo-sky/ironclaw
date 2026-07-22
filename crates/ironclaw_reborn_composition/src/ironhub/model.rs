use std::time::Duration;

pub use ironclaw_product_workflow::{
    IronHubCommand, IronHubCommandError, IronHubEntryKind, IronHubInstallOptions,
};
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_IRONHUB_MANIFEST_URL: &str =
    "https://hub.ironclaw.com/api/catalog/manifest.json";

pub(super) const MANIFEST_VERIFY_KEYS: &[(&str, &str)] = &[(
    "5895a21abea89672",
    "f64d2d3a3228b16ca59450364d26b278071a1a425544f242504033341d8459bd",
)];
pub(super) const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_SIGNED_MANIFEST_BYTES: u64 = MAX_MANIFEST_BYTES * 2;
pub(super) const MAX_METADATA_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_WASM_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MANIFEST_CACHE_TTL: Duration = Duration::from_secs(60);
pub(super) const MANIFEST_CACHE_MAX_ENTRIES: usize = 64;
pub(super) const GENERIC_TOOL_INPUT_SCHEMA: &[u8] =
    br#"{"type":"object","additionalProperties":true}"#;
pub(super) const GENERIC_TOOL_OUTPUT_SCHEMA: &[u8] =
    br#"{"description":"Raw JSON output from the installed IronHub tool"}"#;
pub(crate) const IRONHUB_SEARCH_CAPABILITY_ID: &str = "builtin.ironhub_search";
pub(crate) const IRONHUB_INFO_CAPABILITY_ID: &str = "builtin.ironhub_info";
pub(crate) const IRONHUB_INSTALL_CAPABILITY_ID: &str = "builtin.ironhub_install";
pub(super) const IRONHUB_CAPABILITY_IDS: [&str; 3] = [
    IRONHUB_SEARCH_CAPABILITY_ID,
    IRONHUB_INFO_CAPABILITY_ID,
    IRONHUB_INSTALL_CAPABILITY_ID,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum IronHubProvenance {
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
    pub(super) fn as_wire(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Trusted => "trusted",
            Self::Verified => "verified",
            Self::Private => "private",
            Self::New => "new",
        }
    }

    pub(super) fn is_community_unverified(self) -> bool {
        matches!(self, Self::New)
    }

    pub(super) fn trust_label(self) -> &'static str {
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
pub(super) struct IronHubManifest {
    pub(super) version: String,
    pub(super) generated_at: String,
    pub(super) release_tag: String,
    pub(super) repo: String,
    #[serde(default)]
    pub(super) tools: Vec<IronHubToolEntry>,
    #[serde(default)]
    pub(super) skills: Vec<IronHubSkillEntry>,
}

impl IronHubManifest {
    pub(super) fn find_tool(&self, name: &str) -> Option<&IronHubToolEntry> {
        self.tools.iter().find(|entry| entry.name == name)
    }

    pub(super) fn find_skill(&self, name: &str) -> Option<&IronHubSkillEntry> {
        self.skills.iter().find(|entry| entry.name == name)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct IronHubToolEntry {
    pub(super) name: String,
    pub(super) crate_name: String,
    pub(super) version: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) provenance: IronHubProvenance,
    pub(super) wasm: IronHubArtifact,
    pub(super) capabilities: IronHubArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct IronHubSkillEntry {
    pub(super) name: String,
    #[serde(default)]
    pub(super) trunk: String,
    #[serde(default)]
    pub(super) version: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) provenance: IronHubProvenance,
    pub(super) skill_md: IronHubArtifact,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct IronHubArtifact {
    pub(super) url: String,
    pub(super) size_bytes: u64,
    pub(super) sha256: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SignedManifestEnvelope {
    pub(super) v: u8,
    pub(super) key_id: String,
    pub(super) manifest_b64: String,
    pub(super) sig: String,
}
