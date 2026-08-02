use serde::{Deserialize, Serialize};

use super::model::{IronHubCommandError, IronHubEntryKind};

pub(crate) const VERSION_INDEX_FILE: &str = "versions.json";
pub(crate) const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct IronHubVersionEntry {
    pub(crate) kind: IronHubEntryKind,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) digest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct IronHubVersionIndex {
    pub(crate) version: String,
    pub(crate) generated_at: String,
    pub(crate) release_tag: String,
    pub(crate) repo: String,
    #[serde(default)]
    pub(crate) unchanged: bool,
    #[serde(default)]
    pub(crate) entries: Vec<IronHubVersionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledEntry {
    pub(crate) kind: IronHubEntryKind,
    pub(crate) name: String,
    pub(crate) version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutdatedEntry {
    pub(crate) kind: IronHubEntryKind,
    pub(crate) name: String,
    pub(crate) installed_version: String,
    pub(crate) catalog_version: String,
}

pub(crate) fn version_index_url(manifest_url: &str) -> Result<String, IronHubCommandError> {
    let mut parsed = url::Url::parse(manifest_url)
        .map_err(|error| invalid_index(format!("manifest URL is not a valid URL: {error}")))?;
    let file = parsed
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .ok_or_else(|| invalid_index("manifest URL has no path segment"))?;
    if file != MANIFEST_FILE {
        return Err(invalid_index(format!(
            "manifest URL must end in '{MANIFEST_FILE}' to derive a version index"
        )));
    }
    parsed
        .path_segments_mut()
        .map_err(|()| invalid_index("manifest URL cannot carry a path"))?
        .pop()
        .push(VERSION_INDEX_FILE);
    Ok(parsed.into())
}

pub(crate) fn validate_version_index(
    index: &IronHubVersionIndex,
) -> Result<(), IronHubCommandError> {
    if index.version != "1" {
        return Err(invalid_index(format!(
            "unsupported version index version {}",
            index.version
        )));
    }
    if index.release_tag.trim().is_empty() || index.repo.trim().is_empty() {
        return Err(invalid_index(
            "version index release_tag and repo must be non-empty",
        ));
    }
    if index.unchanged && !index.entries.is_empty() {
        return Err(invalid_index(
            "version index claims unchanged but carries entries",
        ));
    }
    for entry in &index.entries {
        if entry.name.trim().is_empty() {
            return Err(invalid_index("version index entry has an empty name"));
        }
        if entry.digest.trim().is_empty() {
            return Err(invalid_index(format!(
                "version index entry '{}' has an empty digest",
                entry.name
            )));
        }
    }
    Ok(())
}

pub(crate) fn diff_installed(
    installed: &[InstalledEntry],
    index: &IronHubVersionIndex,
) -> Vec<OutdatedEntry> {
    let mut diffs: Vec<OutdatedEntry> = installed
        .iter()
        .filter_map(|entry| {
            let current = index
                .entries
                .iter()
                .find(|candidate| candidate.kind == entry.kind && candidate.name == entry.name)?;
            if current.version == entry.version {
                return None;
            }
            Some(OutdatedEntry {
                kind: entry.kind,
                name: entry.name.clone(),
                installed_version: entry.version.clone(),
                catalog_version: current.version.clone(),
            })
        })
        .collect();

    diffs.sort_by(|left, right| {
        left.kind
            .as_str()
            .cmp(right.kind.as_str())
            .then_with(|| left.name.cmp(&right.name))
    });
    diffs
}

fn invalid_index(reason: impl Into<String>) -> IronHubCommandError {
    IronHubCommandError::Catalog {
        reason: reason.into(),
    }
}
