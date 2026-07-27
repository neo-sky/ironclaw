use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

use crate::catalog::{IronHubCommandError, IronHubToolEntry, validate_hub_name};
use crate::model::{GENERIC_TOOL_INPUT_SCHEMA, GENERIC_TOOL_OUTPUT_SCHEMA};

pub(crate) fn ironhub_tool_bundle_zip(
    entry: &IronHubToolEntry,
    wasm: &[u8],
) -> Result<Vec<u8>, IronHubCommandError> {
    validate_hub_name(&entry.name)?;
    let manifest_toml = generic_tool_manifest(entry);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    let entries: [(String, &[u8]); 4] = [
        ("manifest.toml".to_string(), manifest_toml.as_bytes()),
        (format!("wasm/{}_tool.wasm", entry.name), wasm),
        (
            format!("schemas/{}/invoke.input.v1.json", entry.name),
            GENERIC_TOOL_INPUT_SCHEMA,
        ),
        (
            format!("schemas/{}/raw_output.v1.json", entry.name),
            GENERIC_TOOL_OUTPUT_SCHEMA,
        ),
    ];
    for (path, bytes) in entries {
        writer
            .start_file(path, options)
            .map_err(|error| bundle_error(error.to_string()))?;
        writer
            .write_all(bytes)
            .map_err(|error| bundle_error(error.to_string()))?;
    }
    let cursor = writer
        .finish()
        .map_err(|error| bundle_error(error.to_string()))?;
    Ok(cursor.into_inner())
}

fn bundle_error(reason: impl Into<String>) -> IronHubCommandError {
    IronHubCommandError::Install {
        reason: reason.into(),
    }
}

fn generic_tool_manifest(entry: &IronHubToolEntry) -> String {
    format!(
        r#"schema_version = "reborn.extension_manifest.v2"
id = {id}
name = {name}
version = {version}
description = {description}
trust = "third_party"

[runtime]
kind = "wasm"
module = {module}

[[host_api]]
id = "ironclaw.capability_provider/v1"
section = "capability_provider.tools"

[capability_provider.tools]

[[capability_provider.tools.capabilities]]
id = {capability_id}
description = {description}
effects = ["dispatch_capability", "network"]
default_permission = "ask"
visibility = "model"
input_schema_ref = {input_schema_ref}
output_schema_ref = {output_schema_ref}
required_host_ports = ["host.runtime.http_egress"]
"#,
        id = toml_string(&entry.name),
        name = toml_string(&entry.name),
        version = toml_string(&entry.version),
        description = toml_string(&entry.description),
        module = toml_string(format!("wasm/{}_tool.wasm", entry.name)),
        capability_id = toml_string(format!("{}.invoke", entry.name)),
        input_schema_ref = toml_string(format!("schemas/{}/invoke.input.v1.json", entry.name)),
        output_schema_ref = toml_string(format!("schemas/{}/raw_output.v1.json", entry.name)),
    )
}

fn toml_string(value: impl Into<String>) -> String {
    toml::Value::String(value.into()).to_string()
}

#[cfg(test)]
mod tests {
    use super::generic_tool_manifest;
    use crate::catalog::{IronHubArtifact, IronHubProvenance, IronHubToolEntry};

    #[test]
    fn generic_tool_manifest_uses_toml_escaping_for_catalog_strings() {
        let manifest = generic_tool_manifest(&IronHubToolEntry {
            name: "quote_tool".to_string(),
            crate_name: "quote_tool".to_string(),
            version: "0.1.0".to_string(),
            description: "quote \" slash \\ newline\nok".to_string(),
            provenance: IronHubProvenance::Official,
            wasm: IronHubArtifact {
                url: "https://hub.ironclaw.com/quote_tool.wasm".to_string(),
                size_bytes: 1,
                sha256: "a".repeat(64),
            },
            capabilities: IronHubArtifact {
                url: "https://hub.ironclaw.com/quote_tool.capabilities.json".to_string(),
                size_bytes: 1,
                sha256: "b".repeat(64),
            },
        });
        let parsed: toml::Value = toml::from_str(&manifest).expect("manifest TOML parses");
        assert_eq!(parsed["id"].as_str(), Some("quote_tool"));
        assert_eq!(
            parsed["runtime"]["module"].as_str(),
            Some("wasm/quote_tool_tool.wasm")
        );
    }
}
