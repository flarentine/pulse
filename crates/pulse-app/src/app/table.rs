use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder, TableRow};

use super::{PulseApp, RowAction, SortColumn, ViewMode};
use crate::fmt;

const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 16.0;
const HOT_CPU_PERCENT: f32 = 50.0;

const NEW_COLOR: Color32 = Color32::from_rgb(110, 210, 140);
const HOT_COLOR: Color32 = Color32::from_rgb(230, 120, 60);

impl PulseApp {
    fn header_cell(&mut self, header: &mut TableRow<'_, '_>, label: &str, col: SortColumn) {
        let text = match (self.sort == col, self.sort_desc) {
            (true, true) => format!("{label} v"),
            (true, false) => format!("{label} ^"),
            _ => label.to_string(),
        };
        let mut clicked = false;
        header.col(|ui| clicked = ui.button(text).clicked());
        if clicked {
            self.set_sort(col);
        }
    }

    pub(super) fn table_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(err) = &self.fatal_error {
            ui.centered_and_justified(|ui| ui.label(err));
            return;
        }
        if self.rows.is_empty() {
            ui.centered_and_justified(|ui| ui.label("Loading..."));
            return;
        }

        let ctx = ui.ctx().clone();
        let mut selection = None;
        let mut action = None;
        let mut toggle_collapse = None;

        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(230.0).at_least(140.0).clip(true))
            .column(Column::initial(65.0).at_least(55.0))
            .column(Column::initial(65.0).at_least(55.0))
            .column(Column::initial(85.0).at_least(70.0))
            .column(Column::initial(70.0).at_least(60.0))
            .column(Column::initial(75.0).at_least(60.0))
            .column(Column::initial(120.0).at_least(80.0).clip(true))
            .column(Column::remainder().clip(true))
            .header(24.0, |mut header| {
                self.header_cell(&mut header, "Name", SortColumn::Name);
                self.header_cell(&mut header, "PID", SortColumn::Pid);
                self.header_cell(&mut header, "CPU %", SortColumn::Cpu);
                self.header_cell(&mut header, "Memory", SortColumn::Memory);
                self.header_cell(&mut header, "Threads", SortColumn::Threads);
                self.header_cell(&mut header, "Handles", SortColumn::Handles);
                self.header_cell(&mut header, "User", SortColumn::User);
                header.col(|ui| {
                    ui.strong("Path");
                });
            })
            .body(|body| {
                let is_tree = self.view_mode == ViewMode::Tree;
                body.rows(ROW_HEIGHT, self.visible.len(), |mut table_row| {
                    let vis = self.visible[table_row.index()];
                    let row = &self.rows[vis.idx];
                    table_row.set_selected(self.selected_pid == Some(row.pid));

                    table_row.col(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if is_tree {
                            ui.add_space(vis.depth as f32 * INDENT);
                            if vis.has_children {
                                let glyph = if self.collapsed.contains(&row.pid) {
                                    "+"
                                } else {
                                    "-"
                                };
                                if ui.small_button(glyph).clicked() {
                                    toggle_collapse = Some(row.pid);
                                }
                            } else {
                                ui.add_space(INDENT);
                            }
                        }
                        let suspended = self.suspended.contains_key(&row.pid);
                        let mut name = RichText::new(row.display_name());
                        if suspended {
                            name = name.weak();
                        } else if self.new_pids.contains_key(&row.pid) {
                            name = name.color(NEW_COLOR);
                        }
                        ui.label(name);
                        if suspended {
                            ui.label(RichText::new(" suspended").weak().small());
                        }
                    });
                    table_row.col(|ui| {
                        ui.label(row.pid.to_string());
                    });
                    table_row.col(|ui| {
                        let mut cpu = RichText::new(format!("{:.1}", row.cpu_percent));
                        if row.cpu_percent > HOT_CPU_PERCENT {
                            cpu = cpu.color(HOT_COLOR);
                        }
                        ui.label(cpu);
                    });
                    table_row.col(|ui| {
                        ui.label(fmt::bytes(row.memory_bytes));
                    });
                    table_row.col(|ui| {
                        ui.label(row.threads.to_string());
                    });
                    table_row.col(|ui| {
                        ui.label(row.handles.map(|h| h.to_string()).unwrap_or_default());
                    });
                    table_row.col(|ui| {
                        ui.label(row.user.as_deref().unwrap_or_default());
                    });
                    table_row.col(|ui| {
                        ui.label(row.exe_path.as_deref().unwrap_or_default());
                    });

                    let response = table_row.response();
                    if response.clicked() {
                        selection = Some(row.pid);
                    }
                    response.context_menu(|ui| {
                        selection = Some(row.pid);
                        let mut item = |ui: &mut egui::Ui, label: &str, act: RowAction| {
                            if ui.button(label).clicked() {
                                action = Some((row.pid, act));
                                ui.close();
                            }
                        };
                        item(ui, "End process", RowAction::Kill);
                        item(ui, "End process tree", RowAction::KillTree);
                        ui.separator();
                        item(ui, "Suspend", RowAction::Suspend);
                        item(ui, "Resume", RowAction::Resume);
                        ui.separator();
                        if row.exe_path.is_some() {
                            item(ui, "Open file location", RowAction::OpenLocation);
                            item(ui, "Copy path", RowAction::CopyPath);
                        }
                        item(ui, "Copy PID", RowAction::CopyPid);
                    });
                });
            });

        if let Some(pid) = selection {
            self.selected_pid = Some(pid);
        }
        if let Some((pid, act)) = action {
            self.apply_row_action(&ctx, pid, act);
        }
        if let Some(pid) = toggle_collapse {
            if !self.collapsed.remove(&pid) {
                self.collapsed.insert(pid);
            }
            self.refresh_visible();
        }
    }
}
