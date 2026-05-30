mod app;
mod ui;

use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use uuid::Uuid;

use claude_manager_core::session::summary;
use claude_manager_core::store;
use claude_manager_core::store::paths;

#[derive(Parser, Debug)]
#[command(name = "claude-manager", about = "TUI for Claude Code sessions")]
struct Cli {
    /// Override the ~/.claude root (useful for inspecting another user's tree).
    #[arg(long)]
    root: Option<PathBuf>,

    /// Print discovered sessions to stdout and exit, instead of launching the TUI.
    #[arg(long)]
    print: bool,

    /// Dev: fork an existing session file to a new flattened session, print the new id, then exit.
    #[arg(long)]
    fork_test: Option<std::path::PathBuf>,

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
    /// Print the messages in the range [from..to] (turn-snapped) for summarizing.
    Extract {
        /// Session file path or bare session id.
        #[arg(long)]
        session: String,
        /// Start message id (uuid).
        #[arg(long)]
        from: String,
        /// End message id (uuid).
        #[arg(long)]
        to: String,
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
        use claude_manager_core::session::surgery::{self, RewriteMode};
        match op {
            SurgeryOp::Extract { session, from, to } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let a = Uuid::parse_str(from).context("invalid --from uuid")?;
                let b = Uuid::parse_str(to).context("invalid --to uuid")?;
                print!("{}", surgery::extract(&path, a, b)?);
            }
            SurgeryOp::Rewrite { session, from, to, mode, summary_file } => {
                let path = surgery::resolve_session_path(&root, session)?;
                let a = Uuid::parse_str(from).context("invalid --from uuid")?;
                let b = Uuid::parse_str(to).context("invalid --to uuid")?;
                let m = match mode.as_str() {
                    "prune" => RewriteMode::Prune,
                    "summary" => RewriteMode::Summary,
                    other => anyhow::bail!("--mode must be 'prune' or 'summary', got '{}'", other),
                };
                let summary_text = match summary_file {
                    Some(f) => Some(std::fs::read_to_string(f).context("could not read --summary-file")?),
                    None => None,
                };
                let out = surgery::rewrite(&root, &path, a, b, m, summary_text.as_deref())?;
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

    if let Some(src) = cli.fork_test.clone() {
        use claude_manager_core::session::edit::{commit, CommitOpts};
        use claude_manager_core::session::raw::{active_path, read_raw};
        let lines = read_raw(&src)?;
        let path = active_path(&lines);
        let cwd = lines
            .iter()
            .find_map(|l| l.value.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| ".".to_string());
        let git_branch = lines
            .iter()
            .find_map(|l| l.value.get("gitBranch").and_then(|v| v.as_str()).map(|s| s.to_string()));
        let opts = CommitOpts { target_cwd: cwd, git_branch, custom_title: Some("fork-test".into()) };
        let res = commit(&root, &path, &opts)?;
        println!("new session: {}", res.new_session_id);
        println!("path: {}", res.new_path);
        println!("repairs: {}", res.repairs.len());
        println!("resume with: claude --resume {}", res.new_session_id);
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

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let guard = TerminalGuard;

    let res = app::App::new(root).and_then(|a| a.run(&mut terminal));

    drop(guard);
    res
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
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
