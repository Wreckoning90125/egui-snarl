//! Five kinds of tube motion beside the travelling beads they are an alternative to.
//!
//! ```text
//! cargo run --example pipes
//! cargo run --example pipes -- --size 1440x900 --sweep
//! ```

#![allow(missing_docs)]

mod drive;
mod flow;
mod graph;
mod pipe;
#[path = "../interaction/tube.rs"]
mod tube;

use std::ops::RangeInclusive;

use drive::Role;
use eframe::App;
use egui::{Align2, Color32, FontId, Id, Ui, vec2};
use egui_snarl::{Snarl, ui::get_snarl_anatomy};
use flow::{Mechanism, Params};
use graph::{Assign, Look, Node, Viewer};
use pipe::Ends;

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

    /// Whether the panel with every number of the models is open.
    parameters: bool,
}

/// One number of a model, with the range it can be set in.
fn number(ui: &mut Ui, name: &str, value: &mut f32, range: RangeInclusive<f32>, about: &str) {
    ui.add(egui::Slider::new(value, range).text(name))
        .on_hover_text(about);
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
            parameters: false,
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
        ui.label("Ends:");
        ui.selectable_value(&mut self.viewer.style.ends, Ends::Behind, "Behind")
            .on_hover_text("A pipe runs in behind the node it meets");
        ui.selectable_value(&mut self.viewer.style.ends, Ends::Fittings, "Fittings")
            .on_hover_text("A fitting sits on the front of the node at each end of a pipe");
    }

    fn drives(&mut self, ui: &mut Ui) {
        let signal = &mut self.viewer.drives.signal;

        ui.label("Signal:");
        ui.selectable_value(&mut signal.role, Role::Pump, "pumps")
            .on_hover_text("The signal pushes the contents");
        ui.selectable_value(&mut signal.role, Role::Valve, "opens a valve")
            .on_hover_text(
                "The signal lets through what something else pushes. \
                 With no gravity there is nothing to let through.",
            );
        ui.add_enabled(
            !self.sweep,
            egui::Slider::new(&mut signal.command, -1.0..=1.0)
                .text("command")
                .custom_formatter(|value, _| format!("{value:+.2}")),
        )
        .on_hover_text(
            "How hard the sources push, or how far the valves are open. Negative pulls.",
        );
        ui.checkbox(&mut self.sweep, "Sweep")
            .on_hover_text("Swing the command slowly from full push to full pull and back");

        ui.separator();
        ui.add(
            egui::Slider::new(&mut self.viewer.drives.gravity.strength, 0.0..=2.0).text("gravity"),
        )
        .on_hover_text(
            "Pushes the contents of a pipe toward its lower end and lets the pipe sag. \
             Zero is a pipe driven by the signal alone.",
        );

        ui.separator();
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
        if ui
            .toggle_value(&mut self.parameters, "Parameters")
            .on_hover_text("Every number the models and the look use")
            .changed()
        {
            self.viewer.fit();
        }
    }

    fn parameters(&mut self, ui: &mut Ui) {
        if ui
            .button("Defaults")
            .on_hover_text("Put every number below back")
            .clicked()
        {
            let ends = self.viewer.style.ends;
            self.viewer.params = Params::default();
            self.viewer.style = pipe::Style {
                ends,
                ..pipe::Style::default()
            };
        }

        let Viewer {
            params,
            style,
            drives,
            ..
        } = &mut self.viewer;

        ui.collapsing("What drives a pipe", |ui| {
            number(
                ui,
                "speed",
                &mut params.speed,
                20.0..=400.0,
                "Speed of the contents under a full push",
            );
            number(
                ui,
                "gravity angle",
                &mut drives.gravity.angle,
                -180.0..=180.0,
                "Which way is down, in degrees clockwise from straight down the screen",
            );
            number(
                ui,
                "full drop",
                &mut params.full_drop,
                10.0..=400.0,
                "The drop from inlet to outlet that, under gravity of 1, pushes as hard as a full \
                 command",
            );
            number(
                ui,
                "sag",
                &mut params.sag,
                0.0..=80.0,
                "How far the middle of a level pipe 500 long sags under gravity of 1",
            );
        });

        ui.collapsing("Peristalsis", |ui| {
            let rings = &mut params.peristalsis;
            number(
                ui,
                "ring speed",
                &mut rings.speed,
                10.0..=400.0,
                "Speed of the rings of contraction under a full push",
            );
            number(
                ui,
                "spacing",
                &mut rings.spacing,
                24.0..=300.0,
                "Distance from one ring to the next",
            );
            number(
                ui,
                "ring length",
                &mut rings.length,
                8.0..=300.0,
                "Length of pipe one ring narrows. What is left of the spacing stays fully open.",
            );
            number(
                ui,
                "throat",
                &mut rings.throat,
                0.2..=1.0,
                "Cross-section left open in the middle of a ring",
            );
        });

        ui.collapsing("Pressure wave", |ui| {
            let wave = &mut params.wave;
            number(
                ui,
                "wave speed",
                &mut wave.speed,
                60.0..=900.0,
                "Speed at which a change of pressure runs along the wall",
            );
            number(
                ui,
                "friction",
                &mut wave.friction,
                0.0..=3.0,
                "Share of the flow lost each second",
            );
            number(
                ui,
                "beats",
                &mut wave.rate,
                0.2..=3.0,
                "Beats of the source each second",
            );
            number(
                ui,
                "beat share",
                &mut wave.share,
                0.05..=0.9,
                "Share of a beat during which the source pushes",
            );
            number(
                ui,
                "between beats",
                &mut wave.resting,
                0.0..=1.0,
                "Flow between beats, as a multiple of the speed",
            );
            number(
                ui,
                "at a beat",
                &mut wave.pushing,
                0.5..=3.0,
                "Flow at the height of a beat, as a multiple of the speed",
            );
        });

        ui.collapsing("Ballooning", |ui| {
            let balloon = &mut params.balloon;
            number(
                ui,
                "where",
                &mut balloon.at,
                0.1..=0.9,
                "Where along the pipe the soft stretch is",
            );
            number(
                ui,
                "reach",
                &mut balloon.reach,
                8.0..=140.0,
                "How far the soft stretch reaches to each side of its middle",
            );
            number(
                ui,
                "roundness",
                &mut balloon.roundness,
                1.0..=10.0,
                "2 is a bell, pointed in the middle. Larger is flatter across the middle, toward \
                 a sausage.",
            );
            number(
                ui,
                "threshold",
                &mut balloon.threshold,
                0.1..=0.95,
                "The push above which the soft stretch gives way",
            );
            number(
                ui,
                "growth",
                &mut balloon.growth,
                0.2..=3.0,
                "How much the cross-section grows for each unit of fill",
            );
            number(
                ui,
                "gives way",
                &mut balloon.gives_way,
                0.2..=2.0,
                "The fill at which the wall resists most",
            );
            number(
                ui,
                "gives back",
                &mut balloon.gives_back,
                0.4..=4.0,
                "The fill at which the wall resists least; it must fall to here to give back",
            );
        });

        ui.collapsing("Bourdon flex", |ui| {
            let bourdon = &mut params.bourdon;
            number(
                ui,
                "bow",
                &mut bourdon.bow,
                0.0..=0.45,
                "How far the pipe at rest bows out, as a share of the distance between its ends",
            );
            number(
                ui,
                "uncurl",
                &mut bourdon.uncurl,
                0.0..=1.0,
                "How much of its curl the pipe loses under a full push",
            );
            number(
                ui,
                "curl",
                &mut bourdon.curl,
                0.0..=0.5,
                "How much curl it gains when pulled on as hard",
            );
            number(
                ui,
                "quickness",
                &mut bourdon.rate,
                1.0..=20.0,
                "How quickly the pipe follows the push",
            );
            number(
                ui,
                "damping",
                &mut bourdon.damping,
                0.1..=1.5,
                "How strongly its swing is damped. One does not overshoot.",
            );
            number(
                ui,
                "swell",
                &mut bourdon.swell,
                0.0..=0.6,
                "How much the cross-section rounds out under a full push",
            );
        });

        ui.collapsing("Look", |ui| {
            number(
                ui,
                "half width",
                &mut style.radius,
                4.0..=18.0,
                "Half width of a pipe at rest",
            );
            number(
                ui,
                "wall",
                &mut style.wall,
                0.5..=4.0,
                "Thickness of the wall",
            );
            number(
                ui,
                "colour length",
                &mut style.band,
                16.0..=512.0,
                "Length of pipe that contents of one colour fill at rest",
            );
            number(
                ui,
                "blend",
                &mut style.blend,
                0.0..=1.0,
                "How much of that length a colour takes to run into the next",
            );
            number(
                ui,
                "roundness",
                &mut style.depth,
                0.0..=1.0,
                "How round the pipe is lit, from flat colour to a lit side and a highlight",
            );
        });
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
                self.viewer.drives.signal.command =
                    (std::f64::consts::TAU * time / SWEEP_PERIOD).cos() as f32;
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
            ui.horizontal_wrapped(|ui| self.drives(ui));
        });

        if self.parameters {
            egui::Panel::right("parameters")
                .resizable(false)
                .default_size(300.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.parameters(ui));
                });
        }

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
