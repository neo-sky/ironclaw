use anyhow::Context;
use clap::{Args, Subcommand, ValueEnum};
use ironclaw_ironhub::catalog::{
    IronHubCommand as RebornIronHubCommand, IronHubEntryKind, IronHubInstallOptions,
};
use ironclaw_ironhub::render::render_reborn_ironhub_response;
use ironclaw_ironhub::response::IronHubResponse;
use ironclaw_reborn_composition::{RebornRuntimeInput, build_reborn_runtime};

use crate::context::RebornCliContext;
use crate::runtime::{RuntimeInputCaller, RuntimeInputOptions};

#[derive(Debug, Args)]
pub(crate) struct IronHubCommand {
    /// Confirm trusted-laptop host filesystem access for local-dev-yolo.
    #[arg(long = "confirm-host-access", global = true)]
    confirm_host_access: bool,

    #[command(subcommand)]
    command: IronHubSubcommand,
}

#[derive(Debug, Subcommand)]
enum IronHubSubcommand {
    /// Search the signed IronHub catalog.
    Search(SearchCommand),
    /// List IronHub catalog tools or skills.
    List(ListCommand),
    /// Show one IronHub catalog entry.
    Info(InfoCommand),
    /// Install a tool or skill from the signed IronHub catalog.
    Install(InstallCommand),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum KindArg {
    Tool,
    Skill,
}

impl From<KindArg> for IronHubEntryKind {
    fn from(kind: KindArg) -> Self {
        match kind {
            KindArg::Tool => IronHubEntryKind::Tool,
            KindArg::Skill => IronHubEntryKind::Skill,
        }
    }
}

#[derive(Debug, Args)]
struct SearchCommand {
    /// Query by name or description. Omit to list the whole catalog.
    query: Option<String>,
    /// Output the response as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ListCommand {
    /// Limit to tools or skills.
    #[arg(long, value_enum)]
    kind: Option<KindArg>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct InfoCommand {
    /// Catalog entry name.
    name: String,
    #[arg(long, value_enum)]
    kind: Option<KindArg>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct InstallCommand {
    /// Catalog entry name.
    name: String,
    #[arg(long, value_enum)]
    kind: Option<KindArg>,
    /// Replace an already-installed package (operator override).
    #[arg(long)]
    force: bool,
    /// Acknowledge installing unverified community content.
    #[arg(long)]
    acknowledge_unverified: bool,
    /// Require the catalog entry to still be this version.
    #[arg(long)]
    expected_version: Option<String>,
    /// Require the catalog entry to still have this artifact digest.
    #[arg(long)]
    expected_artifact_digest: Option<String>,
    /// Install from a private-space signed manifest URL.
    #[arg(long)]
    private_manifest_url: Option<String>,
    /// Activate the tool after installing it (tools only; skills are not activatable).
    #[arg(long)]
    activate: bool,
    #[arg(long)]
    json: bool,
}

impl IronHubCommand {
    pub(crate) fn execute(self, context: RebornCliContext) -> anyhow::Result<()> {
        crate::runtime::init_tracing();
        let (command, json, label) = match self.command {
            IronHubSubcommand::Search(command) => (
                RebornIronHubCommand::Search {
                    query: command.query.unwrap_or_default(),
                },
                command.json,
                "search",
            ),
            IronHubSubcommand::List(command) => (
                RebornIronHubCommand::List {
                    kind: command.kind.map(IronHubEntryKind::from),
                },
                command.json,
                "list",
            ),
            IronHubSubcommand::Info(command) => (
                RebornIronHubCommand::Info {
                    name: command.name,
                    kind: command.kind.map(IronHubEntryKind::from),
                },
                command.json,
                "info",
            ),
            IronHubSubcommand::Install(command) => (
                RebornIronHubCommand::Install {
                    name: command.name,
                    options: IronHubInstallOptions {
                        kind: command.kind.map(IronHubEntryKind::from),
                        force: command.force,
                        acknowledge_unverified: command.acknowledge_unverified,
                        expected_version: command.expected_version,
                        expected_artifact_digest: command.expected_artifact_digest,
                        private_manifest_url: command.private_manifest_url,
                        activate: command.activate,
                    },
                },
                command.json,
                "install",
            ),
        };
        let response = execute_ironhub_command(context, command, self.confirm_host_access)?;
        if json {
            println!("{}", serde_json::to_string(&response)?);
        } else {
            print!("{}", render_reborn_ironhub_response(label, &response));
        }
        Ok(())
    }
}

fn execute_ironhub_command(
    context: RebornCliContext,
    command: RebornIronHubCommand,
    confirm_host_access: bool,
) -> anyhow::Result<IronHubResponse> {
    let runtime_services = crate::runtime::build_services_input_with_options(
        context.boot_config(),
        RuntimeInputCaller::Run,
        RuntimeInputOptions {
            confirm_host_access,
        },
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime for ironhub command")?;
    runtime.block_on(async move {
        let services_input =
            crate::runtime::with_binary_host_extension_bindings(runtime_services.services_input)?;
        let runtime = build_reborn_runtime(RebornRuntimeInput::from_build_input(services_input))
            .await
            .context("failed to assemble Reborn runtime for ironhub command")?;
        let response = crate::ironhub_host::execute_catalog_command(&runtime, command)
            .await
            .map_err(anyhow::Error::from)?;
        runtime
            .shutdown()
            .await
            .context("failed to shut down Reborn runtime after ironhub command")?;
        Ok(response)
    })
}
