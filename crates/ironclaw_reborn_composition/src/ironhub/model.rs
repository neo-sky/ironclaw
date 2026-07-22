use std::time::Duration;

pub(super) use ironclaw_product_workflow::{
    IronHubArtifact, IronHubManifest, IronHubProvenance, IronHubToolEntry,
};
pub use ironclaw_product_workflow::{
    IronHubCommand, IronHubCommandError, IronHubEntryKind, IronHubInstallOptions,
};
use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
pub(super) struct SignedManifestEnvelope {
    pub(super) v: u8,
    pub(super) key_id: String,
    pub(super) manifest_b64: String,
    pub(super) sig: String,
}
