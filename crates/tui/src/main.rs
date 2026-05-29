mod app;
mod ui;

use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli
        .root
        .or_else(paths::default_claude_root)
        .context("could not locate ~/.claude (HOME unset?)")?;

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
