//! The graph shown by the `interaction` example: its saved state, its viewer and its view.
//!
//! This file has no window code, so `tests/interaction_demo.rs` includes it and drives the
//! same view through headless egui passes.

use egui::{Color32, Id, Pos2, Rect, Stroke, Ui, Vec2, pos2, vec2};
use egui_snarl::{
    InPin, InPinId, NodeId, OutPin, OutPinId, Snarl,
    ui::{
        CableGeometry, InsertionAnchor, PinInfo, SnarlAnatomy, SnarlPin, SnarlStyle, SnarlViewer,
        SnarlWidget, WireAction, WirePaint, WirePins, WireRoute, WireTrigger, WireView,
    },
};
use serde::{Deserialize, Serialize};

/// The fixture the example starts from.
pub const FIXTURE: &str = include_str!("fixture.json");

/// Size of the graph view shown inside a group node, in the group's own coordinates.
pub const EMBEDDED_VIEW_SIZE: Vec2 = vec2(340.0, 200.0);

const SIGNAL: Color32 = Color32::from_rgb(64, 196, 178);
const CONTROL: Color32 = Color32::from_rgb(240, 170, 60);

/// What a pin carries. It decides the pin's colour and size, nothing else.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Signal,
    Control,
}

impl Lane {
    fn pin(self) -> PinInfo {
        match self {
            Self::Signal => PinInfo::circle().with_fill(SIGNAL),
            // Control pins are drawn smaller than the style's pin size.
            Self::Control => PinInfo::square().with_fill(CONTROL).with_size(8.0),
        }
    }
}

/// One node of the example graph.
#[derive(Clone, Serialize, Deserialize)]
pub enum Block {
    /// Emits a signal.
    Source { name: String, level: f32 },

    /// Emits a slow control value.
    Control { name: String, rate: f32 },

    /// Changes a signal by an amount a control can drive.
    Shaper { name: String, amount: f32 },

    /// Sums signals.
    Mix { gains: Vec<f32> },

    /// Receives a signal.
    Sink { name: String },

    /// Passes a signal through beside a note whose text wraps.
    Note { text: String },

    /// Holds a graph of its own behind named ports.
    Group {
        name: String,
        inputs: Vec<String>,
        outputs: Vec<String>,
        graph: Box<Snarl<Block>>,
    },
}

impl Block {
    /// The name shown in the node's header.
    pub fn title(&self) -> &str {
        match self {
            Self::Source { name, .. }
            | Self::Control { name, .. }
            | Self::Shaper { name, .. }
            | Self::Sink { name }
            | Self::Group { name, .. } => name,
            Self::Mix { .. } => "Mix",
            Self::Note { .. } => "Note",
        }
    }

    fn inputs(&self) -> usize {
        match self {
            Self::Source { .. } | Self::Control { .. } => 0,
            Self::Shaper { .. } => 2,
            Self::Mix { gains } => gains.len(),
            Self::Sink { .. } | Self::Note { .. } => 1,
            Self::Group { inputs, .. } => inputs.len(),
        }
    }

    fn outputs(&self) -> usize {
        match self {
            Self::Sink { .. } => 0,
            Self::Group { outputs, .. } => outputs.len(),
            _ => 1,
        }
    }

    fn input_lane(&self, input: usize) -> Lane {
        match self {
            Self::Shaper { .. } if input == 1 => Lane::Control,
            _ => Lane::Signal,
        }
    }

    fn output_lane(&self) -> Lane {
        match self {
            Self::Control { .. } => Lane::Control,
            _ => Lane::Signal,
        }
    }
}

/// How wires are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Routing {
    /// The view's own route for the wire style.
    Default,

    /// A single arc supplied by the viewer.
    Arc,
}

/// Everything about the look and feel of the wires that the example lets you change.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Paint wires as glowing tubes instead of plain strokes.
    pub glow: bool,

    /// Share of the pin distance the middle of a wire hangs by.
    pub slack: f32,

    /// How much wires lag behind moving nodes.
    pub sway: f32,

    /// Never animate: wires stay still and nodes open and close at once.
    pub reduced_motion: bool,

    /// How far from a wire the pointer still counts as on it, in screen points.
    pub hit_tolerance: f32,

    /// How wires are laid out.
    pub routing: Routing,

    /// Show a handle on the middle of every wire.
    pub midpoint_handles: bool,
}

/// The saved state of the example.
#[derive(Clone, Serialize, Deserialize)]
pub struct Document {
    pub settings: Settings,
    pub graph: Snarl<Block>,
}

impl Document {
    /// Reads a saved state.
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// Writes this state the way the fixture is stored.
    ///
    /// A graph keeps its wires in a hash set, so they are sorted here to make the same
    /// state always write the same text.
    pub fn to_json(&self) -> String {
        fn sort_wires(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(fields) => {
                    for (name, field) in fields.iter_mut() {
                        if name == "wires"
                            && let serde_json::Value::Array(wires) = field
                        {
                            wires.sort_by_cached_key(ToString::to_string);
                        }
                        sort_wires(field);
                    }
                }
                serde_json::Value::Array(items) => items.iter_mut().for_each(sort_wires),
                _ => {}
            }
        }

        let mut value = serde_json::to_value(self).expect("the document is plain data");
        sort_wires(&mut value);

        let mut json = serde_json::to_string_pretty(&value).expect("the document is plain data");
        json.push('\n');
        json
    }

    /// Builds the state the fixture holds.
    pub fn seed() -> Self {
        let out = |node, output| OutPinId { node, output };
        let inp = |node, input| InPinId { node, input };

        let mut inner = Snarl::new();
        let inner_in = inner.insert_node(
            pos2(0.0, 0.0),
            Block::Note {
                text: "port: in".to_owned(),
            },
        );
        let inner_drive = inner.insert_node(
            pos2(0.0, 90.0),
            Block::Control {
                name: "Wobble".to_owned(),
                rate: 0.25,
            },
        );
        let inner_shape = inner.insert_node(
            pos2(170.0, 20.0),
            Block::Shaper {
                name: "Fold".to_owned(),
                amount: 0.5,
            },
        );
        let inner_out = inner.insert_node(
            pos2(360.0, 30.0),
            Block::Sink {
                name: "port: out".to_owned(),
            },
        );
        inner.connect(out(inner_in, 0), inp(inner_shape, 0));
        inner.connect(out(inner_drive, 0), inp(inner_shape, 1));
        inner.connect(out(inner_shape, 0), inp(inner_out, 0));

        let mut graph = Snarl::new();
        let tone = graph.insert_node(
            pos2(0.0, 0.0),
            Block::Source {
                name: "Tone".to_owned(),
                level: 0.75,
            },
        );
        let noise = graph.insert_node(
            pos2(0.0, 150.0),
            Block::Source {
                name: "Noise".to_owned(),
                level: 0.25,
            },
        );
        let sweep = graph.insert_node(
            pos2(0.0, 300.0),
            Block::Control {
                name: "Sweep".to_owned(),
                rate: 0.125,
            },
        );
        let group = graph.insert_node(
            pos2(230.0, -50.0),
            Block::Group {
                name: "Voice".to_owned(),
                inputs: vec!["in".to_owned()],
                outputs: vec!["out".to_owned()],
                graph: Box::new(inner),
            },
        );
        let filter = graph.insert_node(
            pos2(250.0, 290.0),
            Block::Shaper {
                name: "Filter".to_owned(),
                amount: 0.625,
            },
        );
        let note = graph.insert_node(
            pos2(520.0, 290.0),
            Block::Note {
                text: "Edit this text. The row grows as it wraps and the pins stay on it."
                    .to_owned(),
            },
        );
        let mix = graph.insert_node(
            pos2(680.0, 40.0),
            Block::Mix {
                gains: vec![1.0, 0.75],
            },
        );
        let output = graph.insert_node(
            pos2(880.0, 60.0),
            Block::Sink {
                name: "Output".to_owned(),
            },
        );

        graph.connect(out(tone, 0), inp(group, 0));
        graph.connect(out(noise, 0), inp(filter, 0));
        graph.connect(out(sweep, 0), inp(filter, 1));
        graph.connect(out(group, 0), inp(mix, 0));
        graph.connect(out(filter, 0), inp(note, 0));
        graph.connect(out(note, 0), inp(mix, 1));
        graph.connect(out(mix, 0), inp(output, 0));

        Self {
            settings: Settings {
                glow: true,
                slack: 0.0625,
                sway: 0.5,
                reduced_motion: false,
                hit_tolerance: 9.0,
                routing: Routing::Default,
                midpoint_handles: false,
            },
            graph,
        }
    }

    /// The style the view is shown with for these settings.
    pub fn style(&self) -> SnarlStyle {
        SnarlStyle {
            wire_width: Some(2.5),
            wire_slack: Some(self.settings.slack),
            wire_sway: Some(if self.settings.reduced_motion {
                0.0
            } else {
                self.settings.sway
            }),
            wire_hit_tolerance: Some(self.settings.hit_tolerance),
            pin_size: Some(12.0),
            ..SnarlStyle::new()
        }
    }
}

/// The id of the top-level view of the example.
pub fn root_view_id() -> Id {
    Id::unique("interaction-example")
}

/// The id of the view shown inside the group node `node` of the top-level graph.
pub fn embedded_view_id(node: NodeId) -> Id {
    Id::unique(("interaction-example-embedded", node.0))
}

/// One entry of the node palette.
struct Entry {
    section: &'static str,
    name: &'static str,
    make: fn() -> Block,
}

const PALETTE: &[Entry] = &[
    Entry {
        section: "Sources",
        name: "Tone",
        make: || Block::Source {
            name: "Tone".to_owned(),
            level: 0.5,
        },
    },
    Entry {
        section: "Sources",
        name: "Noise",
        make: || Block::Source {
            name: "Noise".to_owned(),
            level: 0.5,
        },
    },
    Entry {
        section: "Sources",
        name: "Sweep",
        make: || Block::Control {
            name: "Sweep".to_owned(),
            rate: 0.1,
        },
    },
    Entry {
        section: "Processing",
        name: "Filter",
        make: || Block::Shaper {
            name: "Filter".to_owned(),
            amount: 0.5,
        },
    },
    Entry {
        section: "Processing",
        name: "Fold",
        make: || Block::Shaper {
            name: "Fold".to_owned(),
            amount: 0.5,
        },
    },
    Entry {
        section: "Processing",
        name: "Mix",
        make: || Block::Mix {
            gains: vec![1.0, 1.0],
        },
    },
    Entry {
        section: "Structure",
        name: "Note",
        make: || Block::Note {
            text: "A note".to_owned(),
        },
    },
    Entry {
        section: "Structure",
        name: "Output",
        make: || Block::Sink {
            name: "Output".to_owned(),
        },
    },
];

/// Shows the example graph and remembers what happened in it.
pub struct Viewer {
    pub settings: Settings,

    /// What was last done to a wire or with a menu, for the status line.
    pub status: String,

    /// Text typed into the search field of the node palette.
    pub search: String,

    /// How many views deep the view being shown is: 0 for the top-level graph.
    depth: usize,

    /// The group node whose graph is being shown, while it is being shown.
    showing_group: Option<NodeId>,

    /// Set while the palette was open last pass, to focus its search field once.
    palette_was_open: bool,
    palette_shown: bool,

    /// Passes left in which every view is asked to frame all of its nodes.
    fit_passes: u8,
}

impl Viewer {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            status: "Right-click or long-press the canvas to add a node, or a wire to act on it."
                .to_owned(),
            search: String::new(),
            depth: 0,
            showing_group: None,
            palette_was_open: false,
            palette_shown: false,
            fit_passes: 0,
        }
    }

    /// Frames all nodes in every view. Node sizes are known after the first pass, so the
    /// request is held for a few passes.
    pub fn fit(&mut self) {
        self.fit_passes = 6;
    }

    /// Whether a request to frame all nodes is still being held.
    pub const fn is_fitting(&self) -> bool {
        self.fit_passes > 0
    }

    /// Call once per pass before showing the graph.
    pub fn begin_pass(&mut self) {
        self.palette_was_open = self.palette_shown;
        self.palette_shown = false;
        self.fit_passes = self.fit_passes.saturating_sub(1);
    }

    fn place(&self) -> &'static str {
        if self.depth == 0 { "graph" } else { "group" }
    }

    fn insert_on_wire(&mut self, wire: WirePins, at: Pos2, snarl: &mut Snarl<Block>) {
        let lane = snarl[wire.out_pin.node].output_lane();
        if lane != Lane::Signal {
            self.status = "Only signal wires take a shaper.".to_owned();
            return;
        }

        let shaper = snarl.insert_node(
            at - vec2(60.0, 30.0),
            Block::Shaper {
                name: "Gain".to_owned(),
                amount: 0.5,
            },
        );

        snarl.disconnect(wire.out_pin, wire.in_pin);
        snarl.connect(
            wire.out_pin,
            InPinId {
                node: shaper,
                input: 0,
            },
        );
        snarl.connect(
            OutPinId {
                node: shaper,
                output: 0,
            },
            wire.in_pin,
        );

        self.status = format!("Inserted a gain on the wire at {:.0}, {:.0}.", at.x, at.y);
    }
}

fn describe(wire: WirePins, snarl: &Snarl<Block>) -> String {
    format!(
        "{} → {}",
        snarl[wire.out_pin.node].title(),
        snarl[wire.in_pin.node].title()
    )
}

impl SnarlViewer<Block> for Viewer {
    fn title(&mut self, node: &Block) -> String {
        node.title().to_owned()
    }

    fn inputs(&mut self, node: &Block) -> usize {
        node.inputs()
    }

    fn outputs(&mut self, node: &Block) -> usize {
        node.outputs()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Block>,
    ) -> impl SnarlPin + 'static {
        let lane = snarl[pin.id.node].input_lane(pin.id.input);

        match &mut snarl[pin.id.node] {
            Block::Shaper { amount, .. } if pin.id.input == 1 => {
                if pin.remotes.is_empty() {
                    ui.add(egui::Slider::new(amount, 0.0..=1.0).text("amount"));
                } else {
                    ui.label("amount (driven)");
                }
            }
            Block::Mix { gains } => {
                ui.add(
                    egui::DragValue::new(&mut gains[pin.id.input])
                        .speed(0.01)
                        .range(0.0..=2.0)
                        .prefix("gain "),
                );
            }
            Block::Note { text } => {
                // The text wraps inside a fixed width, so the row grows downwards as it is
                // edited. The pin is placed on the row as it measures in the same pass.
                ui.add(
                    egui::TextEdit::multiline(text)
                        .desired_width(150.0)
                        .desired_rows(1),
                );
            }
            Block::Group { inputs, .. } => {
                ui.label(inputs[pin.id.input].as_str());
            }
            _ => {
                ui.label("in");
            }
        }

        lane.pin()
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Block>,
    ) -> impl SnarlPin + 'static {
        let lane = snarl[pin.id.node].output_lane();

        match &mut snarl[pin.id.node] {
            Block::Source { level, .. } => {
                ui.add(egui::Slider::new(level, 0.0..=1.0).text("level"));
            }
            Block::Control { rate, .. } => {
                ui.add(
                    egui::DragValue::new(rate)
                        .speed(0.01)
                        .range(0.01..=20.0)
                        .suffix(" Hz"),
                );
            }
            Block::Group { outputs, .. } => {
                ui.label(outputs[pin.id.output].as_str());
            }
            _ => {
                ui.label("out");
            }
        }

        lane.pin()
    }

    fn has_body(&mut self, node: &Block) -> bool {
        matches!(node, Block::Group { .. })
    }

    fn show_body(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<Block>,
    ) {
        let Block::Group { graph, .. } = &mut snarl[node] else {
            return;
        };

        // The group's graph stays a graph of its own. Its view pans, zooms and clips on its
        // own, inside this node, under ids derived from the group node.
        let style = SnarlStyle {
            wire_width: Some(2.5),
            wire_slack: Some(self.settings.slack),
            wire_hit_tolerance: Some(self.settings.hit_tolerance),
            pin_size: Some(12.0),
            min_scale: Some(0.25),
            ..SnarlStyle::new()
        };

        self.depth += 1;
        let outer_group = self.showing_group.replace(node);

        SnarlWidget::new()
            .id(embedded_view_id(node))
            .style(style)
            .min_size(EMBEDDED_VIEW_SIZE)
            .max_size(EMBEDDED_VIEW_SIZE)
            .show(graph.as_mut(), self, ui);

        self.showing_group = outer_group;
        self.depth -= 1;
    }

    fn wire_route(&mut self, route: &WireRoute, _snarl: &Snarl<Block>) -> Option<CableGeometry> {
        (self.settings.routing == Routing::Arc).then(|| {
            let span = route.to - route.from;
            let middle = route.from + span * 0.5;
            let lift = span.length() * 0.25 + route.sag.y;
            CableGeometry::bezier(
                &[route.from, middle + vec2(0.0, lift), route.to],
                route.tolerance * 0.5,
            )
        })
    }

    fn wire_paint(
        &mut self,
        wire: &WireView<'_>,
        default: WirePaint,
        _snarl: &Snarl<Block>,
    ) -> WirePaint {
        if !self.settings.glow {
            return default;
        }

        // A tube: dark casing, coloured core, bright centre line, and a halo that widens
        // while the wire is hovered or selected. All of it follows the one path of the wire.
        let core = wire.width * 1.6;
        let spread = if wire.hovered || wire.selected {
            9.0
        } else {
            5.0
        };

        let paint = WirePaint::solid(Stroke::new(core, wire.color))
            .under(Stroke::new(core + 2.5, Color32::from_black_alpha(160)))
            .with_glow(wire.color, spread)
            .over(Stroke::new(
                core * 0.3,
                Color32::from_white_alpha(if wire.hovered { 220 } else { 120 }),
            ));

        if wire.selected {
            paint.under(Stroke::new(core + 6.0, Color32::WHITE))
        } else {
            paint
        }
    }

    fn wire_action(&mut self, action: &WireAction, _ui: &mut Ui, snarl: &mut Snarl<Block>) {
        let wire = describe(action.wire, snarl);

        match action.trigger {
            WireTrigger::Click => {
                self.status = format!(
                    "Selected {wire} in the {}. Delete removes it, Enter opens its menu.",
                    self.place()
                );
            }
            WireTrigger::DoubleClick => {
                self.insert_on_wire(action.wire, action.anchor.pos, snarl);
            }
            WireTrigger::Delete => {
                snarl.disconnect(action.wire.out_pin, action.wire.in_pin);
                self.status = format!("Removed {wire}.");
            }
            // These open the wire menu instead; see `has_wire_menu`.
            WireTrigger::SecondaryClick | WireTrigger::LongTouch | WireTrigger::Activate => {}
        }
    }

    fn has_wire_menu(&mut self, _wire: WirePins, _snarl: &mut Snarl<Block>) -> bool {
        true
    }

    fn show_wire_menu(
        &mut self,
        wire: WirePins,
        anchor: InsertionAnchor,
        ui: &mut Ui,
        snarl: &mut Snarl<Block>,
    ) {
        ui.strong(describe(wire, snarl));
        ui.separator();

        if ui.button("Insert gain here").clicked() {
            self.insert_on_wire(wire, anchor.graph_pos, snarl);
            ui.close();
        }

        if ui.button("Disconnect").clicked() {
            let name = describe(wire, snarl);
            snarl.disconnect(wire.out_pin, wire.in_pin);
            self.status = format!("Removed {name}.");
            ui.close();
        }
    }

    fn has_wire_widget(&mut self, _from: &OutPinId, _to: &InPinId, _snarl: &Snarl<Block>) -> bool {
        self.settings.midpoint_handles
    }

    fn show_wire_widget(
        &mut self,
        from: &OutPin,
        to: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Block>,
    ) {
        let wire = WirePins::new(from.id, to.id);
        if ui
            .small_button("×")
            .on_hover_text("Disconnect this wire")
            .clicked()
        {
            let name = describe(wire, snarl);
            snarl.disconnect(wire.out_pin, wire.in_pin);
            self.status = format!("Removed {name} from its handle.");
        }
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<Block>) -> bool {
        true
    }

    fn show_graph_menu(
        &mut self,
        pos: Pos2,
        _selected_nodes: &[NodeId],
        ui: &mut Ui,
        snarl: &mut Snarl<Block>,
    ) {
        ui.set_width(190.0);

        let search = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search nodes")
                .desired_width(f32::INFINITY),
        );

        if self.depth == 0 {
            if !self.palette_was_open {
                search.request_focus();
            }
            self.palette_shown = true;
        }

        let needle = self.search.to_lowercase();
        let mut chosen = None;
        let mut any = false;

        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                let mut sections: Vec<&str> = PALETTE.iter().map(|entry| entry.section).collect();
                sections.dedup();

                for section in sections {
                    let entries: Vec<&Entry> = PALETTE
                        .iter()
                        .filter(|entry| {
                            entry.section == section && entry.name.to_lowercase().contains(&needle)
                        })
                        .collect();

                    if entries.is_empty() {
                        continue;
                    }
                    any = true;

                    // Opening and closing a section is a click inside the popup. The popup
                    // stays open and `pos` stays where the menu was opened.
                    egui::CollapsingHeader::new(section)
                        .default_open(true)
                        .show(ui, |ui| {
                            for entry in entries {
                                if ui.button(entry.name).clicked() {
                                    chosen = Some(entry);
                                }
                            }
                        });
                }

                if !any {
                    ui.weak("No node matches.");
                }
            });

        ui.separator();
        ui.weak(format!("Inserts at {:.0}, {:.0}", pos.x, pos.y));

        if let Some(entry) = chosen {
            snarl.insert_node(pos, (entry.make)());
            self.status = format!(
                "Added {} to the {} at {:.0}, {:.0}.",
                entry.name,
                self.place(),
                pos.x,
                pos.y
            );
            self.search.clear();
            ui.close();
        }
    }

    fn menu_close_behavior(&self) -> egui::PopupCloseBehavior {
        egui::PopupCloseBehavior::CloseOnClickOutside
    }

    fn center_view(&mut self, _snarl: &mut Snarl<Block>) -> bool {
        self.fit_passes > 0
    }

    fn measured(&mut self, anatomy: &SnarlAnatomy, ui: &mut Ui, _snarl: &mut Snarl<Block>) {
        // Mark the point an open menu inserts at. It is drawn in graph coordinates, so it
        // shows that the point holds still while the popup is moved about the screen.
        if let Some(anchor) = anatomy.active_insertion_anchor {
            let reach = 7.0 / anatomy.transform.scale();
            let stroke = Stroke::new(1.5 / anatomy.transform.scale(), Color32::WHITE);
            let at = anchor.graph_pos;

            ui.painter()
                .line_segment([at - vec2(reach, 0.0), at + vec2(reach, 0.0)], stroke);
            ui.painter()
                .line_segment([at - vec2(0.0, reach), at + vec2(0.0, reach)], stroke);
            ui.painter().circle_stroke(at, reach * 0.6, stroke);
        }
    }
}

/// Shows the example graph in `ui`, filling what is left of it.
pub fn show(document: &mut Document, viewer: &mut Viewer, ui: &mut Ui) -> Rect {
    viewer.settings = document.settings;
    viewer.begin_pass();

    ui.style_mut().animation_time = if document.settings.reduced_motion {
        0.0
    } else {
        ui.style().animation_time
    };

    // Titles and port names are handles for dragging a node, not text to select.
    ui.style_mut().interaction.selectable_labels = false;

    if viewer.is_fitting() {
        ui.ctx().request_repaint();
    }

    SnarlWidget::new()
        .id(root_view_id())
        .style(document.style())
        .show(&mut document.graph, viewer, ui)
        .rect
}
