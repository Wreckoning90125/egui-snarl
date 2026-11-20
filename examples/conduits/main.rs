//! The RenderLab pipes and hoses as wires of this view. See `README.md` beside this file.
//!
//! ```text
//! cargo run --example conduits
//! ```

mod graph;
mod patch;
mod recipe;

use eframe::App;
use egui::{Align2, Color32, FontId, Id, Ui, vec2};
use egui_snarl::{Snarl, ui::get_snarl_anatomy};
use graph::{Look, Viewer};
use patch::Node;

struct Options {
    size: [f32; 2],
}

impl Options {
    fn from_args() -> Result<Self, String> {
        let mut options = Self {
            size: [1440.0, 900.0],
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
                other => return Err(format!("unknown argument {other}; expected --size")),
            }
        }

        Ok(options)
    }
}

struct ConduitsApp {
    snarl: Snarl<Node>,
    viewer: Viewer,

    /// What the left view shows the wires as.
    left: Look,
    paused: bool,

    /// How wide each of the two views was when they were last fitted.
    view_width: f32,
}

impl ConduitsApp {
    fn new() -> Self {
        let mut viewer = Viewer::new();
        viewer.fit();

        Self {
            snarl: patch::seed(),
            viewer,
            left: Look::Drawer,
            paused: false,
            view_width: 0.0,
        }
    }

    fn controls(&mut self, ui: &mut Ui) {
        ui.label("Left view:");
        ui.selectable_value(&mut self.left, Look::Drawer, "Drawer recipe")
            .on_hover_text("Strokes and discs: what the product's graph drawer lays on a wire");
        ui.selectable_value(&mut self.left, Look::Plain, "Plain")
            .on_hover_text("The same graph with nothing asked of its wires or pins");

        ui.separator();
        ui.add(egui::Slider::new(&mut self.viewer.signals.pressure, 0.0..=1.0).text("pen"))
            .on_hover_text("Pressure of the pen, which a slew eases into Size");
        ui.add(egui::Slider::new(&mut self.viewer.signals.rate, 0.0..=1.0).text("rate"))
            .on_hover_text("Turns a second of the two oscillators");
        ui.add(egui::Slider::new(&mut self.viewer.light.uv, 0.0..=1.0).text("UV lamp"))
            .on_hover_text(
                "How much light of their own the fluorescent paints give off. \
                 The colours are made up for this example.",
            );

        ui.separator();
        ui.checkbox(&mut self.paused, "Pause");
        if ui.button("Fit").clicked() {
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
        let on_wire = ["drawer", "rack"].into_iter().find_map(|name| {
            let anatomy = get_snarl_anatomy(Id::unique(name), ui.ctx())?;
            let wire = anatomy.hovered_wire()?;
            let carry = self.viewer.carry(wire.pins)?;
            let stream = self.viewer.signals.stream(wire.pins);
            Some(format!(
                "{} carrying {}: signal {:+.2}, level {:.2}, glow {:.2}, {} wide where it is picked",
                graph::kind(carry.conduit),
                if carry.recipe.is_some() {
                    "paint"
                } else {
                    "no paint"
                },
                stream.signal,
                stream.level,
                carry.glow,
                wire.geometry
                    .radii()
                    .map_or(wire.paint.hit_width(), |_| 2.0 * wire.geometry.radius_at(0.5)),
            ))
        });

        on_wire.unwrap_or_else(|| {
            "Point at a connection. Click selects it and raises it above the nodes, \
             a secondary click removes it, drag from a pin to lay it again."
                .to_owned()
        })
    }
}

impl App for ConduitsApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let seconds = if self.paused {
            0.0
        } else {
            ui.input(|input| input.stable_dt)
        };
        self.viewer.advance(&self.snarl, seconds);

        if !self.paused || self.viewer.is_fitting() {
            ui.ctx().request_repaint();
        }

        egui::Panel::top("controls").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| self.controls(ui));
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            let status = self.status(ui);
            ui.horizontal_wrapped(|ui| ui.label(status));
        });

        // The two views share what is left evenly, and are fitted again when that changes.
        let half = 0.5 * ui.available_width();
        if (half - self.view_width).abs() > 0.5 {
            self.view_width = half;
            self.viewer.fit();
        }
        egui::Panel::left("left")
            .resizable(false)
            .exact_size(half)
            .show(ui, |ui| {
                let caption = match self.left {
                    Look::Plain => "A  The same graph, nothing asked of its wires or pins",
                    _ => "A  Graph drawer recipe: strokes and discs",
                };
                self.view(self.left, "drawer", caption, ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.view(
                Look::Rack,
                "rack",
                "B  Rack recipe: lit glass and rubber, as wires",
                ui,
            );
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
        "egui-snarl conduits",
        native_options,
        Box::new(move |_cx| Ok(Box::new(ConduitsApp::new()))),
    )
}
