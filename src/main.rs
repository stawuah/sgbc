//! sgbc — the Solana Ghana Builder Cloud deployment tool.
//!
//! Phase 1 scope: read a project's configuration, validate it, and generate
//! every file a deploy would install. It does not execute the deploy. The
//! commands it would run are printed by `sgbc plan` so they can be reviewed
//! before the platform is trusted with a host.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use sgbc::config::Project;
use sgbc::layout::{DEFAULT_ROOT, Layout};
use sgbc::{plan, render};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "sgbc",
    about = "Solana Ghana Builder Cloud — deploy builder projects without Docker",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a project configuration and report what it describes.
    Validate {
        /// Path to the project's YAML configuration.
        config: PathBuf,
    },
    /// Print the systemd units and Caddy configuration this project generates.
    Render {
        config: PathBuf,
        /// Host root the generated paths are based on.
        #[arg(long, default_value = DEFAULT_ROOT)]
        root: PathBuf,
    },
    /// Print the ordered steps a deploy would perform.
    Plan {
        config: PathBuf,
        #[arg(long, default_value = DEFAULT_ROOT)]
        root: PathBuf,
    },
    /// Write the generated files into a root directory.
    ///
    /// This creates the directory tree, the systemd units and the Caddy
    /// fragment. It does not install them into /etc, create accounts, fetch
    /// repositories or start services — `sgbc plan` lists those steps.
    Write {
        config: PathBuf,
        /// Root to write into. Point this at a scratch directory to inspect the
        /// full tree without touching a real host.
        #[arg(long, default_value = DEFAULT_ROOT)]
        root: PathBuf,
    },
}

fn load(path: &Path) -> Result<Project> {
    let yaml =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    Project::parse(&yaml).with_context(|| format!("{} is not a valid project", path.display()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Validate { config } => {
            let project = load(&config)?;
            println!("{} is valid.\n", project.name);
            for (role, service) in project.services() {
                let port = service
                    .port
                    .map(|p| format!("port {p}"))
                    .unwrap_or_else(|| "no port".to_string());
                let domain = project.domain_for(role).unwrap_or("not routed");
                println!("  {role:<9} {} ({port}, {domain})", service.repo);
            }
        }

        Command::Render { config, root } => {
            let project = load(&config)?;
            let layout = Layout::new(&root, &project.name);

            for (role, service) in project.services() {
                println!("# ---- {} ----", layout.unit_path(role).display());
                print!("{}", render::systemd_unit(&project, service, role, &layout));
                println!();
            }
            println!("# ---- {} ----", layout.caddy_fragment().display());
            print!("{}", render::caddy_fragment(&project, &layout));
        }

        Command::Plan { config, root } => {
            let project = load(&config)?;
            let layout = Layout::new(&root, &project.name);
            let steps = plan::build(&project, &layout);

            println!("Deploy plan for {} ({} steps)\n", project.name, steps.len());
            for (index, step) in steps.iter().enumerate() {
                println!("{:>3}. {}", index + 1, step.summary);
                if let Some(command) = &step.command {
                    println!("     $ {command}");
                }
            }
            println!("\nPhase 1 does not run these steps.");
        }

        Command::Write { config, root } => {
            let project = load(&config)?;
            let layout = Layout::new(&root, &project.name);

            for dir in layout.directories() {
                fs::create_dir_all(&dir)
                    .with_context(|| format!("could not create {}", dir.display()))?;
            }
            for (role, service) in project.services() {
                fs::create_dir_all(layout.service_dir(role))?;
                let path = layout.unit_path(role);
                fs::write(
                    &path,
                    render::systemd_unit(&project, service, role, &layout),
                )
                .with_context(|| format!("could not write {}", path.display()))?;
                println!("wrote {}", path.display());
            }
            let fragment = layout.caddy_fragment();
            fs::write(&fragment, render::caddy_fragment(&project, &layout))
                .with_context(|| format!("could not write {}", fragment.display()))?;
            println!("wrote {}", fragment.display());
            println!(
                "\nNothing was installed or started. Run `sgbc plan` for the remaining steps."
            );
        }
    }

    Ok(())
}
