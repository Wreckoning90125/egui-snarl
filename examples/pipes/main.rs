//! Five kinds of tube motion beside the travelling beads they are an alternative to.
//!
//! ```text
//! cargo run --example pipes
//! cargo run --example pipes -- --size 1440x900 --sweep
//! ```

#![allow(missing_docs)]

mod flow;
mod graph;
mod pipe;
#[path = "../interaction/tube.rs"]
mod tube;

use eframe::App;
use egui::{Align2, Color32, FontId, Id, Ui, vec2};
use egui_snarl::{Snarl, ui::get_snarl_anatomy};
use flow::Mechanism;
use graph::{Assign, Look, Node, Viewer};

/// Seconds the command takes to swing from full push to full pull and back when it sweeps.
const SWEEP_PERIOD: f64 = 14.0;

struct Options {
    size: [f32; 2],
    sweep: bool,
}

impl Options {
    fn from_args() -> Result<Self, String> {
        let mut options = Self {
            size: [1440.0, 900.0],
            sweep: false,
        };

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--size" => {
                    let value = args.next().ok_or("--size needs a value")?;
                    let size = value
                        .split_once('x')
                        .and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?]));
                    options.size = size.ok_or_else(|| {
                        format!("--size takes WIDTHxHEIGHT, such as 1440x900, not {value}")
                    })?;
                }
                "--sweep" => options.sweep = true,
                other => {
                    return Err(format!(
                        "unknown argument {other}; expected --size or --sweep"
                    ));
                }
            }
        }

        Ok(options)
    }
}

struct PipesApp {
    snarl: Snarl<Node>,
    viewer: Viewer,

    /// What the left view shows the wires as, to compare the pipes against.
    compare_with: Look,
    sweep: bool,
    paused: bool,
    reduced_motion: bool,
}

impl PipesApp {
    fn new(sweep: bool) -> Self {
        let mut viewer = Viewer::new();
        viewer.fit();

        Self {
            snarl: graph::seed(),
            viewer,
            compare_with: Look::Beads,
            sweep,
            paused: false,
            reduced_motion: false,
        }
    }

    fn still(&self) -> bool {
        self.paused || self.reduced_motion
    }

    fn controls(&mut self, ui: &mut Ui) {
        ui.label("Pipes:");
        ui.selectable_value(&mut self.viewer.assign, Assign::Each, "One of each")
            .on_hover_text("Each pipe is moved by the mechanism its sink is named after");
        for mechanism in Mechanism::ALL {
            ui.selectable_value(
                &mut self.viewer.assign,
                Assign::All(mechanism),
                mechanism.name(),
            )
            .on_hover_text(mechanism.coupling());
        }

        ui.separator();
        ui.label("Compare with:");
        ui.selectable_value(&mut self.compare_with, Look::Beads, "Beads")
            .on_hover_text(
                "Discs travelling along a line: the look these pipes are an alternative to",
            );
        ui.selectable_value(&mut self.compare_with, Look::Plain, "Plain")
            .on_hover_text("The same graph with nothing asked of its wires");

        ui.separator();
        ui.add_enabled(
            !self.sweep,
            egui::Slider::new(&mut self.viewer.command, -1.0..=1.0)
                .text("command")
                .custom_formatter(|value, _| format!("{value:+.2}")),
        )
        .on_hover_text("How hard the sources push. Negative pulls.");
        ui.checkbox(&mut self.sweep, "Sweep")
            .on_hover_text("Swing the command slowly from full push to full pull and back");
        ui.checkbox(&mut self.paused, "Pause");
        ui.checkbox(&mut self.reduced_motion, "Reduced motion")
            .on_hover_text("Hold every pipe and every bead still");

        ui.separator();
        if ui.button("Fit").clicked() {
            self.viewer.fit();
        }
        if ui
            .button("Reset")
            .on_hover_text("Put the graph back and every pipe to rest")
            .clicked()
        {
            self.snarl = graph::seed();
            self.viewer.reset();
            self.viewer.fit();
        }
    }

    fn view(&mut self, look: Look, name: &str, caption: &str, ui: &mut Ui) {
        let rect = ui.available_rect_before_wrap();
        graph::show(
            &mut self.snarl,
            &mut self.viewer,
            look,
            Id::unique(name),
            ui,
        );

        ui.painter().text(
            rect.left_top() + vec2(10.0, 8.0),
            Align2::LEFT_TOP,
            caption,
            FontId::proportional(14.0),
            Color32::from_gray(210),
        );
    }

    fn status(&self, ui: &Ui) -> String {
        let pointer = ui.ctx().pointer_latest_pos();
        let on_pipe = pointer.and_then(|pointer| {
            let anatomy = get_snarl_anatomy(Id::unique("pipes"), ui.ctx())?;
            graph::readout(&self.snarl, &self.viewer, &anatomy, pointer)
        });

        on_pipe.unwrap_or_else(|| {
            "Point at a pipe: it is picked between its walls as they are now. \
             Click selects it, a secondary click removes it, drag from a pin to lay it again."
                .to_owned()
        })
    }
}

impl App for PipesApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if self.sweep && !self.still() {
            let time = ui.input(|input| input.time);
            #[allow(clippy::cast_possible_truncation)]
            {
                self.viewer.command = (std::f64::consts::TAU * time / SWEEP_PERIOD).cos() as f32;
            }
        }

        let seconds = if self.still() {
            0.0
        } else {
            ui.input(|input| input.stable_dt)
        };
        self.viewer.advance(&self.snarl, seconds);

        if !self.still() || self.viewer.is_fitting() {
            ui.ctx().request_repaint();
        }

        egui::Panel::top("controls").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| self.controls(ui));
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            let status = self.status(ui);
            ui.horizontal_wrapped(|ui| ui.label(status));
        });

        let half = 0.5 * ui.available_width();
        egui::Panel::left("compare")
            .resizable(true)
            .default_size(half)
            .show(ui, |ui| {
                let caption = match self.compare_with {
                    Look::Beads => "A  Beads travelling along a line",
                    _ => "A  The same graph, nothing asked of its wires",
                };
                self.view(self.compare_with, "compare", caption, ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let caption = match self.viewer.assign {
                Assign::Each => "B  Pipes that move, one mechanism each".to_owned(),
                Assign::All(mechanism) => {
                    format!("B  {}: {}", mechanism.name(), mechanism.coupling())
                }
            };
            self.view(Look::Pipes, "pipes", &caption, ui);
        });
    }

    // Every run starts the same: panel sizes are not carried over from the last run.
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

fn main() -> eframe::Result<()> {
    let options = match Options::from_args() {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(options.size)
            .with_min_inner_size([640.0, 480.0]),
        // The window opens at the size asked for, not the size it was last closed at.
        persist_window: false,
        ..Default::default()
    };

    eframe::run_native(
        "egui-snarl pipes",
        native_options,
        Box::new(move |_cx| Ok(Box::new(PipesApp::new(options.sweep)))),
    )
}
