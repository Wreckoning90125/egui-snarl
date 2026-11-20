//! The viewer that shows the patch with one of three looks.
//!
//! It changes nothing but wires and pins, and only through `wire_route`, `wire_paint`
//! and the pin a row returns. With [`Look::Plain`] it returns what the view offered, and
//! the graph is an ordinary one.

use std::collections::HashMap;

use egui::{
    Color32, CornerRadius, Frame, Id, Margin, Painter, Rect, RichText, Stroke, Style, Ui, Vec2,
    vec2,
};
use egui_snarl::{
    InPin, NodeId, OutPin, Snarl,
    ui::{
        BackgroundPattern, CableGeometry, Grid, NodeLayout, PinInfo, PinPlacement, PinWireInfo,
        SnarlAnatomy, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget, WireLayer, WirePaint,
        WirePins, WireRoute, WireStyle, WireView,
    },
};

use crate::{
    patch::{self, Kind, Light, Node, Signals},
    recipe::{self, Carry, Conduit, PipeView, Socket},
};

/// What the wires and pins are shown as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// What the view draws when nothing is asked of it.
    Plain,

    /// The product's graph drawer: strokes across the wire and discs along it.
    Drawer,

    /// The product's rack: a lit tube of glass or rubber, with collars.
    Rack,
}

/// A pin as a look shows it.
pub enum Pin {
    Plain(PinInfo),
    Socket(Socket),
}

impl SnarlPin for Pin {
    fn draw(
        self,
        snarl_style: &SnarlStyle,
        style: &Style,
        rect: Rect,
        painter: &Painter,
    ) -> PinWireInfo {
        match self {
            Self::Plain(pin) => pin.draw(snarl_style, style, rect, painter),
            Self::Socket(socket) => socket.draw(snarl_style, style, rect, painter),
        }
    }
}

pub struct Viewer {
    pub look: Look,

    /// The light the paints are seen in. A stand-in: see [`Light`].
    pub light: Light,

    /// The paint on the brush, which the controls that are no pigment hand on.
    pub brush: [f32; 8],

    pub signals: Signals,

    /// What each wire carries, as of the last call to [`Viewer::advance`].
    carries: HashMap<WirePins, Carry>,

    fit_passes: u8,
}

impl Viewer {
    pub fn new() -> Self {
        Self {
            look: Look::Rack,
            light: Light { uv: 1.0 },
            // Mostly rose with some yellow.
            brush: [0.7, 0.0, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0],
            signals: Signals::default(),
            carries: HashMap::new(),
            fit_passes: 0,
        }
    }

    /// Frames all nodes over the next few passes, in every view shown with this viewer.
    pub fn fit(&mut self) {
        self.fit_passes = 6;
    }

    pub const fn is_fitting(&self) -> bool {
        self.fit_passes > 0
    }

    /// Call once per frame, before the graph is shown, with the time since the last
    /// frame. Pass zero to hold everything still.
    pub fn advance(&mut self, snarl: &Snarl<Node>, seconds: f32) {
        self.fit_passes = self.fit_passes.saturating_sub(1);
        self.signals.advance(snarl, seconds);
        self.carries = snarl
            .wires()
            .map(|(out_pin, in_pin)| {
                (
                    WirePins::new(out_pin, in_pin),
                    patch::carry(snarl, out_pin.node, in_pin.node, self.light, self.brush),
                )
            })
            .collect();
    }

    pub fn carry(&self, wire: WirePins) -> Option<Carry> {
        self.carries.get(&wire).copied()
    }

    /// What every connection on a pin carries.
    fn ends(&self, on_pin: impl Fn(&WirePins) -> bool) -> Vec<Carry> {
        let mut wires: Vec<&WirePins> = self.carries.keys().filter(|wire| on_pin(wire)).collect();
        // In one order every pass, whatever order the map holds them in.
        wires.sort_by_key(|wire| {
            (
                wire.out_pin.node.0,
                wire.out_pin.output,
                wire.in_pin.node.0,
                wire.in_pin.input,
            )
        });
        wires.into_iter().map(|wire| self.carries[wire]).collect()
    }

    fn pin(&self, ends: &[Carry], wired: bool) -> Pin {
        match self.look {
            Look::Plain => Pin::Plain(PinInfo::circle()),
            Look::Drawer | Look::Rack => Pin::Socket(Socket(recipe::socket(ends, wired))),
        }
    }
}

impl SnarlViewer<Node> for Viewer {
    fn title(&mut self, node: &Node) -> String {
        node.label.clone()
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) {
        let title = self.title(&snarl[node]);
        ui.add(egui::Label::new(RichText::new(title).strong()).selectable(false));
    }

    fn inputs(&mut self, node: &Node) -> usize {
        node.inputs.len()
    }

    fn outputs(&mut self, node: &Node) -> usize {
        node.outputs()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        let node = &snarl[pin.id.node];
        ui.small(node.inputs.get(pin.id.input).copied().unwrap_or("in"));
        if matches!(node.kind, Kind::Target(_)) {
            ui.small(format!("{:+.2}", self.signals.output(pin.id.node)));
        }
        let ends = self.ends(|wire| wire.in_pin == pin.id);
        self.pin(&ends, !pin.remotes.is_empty())
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        _snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        ui.small(format!("{:+.2}", self.signals.output(pin.id.node)));
        let ends = self.ends(|wire| wire.out_pin == pin.id);
        self.pin(&ends, !pin.remotes.is_empty())
    }

    fn wire_route(&mut self, route: &WireRoute, _snarl: &Snarl<Node>) -> Option<CableGeometry> {
        // A wire still being dragged from a pin is no connection yet.
        let carry = self.carry(route.pins?)?;
        match self.look {
            // The drawer's strokes follow the path the view lays.
            Look::Plain | Look::Drawer => None,
            Look::Rack => Some(recipe::rack_route(route, carry.conduit)),
        }
    }

    fn wire_paint(
        &mut self,
        wire: &WireView<'_>,
        default: WirePaint,
        _snarl: &Snarl<Node>,
    ) -> WirePaint {
        let Some(pins) = wire.pins else {
            return default;
        };
        let Some(carry) = self.carry(pins) else {
            return default;
        };
        let stream = self.signals.stream(pins);

        match self.look {
            Look::Plain => default,
            Look::Drawer => recipe::conduit(carry, PipeView::of(wire, &default), stream),
            // Only a path that came from `rack_route` has walls to fill.
            Look::Rack if wire.geometry.radii().is_some() => recipe::rack(wire, carry, stream),
            Look::Rack => default,
        }
    }

    fn center_view(&mut self, _snarl: &mut Snarl<Node>) -> bool {
        self.fit_passes > 0
    }

    fn measured(&mut self, anatomy: &SnarlAnatomy, _ui: &mut Ui, _snarl: &mut Snarl<Node>) {
        // A view can only be framed once its nodes have been measured.
        if self.fit_passes > 0 && anatomy.nodes.iter().any(|node| node.remeasured) {
            self.fit_passes = self.fit_passes.max(3);
        }
    }
}

/// How the nodes and the canvas look, whatever the wires are shown as: the canvas of
/// `av-native` at `b6665d4`, `control_graph.rs`.
pub fn canvas() -> SnarlStyle {
    let frame = |margin: i8, fill: u8, stroke: Stroke| Frame {
        inner_margin: Margin::same(margin),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::same(10),
        fill: Color32::from_gray(fill),
        stroke,
        shadow: egui::Shadow::NONE,
    };

    SnarlStyle {
        node_layout: Some(
            NodeLayout::coil()
                .with_min_pin_row_height(24.0)
                .with_equal_pin_rows(),
        ),
        node_frame: Some(frame(8, 0x2a, Stroke::new(1.0, Color32::from_gray(0x4a)))),
        header_frame: Some(frame(6, 0x3a, Stroke::NONE)),
        bg_frame: Some(Frame {
            corner_radius: CornerRadius::ZERO,
            ..frame(2, 0x20, Stroke::NONE)
        }),
        bg_pattern: Some(BackgroundPattern::Grid(Grid::new(vec2(28.0, 28.0), 0.0))),
        bg_pattern_stroke: Some(Stroke::new(0.6, Color32::from_gray(0x33))),
        wire_style: Some(WireStyle::Bezier3),
        wire_layer: Some(WireLayer::BehindNodes),
        pin_size: Some(recipe::PIN_VISUAL_SIZE),
        pin_placement: Some(PinPlacement::Edge),
        header_drag_space: Some(Vec2::ZERO),
        collapsible: Some(false),
        ..SnarlStyle::new()
    }
}

/// The style a look is shown with. A plain graph is asked nothing of its wires: it has
/// the canvas and no more.
///
/// The two recipes ask for wires that can be picked, since a selected wire is the one
/// they raise above the nodes, and for nodes that cover the wires behind them.
pub fn style(look: Look) -> SnarlStyle {
    match look {
        Look::Plain => canvas(),
        Look::Drawer | Look::Rack => SnarlStyle {
            wire_picking: Some(true),
            nodes_cover_wires: Some(true),
            ..canvas()
        },
    }
}

/// Shows the graph with one look, in a view of its own.
pub fn show(snarl: &mut Snarl<Node>, viewer: &mut Viewer, look: Look, id: Id, ui: &mut Ui) {
    viewer.look = look;

    // Titles and port names are handles for dragging a node, not text to select.
    ui.style_mut().interaction.selectable_labels = false;

    SnarlWidget::new()
        .id(id)
        .style(style(look))
        .show(snarl, viewer, ui);
}

/// What kind of connection a wire is, in a word.
pub const fn kind(conduit: Conduit) -> &'static str {
    match conduit {
        Conduit::Pipe => "pipe",
        Conduit::Hose => "hose",
    }
}
