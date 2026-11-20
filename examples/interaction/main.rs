//! A graph that exercises measured pins, single-path wires, wire actions, a graph view
//! inside a node and a searchable insert menu.
//!
//! ```text
//! cargo run --example interaction --features serde
//! cargo run --example interaction --features serde -- --size 390x844
//! cargo run --example interaction --features serde -- --fixture my-state.json
//! cargo run --example interaction --features serde -- --write-fixture examples/interaction/fixture.json
//! ```

#![allow(missing_docs)]

mod graph;

use std::path::PathBuf;

use eframe::App;
use egui::Ui;
use egui_snarl::ui::get_snarl_anatomy;
use graph::{Block, Document, Routing, Viewer};

/// Below this width the settings fold away and the readout shrinks to one line.
const NARROW: f32 = 600.0;

struct Options {
    size: [f32; 2],
    fixture: Option<PathBuf>,
    save: PathBuf,
    write_fixture: Option<PathBuf>,
}

impl Options {
    fn from_args() -> Result<Self, String> {
        let mut options = Self {
            size: [1440.0, 900.0],
            fixture: None,
            save: std::env::temp_dir().join("egui-snarl-interaction.json"),
            write_fixture: None,
        };

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));

            match arg.as_str() {
                "--size" => {
                    let value = value()?;
                    let size = value
                        .split_once('x')
                        .and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?]));
                    options.size = size.ok_or_else(|| {
                        format!("--size takes WIDTHxHEIGHT, such as 390x844, not {value}")
                    })?;
                }
                "--fixture" => options.fixture = Some(value()?.into()),
                "--save" => options.save = value()?.into(),
                "--write-fixture" => options.write_fixture = Some(value()?.into()),
                other => {
                    return Err(format!(
                        "unknown argument {other}; expected --size, --fixture, --save or --write-fixture"
                    ));
                }
            }
        }

        Ok(options)
    }
}

struct InteractionApp {
    document: Document,
    viewer: Viewer,
    save_path: PathBuf,
}

impl InteractionApp {
    fn new(document: Document, save_path: PathBuf) -> Self {
        let mut viewer = Viewer::new(document.settings);
        viewer.fit();

        Self {
            document,
            viewer,
            save_path,
        }
    }

    fn replace(&mut self, document: Document, what: &str) {
        self.document = document;
        self.viewer.fit();
        self.viewer.status = what.to_owned();
    }

    fn settings(&mut self, ui: &mut Ui) {
        let settings = &mut self.document.settings;

        ui.checkbox(&mut settings.glow, "Glow")
            .on_hover_text("Paint each wire as a tube with a halo, along the path it is hit on");
        // The sliders round what they show, not what is saved: a slider told to keep two
        // decimals would write the rounded number back into the settings on every pass.
        ui.add(
            egui::Slider::new(&mut settings.slack, 0.0..=0.4)
                .text("slack")
                .custom_formatter(|value, _| format!("{value:.2}")),
        );
        ui.add_enabled(
            !settings.reduced_motion,
            egui::Slider::new(&mut settings.sway, 0.0..=1.0)
                .text("sway")
                .custom_formatter(|value, _| format!("{value:.2}")),
        );
        ui.checkbox(&mut settings.reduced_motion, "Reduced motion")
            .on_hover_text("Wires hold still and nodes open and close at once");
        ui.add(
            egui::Slider::new(&mut settings.hit_tolerance, 2.0..=24.0)
                .text("hit")
                .custom_formatter(|value, _| format!("{value:.0}")),
        )
        .on_hover_text("How far from a wire the pointer still counts as on it, in screen points");

        let mut arc = settings.routing == Routing::Arc;
        ui.checkbox(&mut arc, "Arc route")
            .on_hover_text("Lay wires out with a route supplied by the viewer");
        settings.routing = if arc { Routing::Arc } else { Routing::Default };

        ui.checkbox(&mut settings.midpoint_handles, "Handles")
            .on_hover_text("Show a handle on the middle of every wire");
        ui.checkbox(&mut settings.pulses, "Pulses").on_hover_text(
            "Paint the wires into an output as tubes with travelling pulses, above the nodes",
        );
    }

    fn files(&mut self, ui: &mut Ui) {
        if ui.button("Fit").clicked() {
            self.viewer.fit();
        }

        if ui
            .button("Reset")
            .on_hover_text("Go back to the fixture the example ships with")
            .clicked()
        {
            match Document::from_json(graph::FIXTURE) {
                Ok(document) => self.replace(document, "Went back to the shipped fixture."),
                Err(err) => self.viewer.status = format!("The shipped fixture did not load: {err}"),
            }
        }

        let path = self.save_path.display().to_string();

        if ui
            .button("Save")
            .on_hover_text(format!("Write this state to {path}"))
            .clicked()
        {
            self.viewer.status = match std::fs::write(&self.save_path, self.document.to_json()) {
                Ok(()) => format!("Saved to {path}."),
                Err(err) => format!("Could not save to {path}: {err}"),
            };
        }

        if ui
            .button("Load")
            .on_hover_text(format!("Read the state saved to {path}"))
            .clicked()
        {
            match load(&self.save_path) {
                Ok(document) => self.replace(document, &format!("Loaded {path}.")),
                Err(err) => self.viewer.status = err,
            }
        }
    }

    fn readout(&self, ui: &mut Ui, narrow: bool) {
        let Some(anatomy) = get_snarl_anatomy(graph::root_view_id(), ui.ctx()) else {
            return;
        };

        let wire = |wire: egui_snarl::ui::WirePins| {
            format!(
                "{} → {}",
                title(&self.document.graph, wire.out_pin.node),
                title(&self.document.graph, wire.in_pin.node)
            )
        };
        let hovered = anatomy.hovered_wire().map(|hovered| wire(hovered.pins));
        let selected = anatomy.selected_wire.map(wire);

        if narrow {
            ui.weak(format!(
                "{} nodes · {} wires · ×{:.2}{}",
                anatomy.nodes.len(),
                anatomy.wires.len(),
                anatomy.transform.scale(),
                selected.map_or_else(String::new, |wire| format!(" · {wire}")),
            ));
            return;
        }

        ui.strong("Measured this pass");
        let ctx = ui.ctx().clone();
        egui::Grid::new("measured").num_columns(2).show(ui, |ui| {
            let mut row = |name: &str, value: String| {
                ui.weak(name);
                ui.label(value);
                ui.end_row();
            };

            row("zoom", format!("×{:.2}", anatomy.transform.scale()));
            row("nodes", anatomy.nodes.len().to_string());
            row(
                "pins",
                (anatomy.input_pins.len() + anatomy.output_pins.len()).to_string(),
            );
            row("wires", anatomy.wires.len().to_string());
            row("hovered wire", hovered.unwrap_or_else(|| "–".to_owned()));
            row("selected wire", selected.unwrap_or_else(|| "–".to_owned()));
            row(
                "pointer in group view",
                if anatomy.pointer_in_embedded_view {
                    "yes"
                } else {
                    "no"
                }
                .to_owned(),
            );

            for (node, block) in self.document.graph.node_ids() {
                if let Block::Group { name, .. } = block
                    && let Some(inner) = get_snarl_anatomy(graph::embedded_view_id(node), &ctx)
                {
                    row(
                        &format!("{name} view"),
                        format!(
                            "×{:.2} of its node, {} nodes, {} wires",
                            inner.transform.local_scale(),
                            inner.nodes.len(),
                            inner.wires.len()
                        ),
                    );
                }
            }
        });

        ui.add_space(8.0);
        ui.strong("Try");
        for line in [
            "Hover a wire: the halo widens along the path the pointer is tested against.",
            "Click a wire to select it; Delete removes it, Enter opens its menu.",
            "Right-click or long-press a wire for its menu; double-click to insert a gain.",
            "Drag from a pin to connect, or from a connected input to move the wire.",
            "Scroll and drag inside the Voice node: only its own view pans and zooms.",
            "Collapse the Voice node: its wires stay on the same ports.",
            "Right-click the canvas near an edge: the mark stays where you clicked.",
            "Type in the Note: its row grows and the pins follow in the same frame.",
            "Tick Pulses: the wires into an output become tubes that can still be picked.",
        ] {
            ui.label(format!("• {line}"));
        }
    }
}

fn title(graph: &egui_snarl::Snarl<Block>, node: egui_snarl::NodeId) -> &str {
    graph.get_node(node).map_or("?", Block::title)
}

fn load(path: &std::path::Path) -> Result<Document, String> {
    let json = std::fs::read_to_string(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    Document::from_json(&json)
        .map_err(|err| format!("{} is not a saved state: {err}", path.display()))
}

impl App for InteractionApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let narrow = ui.available_width() < NARROW;

        egui::Panel::top("settings").show(ui, |ui| {
            if narrow {
                ui.horizontal_wrapped(|ui| self.files(ui));
                egui::CollapsingHeader::new("Wires")
                    .default_open(false)
                    .show(ui, |ui| self.settings(ui));
            } else {
                ui.horizontal_wrapped(|ui| {
                    self.files(ui);
                    ui.separator();
                    self.settings(ui);
                });
            }
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| ui.label(self.viewer.status.as_str()));
            if narrow {
                self.readout(ui, true);
            }
        });

        if !narrow {
            egui::Panel::right("readout")
                .default_size(270.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.readout(ui, false));
                });
        }

        egui::CentralPanel::default().show(ui, |ui| {
            graph::show(&mut self.document, &mut self.viewer, ui);
        });
    }

    // Every run starts from the fixture: panel sizes and folded sections are not carried
    // over from the last run.
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

    if let Some(path) = &options.write_fixture {
        if let Err(err) = std::fs::write(path, Document::seed().to_json()) {
            eprintln!("Could not write {}: {err}", path.display());
            std::process::exit(1);
        }
        println!("Wrote {}", path.display());
        return Ok(());
    }

    let document = match &options.fixture {
        Some(path) => load(path),
        None => Document::from_json(graph::FIXTURE)
            .map_err(|err| format!("The shipped fixture did not load: {err}")),
    };
    let document = match document {
        Ok(document) => document,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(options.size)
            .with_min_inner_size([320.0, 480.0]),
        // The window opens at the size asked for, not the size it was last closed at.
        persist_window: false,
        ..Default::default()
    };

    eframe::run_native(
        "egui-snarl interaction",
        native_options,
        Box::new(move |_cx| Ok(Box::new(InteractionApp::new(document, options.save)))),
    )
}
