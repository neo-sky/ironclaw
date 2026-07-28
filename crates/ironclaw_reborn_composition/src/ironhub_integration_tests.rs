#![cfg(test)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ed25519_dalek::{Signer, SigningKey};
use ironclaw_common::hashing::sha256_hex;
use ironclaw_host_api::{
    CapabilityId, InvocationId, LifecycleProductPayload, ResourceScope, RuntimeHttpEgress,
    RuntimeHttpEgressError, RuntimeHttpEgressRequest, RuntimeHttpEgressResponse, UserId,
};
use ironclaw_ironhub::catalog::{IronHubCommand, IronHubInstallOptions};
use ironclaw_ironhub::response::{IronHubActivation, IronHubPayload, IronHubReadBack};
use ironclaw_ironhub::service::{IronHubCatalogSource, IronHubService};

use crate::factory::build_runtime_substrate;

fn manifest_url(case: &str) -> String {
    format!("https://hub.ironclaw.com/api/catalog/{case}.manifest.json")
}

const WASM_URL: &str = "https://github.com/nearai/ironhub/releases/download/v1/attio.wasm";
const CAPS_URL: &str =
    "https://github.com/nearai/ironhub/releases/download/v1/attio.capabilities.json";
const SKILL_URL: &str = "https://github.com/nearai/ironhub/releases/download/v1/reviewer.SKILL.md";
const SKILL_MD: &str = "---\nname: reviewer\ndescription: itest skill\n---\nReview things.\n";

fn component_wasm() -> Vec<u8> {
    include_bytes!("../../ironclaw_first_party_extensions/assets/slack/wasm/slack_user_tool.wasm")
        .to_vec()
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[9_u8; 32])
}

fn verify_keys() -> &'static [(&'static str, &'static str)] {
    let vk = hex::encode(signing_key().verifying_key().to_bytes());
    Box::leak(vec![("itest-key", Box::leak(vk.into_boxed_str()) as &str)].into_boxed_slice())
}

fn signed_envelope(manifest_json: &str) -> Vec<u8> {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let sig = signing_key().sign(manifest_json.as_bytes());
    serde_json::json!({
        "v": 1,
        "key_id": "itest-key",
        "manifest_b64": URL_SAFE_NO_PAD.encode(manifest_json.as_bytes()),
        "sig": URL_SAFE_NO_PAD.encode(sig.to_bytes()),
    })
    .to_string()
    .into_bytes()
}

fn skill_manifest(skill_sha: &str) -> String {
    format!(
        r#"{{"version":"1","generated_at":"2026-01-01T00:00:00Z","release_tag":"itest","repo":"nearai/ironhub","tools":[],"skills":[{{"name":"reviewer","version":"0.1.0","description":"itest skill","provenance":"official","skill_md":{{"url":"{SKILL_URL}","size_bytes":48,"sha256":"{skill_sha}"}}}}]}}"#
    )
}

fn tool_manifest(wasm_sha: &str, caps_sha: &str) -> String {
    format!(
        r#"{{"version":"1","generated_at":"2026-01-01T00:00:00Z","release_tag":"itest","repo":"nearai/ironhub","tools":[{{"name":"attio","crate_name":"attio","version":"0.1.0","description":"itest tool","provenance":"official","wasm":{{"url":"{WASM_URL}","size_bytes":8,"sha256":"{wasm_sha}"}},"capabilities":{{"url":"{CAPS_URL}","size_bytes":2,"sha256":"{caps_sha}"}}}}],"skills":[]}}"#
    )
}

struct FakeEgress {
    responses: HashMap<String, Vec<u8>>,
    fetched: Mutex<Vec<String>>,
}

impl FakeEgress {
    fn new(pairs: Vec<(&str, Vec<u8>)>) -> Arc<Self> {
        Arc::new(Self {
            responses: pairs.into_iter().map(|(u, b)| (u.to_string(), b)).collect(),
            fetched: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl RuntimeHttpEgress for FakeEgress {
    async fn execute(
        &self,
        request: RuntimeHttpEgressRequest,
    ) -> Result<RuntimeHttpEgressResponse, RuntimeHttpEgressError> {
        let url = request.url.clone();
        self.fetched
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(url.clone());
        let body = self.responses.get(&url).cloned().unwrap_or_default();
        Ok(RuntimeHttpEgressResponse {
            status: if body.is_empty() { 404 } else { 200 },
            headers: Vec::new(),
            body,
            saved_body: None,
            request_bytes: 0,
            response_bytes: 0,
            redaction_applied: false,
        })
    }

    async fn execute_credential_exchange(
        &self,
        _request: RuntimeHttpEgressRequest,
    ) -> Result<RuntimeHttpEgressResponse, RuntimeHttpEgressError> {
        Err(RuntimeHttpEgressError::Request {
            reason: "credential exchange not used in ironhub download".to_string(),
            request_bytes: 0,
            response_bytes: 0,
        })
    }
}

async fn service_with(
    dir: &std::path::Path,
    egress: Arc<FakeEgress>,
    manifest_url: String,
) -> (
    IronHubService,
    UserId,
    Arc<ironclaw_extension_host::ExtensionLifecycleManager>,
) {
    let services = build_runtime_substrate(crate::deployment::local_dev_build_input(
        "ironhub-itest-owner",
        dir.join("local-dev"),
    ))
    .await
    .expect("local-dev composed services build");
    let local = services
        .local_runtime_for_test()
        .expect("local runtime substrate");
    let user = UserId::new("ironhub-itest-owner").expect("user");
    let scope = ResourceScope::local_default(user.clone(), InvocationId::new()).expect("scope");
    let service = IronHubService::new_with_runtime_egress(
        Arc::clone(&local.skill_management),
        Arc::clone(&local.extension_management),
        egress,
        CapabilityId::new("builtin.ironhub_install").expect("cap id"),
        scope,
        ironclaw_ironhub::default_artifact_hosts(),
        services
            .product_auth
            .as_ref()
            .runtime_credential_account_selection_service(),
    )
    .with_catalog_source(IronHubCatalogSource {
        manifest_url,
        manifest_verify_keys: verify_keys(),
    });
    (service, user, Arc::clone(&local.extension_management))
}

#[tokio::test]
async fn skill_install_materializes_through_composed_skill_port() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skill = SKILL_MD.as_bytes().to_vec();
    let manifest = skill_manifest(&sha256_hex(&skill));
    let url = manifest_url("skill-install");
    let egress = FakeEgress::new(vec![
        (url.as_str(), signed_envelope(&manifest)),
        (SKILL_URL, skill),
    ]);
    let (service, _user, _extensions) = service_with(dir.path(), Arc::clone(&egress), url).await;

    let response = service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "reviewer".to_string(),
            options: IronHubInstallOptions::default(),
        })
        .await
        .expect("skill install succeeds through the real composed skill port");
    assert!(response.installed, "install reports success");
    assert!(
        egress
            .fetched
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|u| u == SKILL_URL),
        "skill markdown was fetched through the egress boundary"
    );
}

#[tokio::test]
async fn install_rejects_tampered_signature_before_any_write() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wasm = b"\x00asm\x01\x00\x00\x00".to_vec();
    let caps = b"{}".to_vec();
    let manifest = tool_manifest(&sha256_hex(&wasm), &sha256_hex(&caps));
    let mut envelope = signed_envelope(&manifest);
    let pos = envelope.len() - 5;
    envelope[pos] ^= 0x01;
    let url = manifest_url("tampered-signature");
    let egress = FakeEgress::new(vec![(url.as_str(), envelope)]);
    let (service, _user, _extensions) = service_with(dir.path(), egress, url).await;

    service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "attio".to_string(),
            options: IronHubInstallOptions::default(),
        })
        .await
        .expect_err("a tampered signature is rejected before any install");
}

#[tokio::test]
async fn install_rejects_artifact_digest_mismatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wasm = b"\x00asm\x01\x00\x00\x00".to_vec();
    let caps = b"{}".to_vec();
    let manifest = tool_manifest(&sha256_hex(b"a different artifact"), &sha256_hex(&caps));
    let url = manifest_url("digest-mismatch");
    let egress = FakeEgress::new(vec![
        (url.as_str(), signed_envelope(&manifest)),
        (WASM_URL, wasm),
        (CAPS_URL, caps),
    ]);
    let (service, user, extensions) = service_with(dir.path(), egress, url).await;

    service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "attio".to_string(),
            options: IronHubInstallOptions::default(),
        })
        .await
        .expect_err("a wasm digest mismatch is rejected, nothing installed");

    let installed = extensions
        .list_installed(&user)
        .await
        .expect("installed listing readable after the rejected install");
    let Some(LifecycleProductPayload::ExtensionList {
        extensions: records,
        ..
    }) = installed.payload
    else {
        panic!("installed listing must return an extension list payload");
    };
    assert!(
        !records
            .iter()
            .any(|record| record.summary.name.contains("attio")),
        "a rejected install must leave no half-installed record behind"
    );
}

#[tokio::test]
async fn discovery_lists_signed_catalog_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skill = SKILL_MD.as_bytes().to_vec();
    let manifest = skill_manifest(&sha256_hex(&skill));
    let url = manifest_url("discovery");
    let egress = FakeEgress::new(vec![(url.as_str(), signed_envelope(&manifest))]);
    let (service, _user, _extensions) = service_with(dir.path(), egress, url).await;

    let response = service
        .execute(IronHubCommand::Search {
            query: "reviewer".to_string(),
        })
        .await
        .expect("discovery reads the signed catalog");
    match response.payload {
        IronHubPayload::Catalog { count, skills, .. } => {
            assert_eq!(count, 1, "one matching entry discovered");
            assert_eq!(skills[0].name.as_str(), "reviewer");
        }
        other => panic!("expected catalog payload, got {other:?}"),
    }
}

#[tokio::test]
async fn skill_install_reports_read_back_and_activation_not_applicable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skill = SKILL_MD.as_bytes().to_vec();
    let manifest = skill_manifest(&sha256_hex(&skill));
    let url = manifest_url("skill-read-back");
    let egress = FakeEgress::new(vec![
        (url.as_str(), signed_envelope(&manifest)),
        (SKILL_URL, skill),
    ]);
    let (service, _user, _extensions) = service_with(dir.path(), egress, url).await;

    let response = service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "reviewer".to_string(),
            options: IronHubInstallOptions::default(),
        })
        .await
        .expect("skill install succeeds");
    match response.payload {
        IronHubPayload::Installed {
            activation,
            read_back,
            ..
        } => {
            assert_eq!(
                activation,
                IronHubActivation::NotApplicable,
                "skills are installed content and are never activated"
            );
            assert!(
                matches!(read_back, IronHubReadBack::Confirmed { .. }),
                "install is verified by reading the skill back, got {read_back:?}"
            );
        }
        other => panic!("expected installed payload, got {other:?}"),
    }
}

#[tokio::test]
async fn tool_install_defaults_to_no_activation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wasm = component_wasm();
    let caps = b"{}".to_vec();
    let manifest = tool_manifest(&sha256_hex(&wasm), &sha256_hex(&caps));
    let url = manifest_url("tool-default");
    let egress = FakeEgress::new(vec![
        (url.as_str(), signed_envelope(&manifest)),
        (WASM_URL, wasm),
        (CAPS_URL, caps),
    ]);
    let (service, _user, _extensions) = service_with(dir.path(), egress, url).await;

    let response = service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "attio".to_string(),
            options: IronHubInstallOptions::default(),
        })
        .await
        .expect("tool install succeeds");
    match response.payload {
        IronHubPayload::Installed { activation, .. } => assert_eq!(
            activation,
            IronHubActivation::NotRequested,
            "installing must never activate downloaded code implicitly"
        ),
        other => panic!("expected installed payload, got {other:?}"),
    }
}

#[tokio::test]
async fn tool_install_with_activate_performs_explicit_transition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wasm = component_wasm();
    let caps = b"{}".to_vec();
    let manifest = tool_manifest(&sha256_hex(&wasm), &sha256_hex(&caps));
    let url = manifest_url("tool-activate");
    let egress = FakeEgress::new(vec![
        (url.as_str(), signed_envelope(&manifest)),
        (WASM_URL, wasm),
        (CAPS_URL, caps),
    ]);
    let (service, _user, _extensions) = service_with(dir.path(), egress, url).await;

    let response = service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "attio".to_string(),
            options: IronHubInstallOptions {
                activate: true,
                ..IronHubInstallOptions::default()
            },
        })
        .await
        .expect("tool install succeeds");
    match response.payload {
        IronHubPayload::Installed {
            activation,
            read_back,
            ..
        } => {
            assert_eq!(
                activation,
                IronHubActivation::Active,
                "an explicit activation request transitions the tool to active"
            );
            assert!(
                matches!(
                    read_back,
                    IronHubReadBack::Confirmed { ref version } if version.as_deref() == Some("0.1.0")
                ),
                "activation is verified by reading the installed record back, got {read_back:?}"
            );
        }
        other => panic!("expected installed payload, got {other:?}"),
    }
}

#[tokio::test]
async fn discovery_marks_entries_already_installed_locally() {
    let dir = tempfile::tempdir().expect("tempdir");
    let skill = SKILL_MD.as_bytes().to_vec();
    let manifest = skill_manifest(&sha256_hex(&skill));
    let url = manifest_url("installed-marker");
    let egress = FakeEgress::new(vec![
        (url.as_str(), signed_envelope(&manifest)),
        (SKILL_URL, skill),
    ]);
    let (service, _user, _extensions) = service_with(dir.path(), egress, url).await;

    let before = service
        .execute(IronHubCommand::Search {
            query: "reviewer".to_string(),
        })
        .await
        .expect("discovery before install");
    match before.payload {
        IronHubPayload::Catalog {
            installed_skills, ..
        } => assert!(
            !installed_skills.iter().any(|name| name == "reviewer"),
            "the catalog entry is not marked installed before it is installed, got {installed_skills:?}"
        ),
        other => panic!("expected catalog payload, got {other:?}"),
    }

    service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "reviewer".to_string(),
            options: IronHubInstallOptions::default(),
        })
        .await
        .expect("skill install succeeds");

    let after = service
        .execute(IronHubCommand::Search {
            query: "reviewer".to_string(),
        })
        .await
        .expect("discovery after install");
    match after.payload {
        IronHubPayload::Catalog {
            installed_skills, ..
        } => assert!(
            installed_skills.iter().any(|name| name == "reviewer"),
            "discovery reports the entry as already installed, got {installed_skills:?}"
        ),
        other => panic!("expected catalog payload, got {other:?}"),
    }
}

fn credentialed_tool_manifest(wasm_sha: &str, caps_sha: &str) -> String {
    let credential = r#"{"handle":"itest_token","source":{"type":"product_auth_account","provider":"itest","setup":{"kind":"oauth","scopes":["read"]}},"provider_scopes":["read"],"audience":{"scheme":"https","host_pattern":"itest.example.com"},"target":{"type":"header","name":"authorization","prefix":"Bearer "},"required":true}"#;
    format!(
        r#"{{"version":"1","generated_at":"2026-01-01T00:00:00Z","release_tag":"itest","repo":"nearai/ironhub","tools":[{{"name":"attio","crate_name":"attio","version":"0.1.0","description":"itest tool","provenance":"official","wasm":{{"url":"{WASM_URL}","size_bytes":8,"sha256":"{wasm_sha}"}},"capabilities":{{"url":"{CAPS_URL}","size_bytes":2,"sha256":"{caps_sha}"}},"runtime_credentials":[{credential}]}}],"skills":[]}}"#
    )
}

#[tokio::test]
async fn activation_blocks_when_declared_credentials_are_unconfigured() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wasm = component_wasm();
    let caps = b"{}".to_vec();
    let manifest = credentialed_tool_manifest(&sha256_hex(&wasm), &sha256_hex(&caps));
    let url = manifest_url("credential-binding");
    let egress = FakeEgress::new(vec![
        (url.as_str(), signed_envelope(&manifest)),
        (WASM_URL, wasm),
        (CAPS_URL, caps),
    ]);
    let (service, _user, _extensions) = service_with(dir.path(), egress, url).await;

    let response = service
        .execute(IronHubCommand::Install {
            // safety: catalog command dispatch, not a database operation
            name: "attio".to_string(),
            options: IronHubInstallOptions {
                activate: true,
                ..IronHubInstallOptions::default()
            },
        })
        .await
        .expect("install succeeds even when activation is credential-blocked");
    match response.payload {
        IronHubPayload::Installed {
            activation,
            read_back,
            ..
        } => {
            assert!(
                matches!(
                    activation,
                    IronHubActivation::Blocked { ref blockers, .. } if blockers.contains(&"credential")
                ),
                "a tool declaring an unconfigured credential must block activation, got {activation:?}"
            );
            assert!(
                matches!(read_back, IronHubReadBack::Confirmed { .. }),
                "the tool is still installed even though activation is blocked"
            );
        }
        other => panic!("expected installed payload, got {other:?}"),
    }
}
