use crate::response::{IronHubPayload, IronHubResponse};

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
        } => {
            push_line(&mut output, format_args!("count: {count}"));
            for tool in tools {
                push_line(
                    &mut output,
                    format_args!(
                        "- tool {} {} ({})",
                        tool.summary.package_ref.id.as_str(),
                        terminal_safe(&tool.summary.version),
                        terminal_safe(&tool.summary.description)
                    ),
                );
            }
            for skill in skills {
                push_line(
                    &mut output,
                    format_args!(
                        "- skill {} {} ({})",
                        skill.name.as_str(),
                        terminal_safe(&skill.version),
                        terminal_safe(&skill.description)
                    ),
                );
            }
        }
        IronHubPayload::Installed { kind, name } => {
            push_line(&mut output, format_args!("installed: true"));
            push_line(
                &mut output,
                format_args!("{}: {}", kind.as_str(), terminal_safe(name)),
            );
        }
    }
    output
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
