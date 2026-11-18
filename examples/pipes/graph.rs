//! The graph the pipes run through, and the viewer that gives its wires one of three looks.
//!
//! The graph is five sources, five sinks and a wire from each source to its sink. The same
//! graph is shown twice, once with travelling beads and once with pipes that move, so each
//! wire can be compared with itself.
//!
//! The viewer changes nothing but wires, and only through `wire_route` and `wire_paint`.
//! With [`Look::Plain`] it returns what the view offered, and the graph is an ordinary one.

use std::{collections::HashMap, fmt::Write as _};

use egui::{Color32, Id, Pos2, Ui, pos2};
use egui_snarl::{
    InPin, InPinId, NodeId, OutPin, OutPinId, Snarl,
    ui::{
        CableGeometry, PinInfo, SnarlAnatomy, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget,
        WirePaint, WirePins, WireRoute, WireView,
    },
};

use super::{
    flow::{BALLOON_LIMITS, Mechanism, Pipe},
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
    const RISE: [f32; 5] = [-26.0, 24.0, -20.0, 26.0, 0.0];

    // The last pipe bows upward at rest and is given the room to.
    const ROWS: [f32; 5] = [0.0, 104.0, 208.0, 312.0, 520.0];

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

    /// How hard the sources push, in `-1.0..=1.0`. Negative pulls.
    pub command: f32,

    pipes: HashMap<WirePins, Pipe>,

    /// Length of each pipe at rest, as last laid out.
    lengths: HashMap<WirePins, f32>,

    /// Time on the clock the beads run by. It runs backwards under a negative command.
    bead_time: f32,

    fit_passes: u8,
}

impl Viewer {
    pub fn new() -> Self {
        Self {
            look: Look::Pipes,
            assign: Assign::Each,
            command: 0.8,
            pipes: HashMap::new(),
            lengths: HashMap::new(),
            bead_time: 0.0,
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
        self.lengths.retain(|pins, _| wires.contains(pins));

        for pins in wires {
            let mechanism = self.mechanism(snarl, pins.in_pin.node);
            let length = self.lengths.get(&pins).copied().unwrap_or(UNMEASURED);

            let pipe = self
                .pipes
                .entry(pins)
                .and_modify(|pipe| {
                    if pipe.mechanism() != mechanism {
                        *pipe = Pipe::new(mechanism, length);
                    }
                })
                .or_insert_with(|| Pipe::new(mechanism, length));

            pipe.advance(self.command, length, seconds);
        }

        if seconds.is_finite() && seconds > 0.0 {
            self.bead_time = self
                .command
                .mul_add(seconds.min(0.2), self.bead_time)
                .rem_euclid(3600.0);
        }
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
        self.bead_time = 0.0;
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
            Look::Beads => Some(tube::route(route)),
            Look::Pipes => {
                let mechanism = self.mechanism(snarl, pins.in_pin.node);
                let _ = self
                    .lengths
                    .insert(pins, pipe::rest(route, mechanism).length());

                let pipe = self.pipes.get(&pins)?;
                Some(pipe::route(route, pipe))
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
            Look::Beads => tube::paint(wire, pigments[0], Some(self.bead_time)),
            Look::Pipes => match self.pipes.get(&pins) {
                // Only a path that came from `pipe::route` has walls to fill.
                Some(pipe) if wire.geometry.radii().is_some() => pipe::paint(wire, pipe, &pigments),
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

/// The style a look is shown with. A plain graph is given no style of its own at all.
pub fn style(look: Look) -> SnarlStyle {
    match look {
        Look::Plain => SnarlStyle::new(),
        Look::Beads | Look::Pipes => SnarlStyle {
            wire_picking: Some(true),
            nodes_cover_wires: Some(true),
            ..SnarlStyle::new()
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

    let mut words = format!(
        "Pointer on {}: {:.0}% along, {:.1} from the centreline, wall at {:.1}; \
         cross-section x{:.2}, contents moving at {:.0}; the pipe holds x{:.2} of what it \
         holds at rest",
        mechanism.name(),
        100.0 * hit.share,
        hit.distance,
        wire.geometry.radius_at(hit.share),
        pipe.area_at(hit.share),
        pipe.speed_at(hit.share),
        pipe.volume(1.0),
    );

    match mechanism {
        Mechanism::Balloon => {
            let _ = write!(
                words,
                "; soft stretch filled to {:.2} (gives way at {:.1}, gives back at {:.1})",
                pipe.balloon(),
                BALLOON_LIMITS.0,
                BALLOON_LIMITS.1,
            );
        }
        Mechanism::Bourdon => {
            let _ = write!(words, "; straightened by {:.2}", pipe.straightening());
        }
        _ => {}
    }

    Some(words)
}
