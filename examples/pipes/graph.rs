//! The graph the pipes run through, and the viewer that gives its wires one of three looks.
//!
//! The graph is five sources, five sinks and a wire from each source to its sink. The same
//! graph is shown twice, once with travelling beads and once with pipes that move, so each
//! wire can be compared with itself.
//!
//! The viewer changes nothing but wires, and only through `wire_route` and `wire_paint`.
//! With [`Look::Plain`] it returns what the view offered, and the graph is an ordinary one.
//!
//! The nodes are laid out the way the application that uses this crate lays out its
//! canvas: pins on the edge of the node, a title at the left of a header with no arrow.
//! That is [`canvas`], and all three looks share it.

use std::{collections::HashMap, fmt::Write as _};

use egui::{
    Color32, CornerRadius, Frame, Id, Margin, Pos2, RichText, Stroke, Ui, Vec2, pos2, vec2,
};
use egui_snarl::{
    InPin, InPinId, NodeId, OutPin, OutPinId, Snarl,
    ui::{
        BackgroundPattern, CableGeometry, Grid, NodeLayout, PinInfo, PinPlacement, SnarlAnatomy,
        SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget, WireLayer, WirePaint, WirePins, WireRoute,
        WireStyle, WireView,
    },
};

use super::{
    drive::{Drives, Forcing, Lay},
    flow::{Mechanism, Params, Pipe},
    pipe, tube,
};

/// Length assumed for a pipe that has not been laid out yet.
const UNMEASURED: f32 = 500.0;

pub struct Node {
    /// What moves the pipe into this node, for a sink. Sources have none.
    pub mechanism: Option<Mechanism>,
}

/// Five sources on the left, five sinks on the right, a wire between each pair.
pub fn seed() -> Snarl<Node> {
    // Sinks sit a little above or below their source, so that every wire has a bend.
    const RISE: [f32; 5] = [-24.0, 20.0, -16.0, 24.0, 0.0];

    // Far enough apart that a sink that sits low clears the one below it that sits high.
    // The last pipe bows upward at rest and is given the room to.
    const ROWS: [f32; 5] = [0.0, 116.0, 232.0, 348.0, 560.0];

    let mut snarl = Snarl::new();

    for (row, mechanism) in Mechanism::ALL.into_iter().enumerate() {
        let source = snarl.insert_node(pos2(0.0, ROWS[row]), Node { mechanism: None });
        let sink = snarl.insert_node(
            pos2(520.0, ROWS[row] + RISE[row]),
            Node {
                mechanism: Some(mechanism),
            },
        );

        let _ = snarl.connect(
            OutPinId {
                node: source,
                output: 0,
            },
            InPinId {
                node: sink,
                input: 0,
            },
        );
    }

    snarl
}

/// How wires are painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// As the view paints them when no one asks for anything.
    Plain,

    /// A glowing line with discs travelling along it.
    Beads,

    /// A pipe that moves and the contents moving in it.
    Pipes,
}

/// Which mechanism moves which pipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Assign {
    /// Each pipe by the mechanism of the sink it runs into.
    Each,

    /// Every pipe by the same one.
    All(Mechanism),
}

pub struct Viewer {
    pub look: Look,
    pub assign: Assign,

    /// What acts on the pipes: the signal, gravity, and whatever else is plugged in.
    pub drives: Drives,

    /// Every number the models use.
    pub params: Params,

    /// How a pipe is drawn.
    pub style: pipe::Style,

    pipes: HashMap<WirePins, Pipe>,

    /// Where each pipe lies at rest, as last laid out.
    lays: HashMap<WirePins, Lay>,

    /// Time on the clock the beads of each wire run by. It runs backwards while the wire
    /// is pulled on.
    bead_times: HashMap<WirePins, f32>,

    fit_passes: u8,
}

impl Viewer {
    pub fn new() -> Self {
        Self {
            look: Look::Pipes,
            assign: Assign::Each,
            drives: Drives::default(),
            params: Params::default(),
            style: pipe::Style::default(),
            pipes: HashMap::new(),
            lays: HashMap::new(),
            bead_times: HashMap::new(),
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

    /// Call once per frame, before the graph is shown, with the time since the last frame.
    /// Pass zero to hold everything still.
    pub fn advance(&mut self, snarl: &Snarl<Node>, seconds: f32) {
        self.fit_passes = self.fit_passes.saturating_sub(1);

        let wires: Vec<WirePins> = snarl
            .wires()
            .map(|(out_pin, in_pin)| WirePins::new(out_pin, in_pin))
            .collect();

        self.pipes.retain(|pins, _| wires.contains(pins));
        self.lays.retain(|pins, _| wires.contains(pins));
        self.bead_times.retain(|pins, _| wires.contains(pins));

        let moving = seconds.is_finite() && seconds > 0.0;
        if moving {
            self.drives.advance(seconds);
        }

        for pins in wires {
            let mechanism = self.mechanism(snarl, pins.in_pin.node);
            let lay = self.lay(pins);
            let forcing = self.drives.forcing(&lay);

            let params = &self.params;
            let pipe = self
                .pipes
                .entry(pins)
                .and_modify(|pipe| {
                    if pipe.mechanism() != mechanism {
                        *pipe = Pipe::new(mechanism, params, lay.length);
                    }
                })
                .or_insert_with(|| Pipe::new(mechanism, params, lay.length));

            pipe.advance(params, &forcing, &lay, seconds);

            // The beads of a wire are driven by what drives its pipe.
            if moving {
                let time = self.bead_times.entry(pins).or_insert(0.0);
                *time = forcing
                    .drive(&lay, params.full_head())
                    .mul_add(seconds.min(0.2), *time)
                    .rem_euclid(3600.0);
            }
        }
    }

    /// Where the pipe of `wire` lies, or a level pipe if it has not been laid out yet.
    pub fn lay(&self, wire: WirePins) -> Lay {
        self.lays.get(&wire).copied().unwrap_or(Lay {
            wire: Some(wire),
            ..Lay::level(UNMEASURED)
        })
    }

    /// Everything that acts on the pipe of `wire` from outside, added up.
    pub fn forcing(&self, wire: WirePins) -> Forcing {
        self.drives.forcing(&self.lay(wire))
    }

    /// What moves the pipe into `node`.
    pub fn mechanism(&self, snarl: &Snarl<Node>, node: NodeId) -> Mechanism {
        match self.assign {
            Assign::All(mechanism) => mechanism,
            Assign::Each => snarl[node].mechanism.unwrap_or(Mechanism::Advection),
        }
    }

    pub fn pipe(&self, wire: WirePins) -> Option<&Pipe> {
        self.pipes.get(&wire)
    }

    /// Puts every pipe back to rest.
    pub fn reset(&mut self) {
        self.pipes.clear();
        self.bead_times.clear();
    }
}

impl SnarlViewer<Node> for Viewer {
    fn title(&mut self, node: &Node) -> String {
        match (node.mechanism, self.assign) {
            (None, _) => "Source".to_owned(),
            (Some(_), Assign::All(mechanism)) | (Some(mechanism), Assign::Each) => {
                mechanism.name().to_owned()
            }
        }
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
        usize::from(node.mechanism.is_some())
    }

    fn outputs(&mut self, node: &Node) -> usize {
        usize::from(node.mechanism.is_none())
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        let mechanism = self.mechanism(snarl, pin.id.node);
        ui.small(mechanism.kind());
        PinInfo::circle().with_fill(pipe::shown(pipe::pigments(mechanism)[0], 1.0))
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        ui.small("out");

        // A source takes the colour of the pipe it feeds.
        let fill = pin.remotes.first().map_or(Color32::GRAY, |remote| {
            pipe::shown(pipe::pigments(self.mechanism(snarl, remote.node))[0], 1.0)
        });
        PinInfo::circle().with_fill(fill)
    }

    fn wire_route(&mut self, route: &WireRoute, snarl: &Snarl<Node>) -> Option<CableGeometry> {
        // A wire still being dragged from a pin is no pipe yet.
        let pins = route.pins?;

        match self.look {
            Look::Plain => None,
            Look::Beads => {
                let path = tube::route(route);

                // A view of pipes, if there is one, knows the lay of the pipe better.
                let _ = self.lays.entry(pins).or_insert(Lay {
                    wire: Some(pins),
                    from: route.from,
                    to: route.to,
                    length: path.length(),
                });
                Some(path)
            }
            Look::Pipes => {
                let mechanism = self.mechanism(snarl, pins.in_pin.node);
                let lay = Lay {
                    wire: Some(pins),
                    from: route.from,
                    to: route.to,
                    length: pipe::rest(route, mechanism, &self.params).length(),
                };
                let _ = self.lays.insert(pins, lay);

                let pipe = self.pipes.get(&pins)?;
                Some(pipe::route(
                    route,
                    pipe,
                    &self.params,
                    &self.drives.forcing(&lay),
                    &self.style,
                ))
            }
        }
    }

    fn wire_paint(
        &mut self,
        wire: &WireView<'_>,
        default: WirePaint,
        snarl: &Snarl<Node>,
    ) -> WirePaint {
        let Some(pins) = wire.pins else {
            return default;
        };
        let pigments = pipe::pigments(self.mechanism(snarl, pins.in_pin.node));

        match self.look {
            Look::Plain => default,
            Look::Beads => {
                let time = self.bead_times.get(&pins).copied().unwrap_or(0.0);
                tube::paint(wire, pigments[0], Some(time))
            }
            Look::Pipes => match self.pipes.get(&pins) {
                // Only a path that came from `pipe::route` has walls to fill.
                Some(pipe) if wire.geometry.radii().is_some() => {
                    pipe::paint(wire, pipe, &pigments, &self.style)
                }
                _ => default,
            },
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

/// How the nodes and the canvas look, whatever the wires are shown as.
///
/// It follows the canvas of the application that uses this crate: nodes with their pins
/// on the edge, rows of one height, a header without the arrow that folds a node, and
/// wires that leave a pin level and run behind the nodes. Nothing here is about what a
/// wire does when it is pointed at or clicked.
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
        pin_size: Some(8.0),
        pin_placement: Some(PinPlacement::Edge),
        header_drag_space: Some(Vec2::ZERO),
        collapsible: Some(false),
        ..SnarlStyle::new()
    }
}

/// The style a look is shown with. A plain graph is asked nothing of its wires: it has
/// the canvas and no more.
pub fn style(look: Look) -> SnarlStyle {
    match look {
        Look::Plain => canvas(),
        Look::Beads | Look::Pipes => SnarlStyle {
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

/// What the pointer, at `pointer` on screen, is on in the view that measured `anatomy`,
/// in words.
pub fn readout(
    snarl: &Snarl<Node>,
    viewer: &Viewer,
    anatomy: &SnarlAnatomy,
    pointer: Pos2,
) -> Option<String> {
    let wire = anatomy.hovered_wire()?;
    let hit = wire
        .geometry
        .closest(anatomy.transform.screen_to_graph_pos(pointer));

    let mechanism = viewer.mechanism(snarl, wire.pins.in_pin.node);
    let pipe = viewer.pipe(wire.pins)?;

    let lay = viewer.lay(wire.pins);
    let forcing = viewer.forcing(wire.pins);
    let full_head = viewer.params.full_head();

    let mut words = format!(
        "Pointer on {}: {:.0}% along, {:.1} from the centreline, wall at {:.1}; \
         cross-section x{:.2}, contents moving at {:.0}; the pipe holds x{:.2} of what it \
         holds at rest. Driven at {:+.2} by {}: pump {:+.2}, head {:+.2}, valve {:.2}",
        mechanism.name(),
        100.0 * hit.share,
        hit.distance,
        wire.geometry.radius_at(hit.share),
        pipe.area_at(hit.share),
        pipe.speed_at(hit.share),
        pipe.volume(1.0),
        pipe.drive(),
        viewer.drives.active().join(" and "),
        forcing.push,
        forcing.head(&lay) / full_head,
        forcing.opening,
    );

    match mechanism {
        Mechanism::Balloon => {
            let (gives_way, gives_back) = viewer.params.balloon.limits();
            let _ = write!(
                words,
                "; soft stretch filled to {:.2} (gives way at {gives_way:.1}, gives back at \
                 {gives_back:.1})",
                pipe.balloon(),
            );
        }
        Mechanism::Bourdon => {
            let _ = write!(words, "; straightened by {:.2}", pipe.straightening());
        }
        _ => {}
    }

    Some(words)
}
