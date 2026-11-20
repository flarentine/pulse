use egui::{Color32, RichText};

use pulse_core::Priority;

use super::{Confirm, PulseApp};

const DANGER_COLOR: Color32 = Color32::from_rgb(196, 60, 60);
const WARNING_COLOR: Color32 = Color32::from_rgb(240, 110, 90);

impl PulseApp {
    pub(super) fn show_confirm(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm.clone() else {
            return;
        };

        let (title, target, action_label) = match &confirm {
            Confirm::Kill {
                target,
                include_tree,
                ..
            } => (
                if *include_tree {
                    "End process tree?"
                } else {
                    "End process?"
                },
                target,
                "End process",
            ),
            Confirm::Realtime(target) => ("Set realtime priority?", target, "Set realtime"),
        };

        let mut decision = None;
        let modal = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading(title);
            ui.label(format!("{} (PID {})", target.name, target.pid));
            ui.add_space(4.0);

            match &confirm {
                Confirm::Kill {
                    include_tree,
                    critical,
                    ..
                } => {
                    if *include_tree {
                        ui.label("All of its child processes will be ended too.");
                    }
                    if *critical {
                        ui.label(
                            RichText::new(
                                "This is a critical Windows process. Ending it will crash \
                                 or restart Windows.",
                            )
                            .color(WARNING_COLOR),
                        );
                    }
                }
                Confirm::Realtime(_) => {
                    ui.label(
                        "A busy realtime process can starve input and drivers and freeze \
                         the machine. Without admin rights Windows uses High instead.",
                    );
                }
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
                let confirm_button =
                    egui::Button::new(RichText::new(action_label).color(Color32::WHITE))
                        .fill(DANGER_COLOR);
                if ui.add(confirm_button).clicked() {
                    decision = Some(true);
                }
            });
        });
        if modal.backdrop_response.clicked() {
            decision = Some(false);
        }

        let Some(confirmed) = decision else {
            return;
        };
        self.confirm = None;
        if !confirmed {
            return;
        }
        match confirm {
            Confirm::Kill {
                target,
                include_tree,
                ..
            } => self.execute_kill(&target, include_tree),
            Confirm::Realtime(target) => self.apply_priority(&target, Priority::Realtime),
        }
    }
}
