//! # build-gate
//!
//! CLI for coordinated cargo builds in multi-agent environments.
//!
//! ## Usage
//!
//! ```bash
//! # Instead of: cargo check
//! build-gate check
//!
//! # With workspace flag
//! build-gate check --workspace
//!
//! # Force build even if hash unchanged
//! build-gate build --force

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
//!
//! # Check lock status
//! build-gate status
//! ```

use clap::{Parser, Subcommand};
use nexcore_build_gate::{
    BuildResult, LockStatus, find_workspace_root, hash_source_dir, lock_status, run_cargo,
    should_build,
};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "build-gate")]
#[command(about = "Coordinated cargo builds for multi-agent environments")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Project directory (default: auto-detect workspace root)
    #[arg(short = 'd', long = "dir", global = true)]
    project_dir: Option<PathBuf>,

    /// Force operation even if hash unchanged
    #[arg(short, long, global = true)]
    force: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run cargo check with coordination
    Check {
        /// Pass --workspace to cargo
        #[arg(long)]
        workspace: bool,

        /// Additional args to pass to cargo
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Run cargo build with coordination
    Build {
        /// Pass --workspace to cargo
        #[arg(long)]
        workspace: bool,

        /// Build in release mode
        #[arg(long)]
        release: bool,

        /// Additional args to pass to cargo
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Run cargo clippy with coordination
    Clippy {
        /// Pass --workspace to cargo
        #[arg(long)]
        workspace: bool,

        /// Additional args to pass to cargo
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Run cargo test with coordination
    Test {
        /// Pass --workspace to cargo
        #[arg(long)]
        workspace: bool,

        /// Additional args to pass to cargo
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Check lock and hash status
    Status,

    /// Compute current source hash (no lock)
    Hash,

    /// Wait for lock to be available
    Wait {
        /// Timeout in seconds
        #[arg(short, long, default_value = "300")]
        timeout: u64,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Resolve project directory
    let project = match cli.project_dir {
        Some(dir) => dir,
        None => {
            let cwd = std::env::current_dir()?;
            find_workspace_root(&cwd).unwrap_or(cwd)
        }
    };

    tracing::debug!("Project: {}", project.display());

    match cli.command {
        Commands::Check {
            workspace: ws,
            args,
        } => {
            let mut cargo_args = vec!["check"];
            if ws {
                cargo_args.push("--workspace");
            }
            for arg in &args {
                cargo_args.push(arg);
            }
            run_cargo(&project, &cargo_args, cli.force)?;
        }

        Commands::Build {
            workspace: ws,
            release,
            args,
        } => {
            let mut cargo_args = vec!["build"];
            if ws {
                cargo_args.push("--workspace");
            }
            if release {
                cargo_args.push("--release");
            }
            for arg in &args {
                cargo_args.push(arg);
            }
            run_cargo(&project, &cargo_args, cli.force)?;
        }

        Commands::Clippy {
            workspace: ws,
            args,
        } => {
            let mut cargo_args = vec!["clippy"];
            if ws {
                cargo_args.push("--workspace");
            }
            for arg in &args {
                cargo_args.push(arg);
            }
            run_cargo(&project, &cargo_args, cli.force)?;
        }

        Commands::Test {
            workspace: ws,
            args,
        } => {
            let mut cargo_args = vec!["test"];
            if ws {
                cargo_args.push("--workspace");
            }
            for arg in &args {
                cargo_args.push(arg);
            }
            run_cargo(&project, &cargo_args, cli.force)?;
        }

        Commands::Status => {
            println!("=== Build Gate Status ===\n");

            // Lock status
            match lock_status() {
                LockStatus::Available => println!("Lock:   🟢 Available"),
                LockStatus::Held => println!("Lock:   🔴 Held by another process"),
            }

            // Hash status
            match hash_source_dir(&project) {
                Ok(hash) => println!("Hash:   {}...", &hash[..32]),
                Err(e) => println!("Hash:   Error: {}", e),
            }

            // Should build?
            match should_build(&project) {
                Ok(true) => println!("Build:  ⚠️  Required (hash changed)"),
                Ok(false) => println!("Build:  ✅ Not required (hash unchanged)"),
                Err(e) => println!("Build:  Error: {}", e),
            }

            // Cached result
            if let Some(result) = BuildResult::load() {
                println!("\n=== Last Build ===\n");
                println!("Command:   {}", result.command);
                println!("Time:      {}", result.timestamp);
                println!("Duration:  {}ms", result.duration_ms);
                println!(
                    "Status:    {}",
                    if result.success {
                        "✅ Success"
                    } else {
                        "❌ Failed"
                    }
                );
                println!("Hash:      {}...", &result.hash[..32]);
            }
        }

        Commands::Hash => {
            let hash = hash_source_dir(&project)?;
            println!("{}", hash);
        }

        Commands::Wait { timeout } => {
            let timeout = std::time::Duration::from_secs(timeout);
            tracing::info!("Waiting for lock (timeout: {:?})...", timeout);
            let lock = nexcore_build_gate::BuildLock::try_acquire(timeout)?;
            tracing::info!("Lock acquired after {:?}", lock.elapsed());
            // Lock is released on drop
        }
    }

    Ok(())
}
