use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use pulse_core::{Collector, ConnectionRow, ProcessRow, list_connections};

const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Flags the UI flips to tell the worker what to do on the next tick.
#[derive(Default)]
pub struct Controls {
    pub paused: AtomicBool,
    pub want_connections: AtomicBool,
}

pub struct Snapshot {
    pub processes: Vec<ProcessRow>,
    /// None when nothing is looking at sockets, so we skip the query.
    pub connections: Option<Vec<ConnectionRow>>,
}

pub enum Update {
    Snapshot(Snapshot),
    Failed(String),
}

pub fn spawn(ctx: egui::Context, controls: Arc<Controls>) -> Receiver<Update> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let mut collector = match Collector::new() {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(Update::Failed(format!(
                    "Couldn't read the process list: {err}"
                )));
                ctx.request_repaint();
                return;
            }
        };

        loop {
            if !controls.paused.load(Ordering::Relaxed) {
                let snapshot = Snapshot {
                    processes: collector.tick(),
                    connections: controls
                        .want_connections
                        .load(Ordering::Relaxed)
                        .then(list_connections),
                };
                if tx.send(Update::Snapshot(snapshot)).is_err() {
                    return;
                }
                // The UI only redraws when there's something new to show.
                ctx.request_repaint();
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });

    rx
}
