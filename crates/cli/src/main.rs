//! The `claude-code-manager` CLI — session surgery over Claude Code's session
//! files, plus a `--print` listing used as a smoke test.
//!
//! The desktop app (`crates/app`) is the interactive interface; this binary
//! exists because the `session-surgery` and `session-harvest` skills drive
//! surgery non-interactively, resolving it by name on PATH.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};

use claude_code_manager_core::session::summary;
use claude_code_manager_core::store;
use claude_code_manager_core::store::paths;

#[derive(Parser, Debug)]
#[command(name = "claude-code-manager", about = "Session surgery for Claude Code sessions")]
struct Cli {
    /// Override the ~/.claude root (useful for inspecting another user's tree).
    #[arg(long)]
    root: Option<PathBuf>,

    /// Print discovered sessions to stdout and exit.
    #[arg(long)]
    print: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Rewrite session history to reduce context (summarize or prune a range).
    Surgery {
        #[command(subcommand)]
        op: SurgeryOp,
    },
}

#[derive(Subcommand, Debug)]
enum SurgeryOp {
    /// Print the messages in a range (turn-snapped) — or the whole session — for
    /// summarizing/harvesting. With --outline, print a compact one-line-per-turn map.
    Extract {
        /// Session file path or bare session id.
        #[arg(long)]
        session: String,
        /// Start message id (uuid). Omit (with --to) to read the whole session.
        #[arg(long)]
        from: Option<String>,
        /// End message id (uuid). Omit (with --from) to read the whole session.
        #[arg(long)]
        to: Option<String>,
        /// Compact map: one line per turn (ids + tool-call one-liners), no outputs.
        #[arg(long)]
        outline: bool,
        /// Cover ALL conversation trees (pre-compaction + active), not just the active path.
        #[arg(long)]
        all_segments: bool,
    },
    /// Write a new session with the range pruned or replaced by a summary.
    Rewrite {
        #[arg(long)]
        session: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        /// "prune" or "summary".
        #[arg(long)]
        mode: String,
        /// File containing the summary text (required for --mode summary).
        #[arg(long)]
        summary_file: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli
        .root
        .or_else(paths::default_claude_root)
        .context("could not locate ~/.claude (HOME unset?)")?;

    if let Some(Command::Surgery { op }) = &cli.command {
        use claude_code_manager_core::session::surgery::{self, RewriteMode};
        match op {
            SurgeryOp::Extract { session, from, to, outline, all_segments } => {
                let path = surgery::resolve_session_path(&root, session)?;
                print!(
                    "{}",
                    surgery::extract(&path, from.as_deref(), to.as_deref(), *outline, *all_segments)?
                );
            }
            SurgeryOp::Rewrite { session, from, to, mode, summary_file } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let m = match mode.as_str() {
                    "prune" => RewriteMode::Prune,
                    "summary" => RewriteMode::Summary,
                    other => anyhow::bail!("--mode must be 'prune' or 'summary', got '{}'", other),
                };
                let summary_text = match summary_file {
                    Some(f) => Some(std::fs::read_to_string(f).context("could not read --summary-file")?),
                    None => None,
                };
                let out = surgery::rewrite(&root, &path, from, to, m, summary_text.as_deref())?;
                println!("new session: {}", out.commit.new_session_id);
                println!("path: {}", out.commit.new_path);
                println!("resume with: claude --resume {}", out.commit.new_session_id);
                println!(
                    "messages: {} -> {}  (removed ~{} bytes)",
                    out.messages_before, out.messages_after, out.removed_bytes
                );
                if !out.commit.repairs.is_empty() {
                    println!("tool-pairing repairs: {}", out.commit.repairs.len());
                }
            }
        }
        return Ok(());
    }

    if cli.print {
        let disc = store::discover(&root)?;
        println!("{} sessions", disc.sessions.len());
        for s in disc.sessions.iter().take(20) {
            println!(
                "  {:24}  {:8}  {:>7}  {}",
                truncate(&s.project_label, 24),
                summary::relative_time(s.mtime),
                summary::compact_tokens(s.total_tokens),
                truncate(&s.title, 80),
            );
        }
        if !disc.warnings.is_empty() {
            eprintln!("\n{} warnings:", disc.warnings.len());
            for w in &disc.warnings {
                eprintln!("  {}", w);
            }
        }
        return Ok(());
    }

    // Bare invocation. There is no interactive mode here — the desktop app is
    // the interface — so show what this binary can do instead of doing nothing.
    Cli::command().print_help()?;
    println!();
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        let head: String = s.chars().take(max - 1).collect();
        format!("{}…", head)
    }
}
