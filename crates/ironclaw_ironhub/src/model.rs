use std::time::Duration;

pub use crate::catalog::{
    IronHubCommand, IronHubCommandError, IronHubEntryKind, IronHubInstallOptions,
};
use serde::Deserialize;

pub(crate) const DEFAULT_IRONHUB_MANIFEST_URL: &str =
    "https://hub.ironclaw.com/api/catalog/manifest.json";

pub(crate) const MANIFEST_VERIFY_KEYS: &[(&str, &str)] = &[(
    "5895a21abea89672",
    "f64d2d3a3228b16ca59450364d26b278071a1a425544f242504033341d8459bd",
)];
pub(crate) const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_SIGNED_MANIFEST_BYTES: u64 = MAX_MANIFEST_BYTES * 2;
pub(crate) const MAX_METADATA_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_WASM_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MANIFEST_CACHE_TTL: Duration = Duration::from_secs(60);
pub(crate) const MANIFEST_CACHE_MAX_ENTRIES: usize = 64;
pub(crate) const GENERIC_TOOL_INPUT_SCHEMA: &[u8] =
    br#"{"type":"object","additionalProperties":true}"#;
pub(crate) const GENERIC_TOOL_OUTPUT_SCHEMA: &[u8] =
    br#"{"description":"Raw JSON output from the installed IronHub tool"}"#;
pub(crate) const IRONHUB_SEARCH_CAPABILITY_ID: &str = "builtin.ironhub_search";
pub(crate) const IRONHUB_INFO_CAPABILITY_ID: &str = "builtin.ironhub_info";
pub(crate) const IRONHUB_INSTALL_CAPABILITY_ID: &str = "builtin.ironhub_install";
pub(crate) const IRONHUB_CAPABILITY_IDS: [&str; 3] = [
    IRONHUB_SEARCH_CAPABILITY_ID,
    IRONHUB_INFO_CAPABILITY_ID,
    IRONHUB_INSTALL_CAPABILITY_ID,
];

#[derive(Debug, Deserialize)]
pub(crate) struct SignedManifestEnvelope {
    pub(crate) v: u8,
    pub(crate) key_id: String,
    pub(crate) manifest_b64: String,
    pub(crate) sig: String,
}
