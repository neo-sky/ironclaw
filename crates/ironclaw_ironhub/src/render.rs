use crate::response::{IronHubActivation, IronHubPayload, IronHubReadBack, IronHubResponse};

pub fn render_reborn_ironhub_response(label: &str, response: &IronHubResponse) -> String {
    let mut output = String::new();
    push_line(&mut output, format_args!("IronHub {label}"));
    if let Some(package_ref) = &response.package_ref {
        push_line(
            &mut output,
            format_args!(
                "package: {}/{}",
                package_kind_label(package_ref.kind),
                package_ref.id.as_str()
            ),
        );
    }
    if let Some(message) = &response.message {
        push_line(
            &mut output,
            format_args!("message: {}", terminal_safe(message)),
        );
    }
    match &response.payload {
        IronHubPayload::Catalog {
            count,
            tools,
            skills,
            installed_tools,
            installed_skills,
        } => {
            push_line(&mut output, format_args!("count: {count}"));
            for tool in tools {
                let name = tool.summary.package_ref.id.as_str();
                push_line(
                    &mut output,
                    format_args!(
                        "- tool {} {} ({}){}",
                        name,
                        terminal_safe(&tool.summary.version),
                        terminal_safe(&tool.summary.description),
                        installed_suffix(installed_tools, name)
                    ),
                );
            }
            for skill in skills {
                let name = skill.name.as_str();
                push_line(
                    &mut output,
                    format_args!(
                        "- skill {} {} ({}){}",
                        name,
                        terminal_safe(&skill.version),
                        terminal_safe(&skill.description),
                        installed_suffix(installed_skills, name)
                    ),
                );
            }
        }
        IronHubPayload::Installed {
            kind,
            name,
            activation,
            read_back,
        } => {
            push_line(&mut output, format_args!("installed: true"));
            push_line(
                &mut output,
                format_args!("{}: {}", kind.as_str(), terminal_safe(name)),
            );
            push_line(
                &mut output,
                format_args!("verified: {}", read_back_label(read_back)),
            );
            match activation {
                IronHubActivation::NotRequested => {
                    push_line(&mut output, format_args!("activation: not requested"));
                }
                IronHubActivation::NotApplicable => {}
                IronHubActivation::Active => {
                    push_line(&mut output, format_args!("activation: active"));
                }
                IronHubActivation::Blocked { phase, blockers } => {
                    push_line(
                        &mut output,
                        format_args!(
                            "activation: blocked ({}) waiting on {}",
                            phase_label(*phase),
                            blockers.join(", ")
                        ),
                    );
                }
            }
        }
    }
    output
}

fn installed_suffix(installed: &[String], name: &str) -> &'static str {
    if installed.iter().any(|entry| entry == name) {
        " [installed]"
    } else {
        ""
    }
}

fn read_back_label(read_back: &IronHubReadBack) -> &'static str {
    match read_back {
        IronHubReadBack::Confirmed { .. } => "confirmed",
        IronHubReadBack::Missing => "not found after install",
    }
}

fn phase_label(phase: ironclaw_host_api::InstallationState) -> &'static str {
    match phase {
        ironclaw_host_api::InstallationState::Installed => "installed",
        ironclaw_host_api::InstallationState::Configured => "configured",
        ironclaw_host_api::InstallationState::Active => "active",
        ironclaw_host_api::InstallationState::Disabled => "disabled",
        ironclaw_host_api::InstallationState::Failed => "failed",
        ironclaw_host_api::InstallationState::Unsupported => "unsupported",
        ironclaw_host_api::InstallationState::Removed => "removed",
    }
}

fn package_kind_label(kind: ironclaw_host_api::LifecyclePackageKind) -> &'static str {
    match kind {
        ironclaw_host_api::LifecyclePackageKind::Extension => "extension",
        ironclaw_host_api::LifecyclePackageKind::Skill => "skill",
        ironclaw_host_api::LifecyclePackageKind::Mcp => "mcp",
        ironclaw_host_api::LifecyclePackageKind::Wasm => "wasm",
    }
}

fn terminal_safe(value: &str) -> String {
    value.chars().flat_map(char::escape_default).collect()
}

fn push_line(output: &mut String, args: std::fmt::Arguments<'_>) {
    use std::fmt::Write as _;
    let _ = output.write_fmt(args);
    output.push('\n');
}
