use std::io::Stdout;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event as CtEvent, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use claude_code_manager_core::session::parse::parse_tail;
use claude_code_manager_core::store;
use claude_code_manager_core::store::watcher::{watch_file, FileWatcher};
use crate::ui::list_view::{ListAction, ListView};
use crate::ui::transcript_view::{TranscriptAction, TranscriptView};

pub struct App {
    root: PathBuf,
    page: Page,
    list: ListView,
    transcript: Option<TranscriptView>,
    watcher: Option<FileWatcher>,
    watch_rx: Option<Receiver<()>>,
}

enum Page {
    List,
    Transcript,
}

impl App {
    pub fn new(root: PathBuf) -> Result<Self> {
        let disc = store::discover(&root)?;
        Ok(Self {
            root,
            page: Page::List,
            list: ListView::new(disc.sessions),
            transcript: None,
            watcher: None,
            watch_rx: None,
        })
    }

    pub fn run(mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        let poll_interval = Duration::from_millis(100);
        let watch_debounce = Duration::from_millis(80);
        let mut last_tail = Instant::now();

        loop {
            terminal.draw(|f| {
                let area = f.area();
                match self.page {
                    Page::List => self.list.render(f, area),
                    Page::Transcript => {
                        if let Some(t) = self.transcript.as_mut() {
                            t.render(f, area);
                        }
                    }
                }
            })?;

            // Handle file-watcher pings (debounced).
            if let Some(rx) = self.watch_rx.as_ref() {
                let mut any = false;
                while let Ok(()) = rx.try_recv() {
                    any = true;
                }
                if any && last_tail.elapsed() >= watch_debounce {
                    self.tail_open_transcript();
                    last_tail = Instant::now();
                }
            }

            if event::poll(poll_interval)? {
                match event::read()? {
                    CtEvent::Key(key) => {
                        if key.kind != KeyEventKind::Press {
                            continue;
                        }
                        match self.page {
                            Page::List => match self.list.handle_key(key) {
                                ListAction::None => {}
                                ListAction::Quit => return Ok(()),
                                ListAction::Refresh => self.refresh_list()?,
                                ListAction::Open(i) => self.open_session(i)?,
                            },
                            Page::Transcript => {
                                if let Some(t) = self.transcript.as_mut() {
                                    match t.handle_key(key) {
                                        TranscriptAction::None => {}
                                        TranscriptAction::Quit => return Ok(()),
                                        TranscriptAction::Back => self.back_to_list(),
                                    }
                                }
                            }
                        }
                    }
                    CtEvent::Resize(_, _) => {
                        if let Some(t) = self.transcript.as_mut() {
                            t.invalidate();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn refresh_list(&mut self) -> Result<()> {
        let disc = store::discover(&self.root)?;
        self.list.replace(disc.sessions);
        Ok(())
    }

    fn open_session(&mut self, visible_idx: usize) -> Result<()> {
        // visible_idx is into ListView::all (we receive the index from ListAction::Open).
        // We refetch via focused_summary using the focused cursor instead.
        let summary = self
            .list
            .focused_summary()
            .cloned()
            .or({
                // fallback: by index in `all`
                None
            });
        let _ = visible_idx;
        let summary = match summary {
            Some(s) => s,
            None => return Ok(()),
        };
        let session = store::open_session(summary.file_path.clone(), summary.project_label.clone())?;
        let view = TranscriptView::new(session);
        self.transcript = Some(view);
        self.page = Page::Transcript;
        self.spawn_watcher(&summary.file_path)?;
        Ok(())
    }

    fn back_to_list(&mut self) {
        self.watcher = None;
        self.watch_rx = None;
        self.transcript = None;
        self.page = Page::List;
    }

    fn spawn_watcher(&mut self, path: &Path) -> Result<()> {
        let (tx, rx): (Sender<()>, Receiver<()>) = channel();
        match watch_file(path, tx) {
            Ok(w) => {
                self.watcher = Some(w);
                self.watch_rx = Some(rx);
            }
            Err(_) => {
                // Watcher failure isn't fatal; live tail just won't work.
                self.watcher = None;
                self.watch_rx = None;
            }
        }
        Ok(())
    }

    fn tail_open_transcript(&mut self) {
        let Some(t) = self.transcript.as_mut() else {
            return;
        };
        let path = t.session.file_path.clone();
        let from = t.session.bytes_read;
        if let Ok(outcome) = parse_tail(&path, from) {
            if !outcome.events.is_empty() {
                t.session.events.extend(outcome.events);
                t.session.bytes_read = outcome.bytes_read;
                t.session.recompute_leaf();
                t.invalidate();
            } else {
                t.session.bytes_read = outcome.bytes_read;
            }
        }
    }
}
