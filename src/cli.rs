use crate::{claude::install_claude, manifest::load_manifest, sync::sync};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "skillctl",
    about = "Synchronize manifest-managed agent skills",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Sync {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_claude: bool,
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        claude_target: Option<PathBuf>,
    },
    InstallClaude {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        claude_target: Option<PathBuf>,
    },
}

pub fn run(args: Vec<OsString>, out: &mut dyn Write) -> Result<()> {
    let cli = Cli::try_parse_from(args)?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")?;
    let (root, target, dry_run, run_sync, no_claude) = match cli.command {
        Commands::Sync {
            dry_run,
            no_claude,
            root,
            claude_target,
        } => (root, claude_target, dry_run, true, no_claude),
        Commands::InstallClaude {
            dry_run,
            root,
            claude_target,
        } => (root, claude_target, dry_run, false, false),
    };
    let root = discover_root(root.as_deref(), &home)?;
    let manifest = load_manifest(&root)?;
    if run_sync {
        sync(&root, &manifest, dry_run, out)?;
    }

    if !no_claude {
        install_claude(
            &root,
            &manifest,
            &target.unwrap_or_else(|| home.join(".claude/skills")),
            dry_run,
            out,
        )?;
    }

    Ok(())
}

fn discover_root(explicit: Option<&Path>, home: &Path) -> Result<PathBuf> {
    if let Some(root) = explicit {
        return Ok(std::fs::canonicalize(root)?);
    }

    let current = std::env::current_dir()?;
    for directory in current.ancestors() {
        if directory.join("deps.yaml").is_file() {
            return Ok(std::fs::canonicalize(directory)?);
        }
    }

    let fallback = home.join(".agents/skills");
    if fallback.join("deps.yaml").is_file() {
        return Ok(std::fs::canonicalize(fallback)?);
    }

    bail!("cannot find skills checkout; pass --root or install ~/.agents/skills")
}
