mod details;
mod dialogs;
mod table;

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::Frame;
use egui::{Color32, RichText};

use pulse_core::{
    ConnectionRow, Filter, Priority, ProcessRow, ProcessTree, VisibleRow, is_critical,
    kill_process, open_file_location, resume_process, set_priority, suspend_process,
};

use crate::worker::{self, Controls, Snapshot, Update};

const NEW_PROCESS_HIGHLIGHT: Duration = Duration::from_secs(2);
const STATUS_TIMEOUT: Duration = Duration::from_secs(4);

const STATUS_COLOR: Color32 = Color32::from_rgb(230, 200, 90);

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Name,
    Pid,
    Cpu,
    Memory,
    Threads,
    Handles,
    User,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    List,
    Tree,
}

#[derive(Clone, Copy)]
enum RowAction {
    Kill,
    KillTree,
    Suspend,
    Resume,
    OpenLocation,
    CopyPath,
    CopyPid,
}

/// Identifies a process across snapshots: PIDs alone get reused.
#[derive(Clone)]
struct Target {
    pid: u32,
    start_time_secs: u64,
    name: String,
}

#[derive(Clone)]
enum Confirm {
    Kill {
        target: Target,
        include_tree: bool,
        critical: bool,
    },
    Realtime(Target),
}

fn compare_rows(rows: &[ProcessRow], sort: SortColumn, desc: bool, a: usize, b: usize) -> Ordering {
    let (ra, rb) = (&rows[a], &rows[b]);
    let ord = match sort {
        SortColumn::Name => ra
            .display_name()
            .to_lowercase()
            .cmp(&rb.display_name().to_lowercase()),
        SortColumn::Pid => ra.pid.cmp(&rb.pid),
        SortColumn::Cpu => ra.cpu_percent.total_cmp(&rb.cpu_percent),
        SortColumn::Memory => ra.memory_bytes.cmp(&rb.memory_bytes),
        SortColumn::Threads => ra.threads.cmp(&rb.threads),
        SortColumn::Handles => ra.handles.cmp(&rb.handles),
        SortColumn::User => ra.user.cmp(&rb.user),
    };
    if desc { ord.reverse() } else { ord }
}

pub struct PulseApp {
    rx: Receiver<Update>,
    controls: Arc<Controls>,
    fatal_error: Option<String>,

    rows: Vec<ProcessRow>,
    tree: ProcessTree,
    connections: Option<Vec<ConnectionRow>>,
    known_pids: HashSet<u32>,
    new_pids: HashMap<u32, Instant>,
    /// Processes suspended from here, by pid -> start time.
    suspended: HashMap<u32, u64>,

    filter: Filter,
    query: String,
    want_search_focus: bool,
    search_focused: bool,
    view_mode: ViewMode,
    sort: SortColumn,
    sort_desc: bool,
    collapsed: HashSet<u32>,
    visible: Vec<VisibleRow>,
    selected_pid: Option<u32>,
    network_open: bool,

    status: Option<(String, Instant)>,
    confirm: Option<Confirm>,
}

impl PulseApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.all_styles_mut(|style| {
            let radius = egui::CornerRadius::same(2);
            let widgets = &mut style.visuals.widgets;
            for w in [
                &mut widgets.noninteractive,
                &mut widgets.inactive,
                &mut widgets.hovered,
                &mut widgets.active,
                &mut widgets.open,
            ] {
                w.corner_radius = radius;
            }
        });

        let controls = Arc::new(Controls::default());
        Self {
            rx: worker::spawn(cc.egui_ctx.clone(), controls.clone()),
            controls,
            fatal_error: None,
            rows: Vec::new(),
            tree: ProcessTree::default(),
            connections: None,
            known_pids: HashSet::new(),
            new_pids: HashMap::new(),
            suspended: HashMap::new(),
            filter: Filter::new(),
            query: String::new(),
            want_search_focus: true,
            search_focused: false,
            view_mode: ViewMode::List,
            sort: SortColumn::Cpu,
            sort_desc: true,
            collapsed: HashSet::new(),
            visible: Vec::new(),
            selected_pid: None,
            network_open: false,
            status: None,
            confirm: None,
        }
    }

    fn paused(&self) -> bool {
        self.controls.paused.load(Relaxed)
    }

    fn drain_updates(&mut self) {
        let mut latest = None;
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::Snapshot(s) => latest = Some(s),
                Update::Failed(msg) => self.fatal_error = Some(msg),
            }
        }
        if let Some(snapshot) = latest
            && !self.paused()
        {
            self.apply_snapshot(snapshot);
        }
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        let now = Instant::now();
        let current: HashSet<u32> = snapshot.processes.iter().map(|r| r.pid).collect();
        // Skip the first snapshot, otherwise everything flashes as new.
        if !self.known_pids.is_empty() {
            for pid in current.difference(&self.known_pids) {
                self.new_pids.insert(*pid, now);
            }
        }
        self.new_pids
            .retain(|_, seen| now.duration_since(*seen) < NEW_PROCESS_HIGHLIGHT);
        self.collapsed.retain(|pid| current.contains(pid));
        self.known_pids = current;

        self.rows = snapshot.processes;
        self.connections = snapshot.connections;
        self.tree = ProcessTree::build(&self.rows);
        let (rows, tree) = (&self.rows, &self.tree);
        self.suspended.retain(|&pid, &mut start| {
            tree.index_of(pid)
                .is_some_and(|i| rows[i].start_time_secs == start)
        });
        self.refresh_visible();
    }

    fn refresh_visible(&mut self) {
        let matched = self.filter.apply(&self.rows, &self.query);
        self.visible = match self.view_mode {
            ViewMode::List => {
                let mut idxs = matched;
                self.sort_indices(&mut idxs);
                idxs.into_iter()
                    .map(|idx| VisibleRow {
                        idx,
                        depth: 0,
                        has_children: false,
                    })
                    .collect()
            }
            // While searching, keep matches plus their ancestors and expand
            // every kept branch, so a hit can't hide under a collapsed parent.
            ViewMode::Tree => {
                let searching = !self.query.trim().is_empty();
                let matched: HashSet<usize> = matched.into_iter().collect();
                let keep = if searching {
                    self.tree.with_ancestors(&self.rows, matched)
                } else {
                    matched
                };
                self.tree.flatten(
                    &self.rows,
                    &keep,
                    |pid| searching || !self.collapsed.contains(&pid),
                    |idxs| self.sort_indices(idxs),
                )
            }
        };
    }

    fn sort_indices(&self, idxs: &mut [usize]) {
        let (rows, sort, desc) = (&self.rows, self.sort, self.sort_desc);
        idxs.sort_by(|&a, &b| compare_rows(rows, sort, desc, a, b));
    }

    fn set_sort(&mut self, col: SortColumn) {
        if self.sort == col {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort = col;
            self.sort_desc = !matches!(col, SortColumn::Name | SortColumn::User);
        }
        self.refresh_visible();
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    fn row(&self, pid: u32) -> Option<&ProcessRow> {
        self.tree.index_of(pid).map(|i| &self.rows[i])
    }

    fn target(&self, pid: u32) -> Option<Target> {
        self.row(pid).map(|r| Target {
            pid,
            start_time_secs: r.start_time_secs,
            name: r.display_name().to_string(),
        })
    }

    /// The row for `target` if it's still the same process.
    fn resolve(&mut self, target: &Target) -> Option<usize> {
        match self.tree.index_of(target.pid) {
            Some(i) if self.rows[i].start_time_secs == target.start_time_secs => Some(i),
            Some(_) => {
                self.set_status(format!(
                    "PID {} now belongs to another process, skipped",
                    target.pid
                ));
                None
            }
            None => {
                self.set_status(format!("{} already exited", target.name));
                None
            }
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let current = self.selected_pid.and_then(|pid| {
            self.visible
                .iter()
                .position(|v| self.rows[v.idx].pid == pid)
        });
        let next = match current {
            Some(pos) => pos.saturating_add_signed(delta).min(self.visible.len() - 1),
            None => 0,
        };
        self.selected_pid = Some(self.rows[self.visible[next].idx].pid);
    }

    fn request_kill(&mut self, pid: u32, include_tree: bool) {
        let (Some(target), Some(root)) = (self.target(pid), self.tree.index_of(pid)) else {
            return;
        };
        let critical = if include_tree {
            self.tree
                .descendants(&self.rows, root)
                .iter()
                .any(|&i| is_critical(self.rows[i].pid, &self.rows[i].name))
        } else {
            is_critical(pid, &target.name)
        };
        self.confirm = Some(Confirm::Kill {
            target,
            include_tree,
            critical,
        });
    }

    fn execute_kill(&mut self, target: &Target, include_tree: bool) {
        let Some(root) = self.resolve(target) else {
            return;
        };
        let targets: Vec<u32> = if include_tree {
            self.tree
                .descendants(&self.rows, root)
                .iter()
                .map(|&i| self.rows[i].pid)
                .collect()
        } else {
            vec![target.pid]
        };

        let mut ended = 0;
        let mut last_err = None;
        for &pid in &targets {
            match kill_process(pid) {
                Ok(()) => ended += 1,
                Err(e) => last_err = Some(e),
            }
        }
        match last_err {
            None if ended == 1 => self.set_status(format!("Ended {}", target.name)),
            None => self.set_status(format!("Ended {ended} processes")),
            Some(e) => self.set_status(format!("Ended {ended} of {}: {e}", targets.len())),
        }
        if self.selected_pid == Some(target.pid) {
            self.selected_pid = None;
        }
    }

    fn request_priority(&mut self, pid: u32, priority: Priority) {
        let Some(target) = self.target(pid) else {
            return;
        };
        if priority == Priority::Realtime {
            self.confirm = Some(Confirm::Realtime(target));
        } else {
            self.apply_priority(&target, priority);
        }
    }

    fn apply_priority(&mut self, target: &Target, priority: Priority) {
        if self.resolve(target).is_none() {
            return;
        }
        match set_priority(target.pid, priority) {
            Ok(()) => self.set_status(format!("{} set to {}", target.name, priority.label())),
            Err(e) => self.set_status(format!("Set priority failed: {e}")),
        }
    }

    fn suspend(&mut self, pid: u32) {
        let Some(target) = self.target(pid) else {
            return;
        };
        // Suspends stack, so a second one would need two resumes to undo.
        if self.suspended.contains_key(&pid) {
            self.set_status(format!("{} is already suspended", target.name));
            return;
        }
        match suspend_process(pid) {
            Ok(()) => {
                self.suspended.insert(pid, target.start_time_secs);
                self.set_status(format!("Suspended {}", target.name));
            }
            Err(e) => self.set_status(format!("Suspend failed: {e}")),
        }
    }

    fn resume(&mut self, pid: u32) {
        let Some(target) = self.target(pid) else {
            return;
        };
        match resume_process(pid) {
            Ok(()) => {
                self.suspended.remove(&pid);
                self.set_status(format!("Resumed {}", target.name));
            }
            Err(e) => self.set_status(format!("Resume failed: {e}")),
        }
    }

    fn apply_row_action(&mut self, ctx: &egui::Context, pid: u32, action: RowAction) {
        let Some(path) = self.row(pid).map(|r| r.exe_path.clone()) else {
            return;
        };
        match action {
            RowAction::Kill => self.request_kill(pid, false),
            RowAction::KillTree => self.request_kill(pid, true),
            RowAction::Suspend => self.suspend(pid),
            RowAction::Resume => self.resume(pid),
            RowAction::OpenLocation => {
                if let Some(path) = path
                    && let Err(e) = open_file_location(&path)
                {
                    self.set_status(format!("Couldn't open location: {e}"));
                }
            }
            RowAction::CopyPath => {
                if let Some(path) = path {
                    ctx.copy_text(path);
                }
            }
            RowAction::CopyPid => ctx.copy_text(pid.to_string()),
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let (escape, up, down, delete, enter) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Enter),
            )
        });

        if self.confirm.is_some() {
            if escape {
                self.confirm = None;
            }
            return;
        }

        if escape && !self.query.is_empty() {
            self.query.clear();
            self.refresh_visible();
        }
        if up {
            self.move_selection(-1);
        }
        if down {
            self.move_selection(1);
        }
        if enter
            && self.search_focused
            && let Some(first) = self.visible.first()
        {
            self.selected_pid = Some(self.rows[first.idx].pid);
        }
        if delete
            && !self.search_focused
            && let Some(pid) = self.selected_pid
        {
            self.request_kill(pid, false);
        }
    }

    fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Pulse").strong().size(16.0));
            ui.separator();

            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Filter by name, PID or path")
                    .desired_width(320.0),
            );
            if self.want_search_focus {
                response.request_focus();
                self.want_search_focus = false;
            }
            self.search_focused = response.has_focus();
            if response.changed() {
                self.refresh_visible();
            }

            if !self.query.is_empty() && ui.button("Clear").clicked() {
                self.query.clear();
                self.refresh_visible();
                self.want_search_focus = true;
            }

            ui.separator();
            ui.label(format!(
                "{} / {} processes",
                self.visible.len(),
                self.rows.len()
            ));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let paused = self.paused();
                if ui.button(if paused { "Resume" } else { "Pause" }).clicked() {
                    self.controls.paused.store(!paused, Relaxed);
                }

                let mut tree = self.view_mode == ViewMode::Tree;
                if ui.checkbox(&mut tree, "Tree view").changed() {
                    self.view_mode = if tree { ViewMode::Tree } else { ViewMode::List };
                    self.refresh_visible();
                }

                match &self.status {
                    Some((msg, at)) if at.elapsed() < STATUS_TIMEOUT => {
                        ui.label(RichText::new(msg).color(STATUS_COLOR));
                        // Nothing else may repaint while paused, so schedule the fade-out.
                        ui.ctx()
                            .request_repaint_after(STATUS_TIMEOUT.saturating_sub(at.elapsed()));
                    }
                    Some(_) => self.status = None,
                    None => {}
                }
            });
        });
    }
}

impl eframe::App for PulseApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let ctx = ui.ctx().clone();
        self.drain_updates();
        self.network_open = false;

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar_ui(ui));
        self.handle_shortcuts(&ctx);

        if self.selected_pid.is_some() {
            egui::Panel::right("details")
                .resizable(true)
                .default_size(320.0)
                .min_size(260.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.details_ui(ui));
                });
        }

        egui::CentralPanel::default().show(ui, |ui| self.table_ui(ui));
        self.show_confirm(&ctx);

        // Socket tables are only read while someone is looking at them.
        self.controls
            .want_connections
            .store(self.network_open, Relaxed);
    }
}
