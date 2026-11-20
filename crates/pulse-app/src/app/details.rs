use egui::RichText;

use pulse_core::{Priority, is_real_parent};

use super::{PulseApp, RowAction};
use crate::fmt;

impl PulseApp {
    pub(super) fn details_ui(&mut self, ui: &mut egui::Ui) {
        let Some(pid) = self.selected_pid else {
            return;
        };
        let Some(row) = self.row(pid).cloned() else {
            ui.label("Process has exited.");
            return;
        };
        let parent = row.parent_pid.map(|ppid| match self.row(ppid) {
            Some(p) if is_real_parent(p, &row) => format!("{} ({ppid})", p.display_name()),
            _ => format!("{ppid} (exited)"),
        });
        let suspended = self.suspended.contains_key(&pid);

        ui.heading(row.display_name());
        ui.label(format!("PID {}", row.pid));
        ui.add_space(6.0);

        egui::Grid::new("details_grid")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                let mut field = |label: &str, value: String| {
                    ui.label(label);
                    ui.label(value);
                    ui.end_row();
                };
                if suspended {
                    field("State", "Suspended".into());
                }
                field(
                    "User",
                    row.user.clone().unwrap_or_else(|| "unavailable".into()),
                );
                field("CPU", format!("{:.1}%", row.cpu_percent));
                field("Memory", fmt::bytes(row.memory_bytes));
                field("Private bytes", fmt::bytes(row.private_bytes));
                field("Threads", row.threads.to_string());
                field(
                    "Handles",
                    row.handles
                        .map_or_else(|| "unavailable".into(), |h| h.to_string()),
                );
                field("Started", fmt::started_at(row.start_time_secs));
                if row.start_time_secs != 0 {
                    field("Run time", fmt::duration_secs(row.run_time_secs));
                }
                if let Some(parent) = parent {
                    field("Parent", parent);
                }
            });

        ui.add_space(8.0);
        ui.label(RichText::new("Path").strong());
        ui.add(egui::Label::new(row.exe_path.as_deref().unwrap_or("unavailable")).wrap());
        ui.add_space(8.0);
        ui.label(RichText::new("Command line").strong());
        let cmd = if row.cmd.is_empty() {
            "unavailable"
        } else {
            &row.cmd
        };
        ui.add(egui::Label::new(cmd).wrap());

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(6.0);

        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            let mut button = |ui: &mut egui::Ui, label: &str, act: RowAction| {
                if ui.button(label).clicked() {
                    action = Some(act);
                }
            };
            button(ui, "End process", RowAction::Kill);
            button(ui, "End process tree", RowAction::KillTree);
            if suspended {
                button(ui, "Resume", RowAction::Resume);
            } else {
                button(ui, "Suspend", RowAction::Suspend);
                button(ui, "Resume", RowAction::Resume);
            }
            if row.exe_path.is_some() {
                button(ui, "Open file location", RowAction::OpenLocation);
                button(ui, "Copy path", RowAction::CopyPath);
            }
            button(ui, "Copy PID", RowAction::CopyPid);
        });
        if let Some(act) = action {
            let ctx = ui.ctx().clone();
            self.apply_row_action(&ctx, pid, act);
        }

        ui.add_space(6.0);
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label("Priority");
            egui::ComboBox::from_id_salt("priority")
                .selected_text(row.priority.map_or("Unknown", Priority::label))
                .show_ui(ui, |ui| {
                    for p in Priority::ALL {
                        if ui
                            .selectable_label(row.priority == Some(p), p.label())
                            .clicked()
                        {
                            chosen = Some(p);
                        }
                    }
                });
        });
        if let Some(p) = chosen
            && row.priority != Some(p)
        {
            self.request_priority(pid, p);
        }

        ui.add_space(12.0);
        ui.separator();
        ui.add_space(6.0);

        // Closed by default so addresses don't end up on a stream or screen share.
        let conns: Option<Vec<_>> = self
            .connections
            .as_ref()
            .map(|all| all.iter().filter(|c| c.pid == pid).collect());
        let title = match &conns {
            Some(c) => format!("Network ({})", c.len()),
            None => "Network".to_string(),
        };
        let header = egui::CollapsingHeader::new(title)
            .id_salt("network")
            .default_open(false)
            .show(ui, |ui| match conns {
                None => {
                    ui.label(RichText::new("Loading...").weak());
                }
                Some(c) if c.is_empty() => {
                    ui.label(RichText::new("No open sockets").weak());
                }
                Some(c) => {
                    for conn in c {
                        ui.label(fmt::connection(conn));
                    }
                }
            });
        self.network_open = header.openness > 0.0;
    }
}
