//! The patch the conduits run through, what each connection carries, and the signals
//! that run in it.
//!
//! What a connection carries follows `renderlab-native` at
//! `265515a1a37ab949bdc6b049636b0cb31957fb44`, `crates/renderlab-dx12/src/patchbay.rs`:
//! [`target_recipe`], [`carried`] and [`carry`] are its functions of those names, over a
//! [`Snarl`] in place of the product's graph document, and [`seed`] is its `example`.
//!
//! Two things here are stand-ins, because the product's own are not in this crate and
//! are not its business:
//!
//! - [`Light`]. The product gets the colour a paint shows, and the light it gives off
//!   under its lamps, from a spectral model. This one mixes eight made-up colours.
//!   No colour in this example is evidence of what the product shows.
//! - [`Signals`]. The product runs its graph in a session and reads what each edge
//!   carried. This one makes the same shape of patch move: two oscillators, the second
//!   bending the rate of the first, and a pen pressure eased by a slew.

use std::collections::HashMap;

use egui::pos2;
use egui_snarl::{InPinId, NodeId, OutPinId, Snarl, ui::WirePins};

use crate::recipe::{Carry, Conduit, Stream};

/// The brush controls a patch can drive, by key and by name. The first eight are the
/// loads of the eight pigments.
pub const TARGETS: [(&str, &str); 14] = [
    ("paint.rose", "Rose"),
    ("paint.blue", "Blue"),
    ("paint.yellow", "Yellow"),
    ("paint.uv_blue", "UV Blue"),
    ("paint.uv_cerise", "UV Cerise"),
    ("paint.uv_orange", "UV Orange"),
    ("paint.uv_lemon", "UV Lemon"),
    ("paint.uv_green", "UV Green"),
    ("brush.size", "Size"),
    ("brush.load", "Load"),
    ("finish.gloss", "Gloss"),
    ("finish.metal", "Metal"),
    ("finish.interference", "Interference"),
    ("finish.film", "Film"),
];

/// The colour of the pen, and of anything that carries no paint and is no module.
pub const PEN_COLOUR: [f32; 3] = [0.96, 0.93, 0.98];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Lfo,
    Slew,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A measure of the pen.
    Source,

    /// A module of the rack.
    Operator(Operator),

    /// A brush control, by its key in [`TARGETS`].
    Target(&'static str),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    pub label: String,

    /// The names of its inputs.
    pub inputs: &'static [&'static str],
}

impl Node {
    pub const fn outputs(&self) -> usize {
        match self.kind {
            Kind::Source | Kind::Operator(_) => 1,
            Kind::Target(_) => 0,
        }
    }
}

/// The example patch: two LFOs, the second bending the first's rate and driving UV
/// Lemon while the first drives UV Cerise and Gloss, and the pen's pressure through a
/// slew into Size.
pub fn seed() -> Snarl<Node> {
    let mut snarl = Snarl::new();
    let mut add = |kind: Kind, label: &str, inputs: &'static [&'static str], x: f32, y: f32| {
        snarl.insert_node(
            pos2(x, y),
            Node {
                kind,
                label: label.to_owned(),
                inputs,
            },
        )
    };
    let target = |key: &'static str| {
        let (_, label) = TARGETS
            .iter()
            .find(|(have, _)| *have == key)
            .expect("a key of the table");
        (Kind::Target(key), *label)
    };

    let lfo_1 = add(
        Kind::Operator(Operator::Lfo),
        "LFO 1",
        &["rate"],
        240.0,
        40.0,
    );
    let lfo_2 = add(
        Kind::Operator(Operator::Lfo),
        "LFO 2",
        &["rate"],
        20.0,
        150.0,
    );
    let slew = add(
        Kind::Operator(Operator::Slew),
        "Slew",
        &["signal"],
        240.0,
        400.0,
    );
    let pen = add(Kind::Source, "Pen Pressure", &[], 20.0, 400.0);

    let mut targets = Vec::new();
    for (row, key) in [
        "paint.uv_cerise",
        "finish.gloss",
        "paint.uv_lemon",
        "brush.size",
    ]
    .into_iter()
    .enumerate()
    {
        let (kind, label) = target(key);
        #[allow(clippy::cast_precision_loss)]
        targets.push(add(kind, label, &["in"], 480.0, 10.0 + 125.0 * row as f32));
    }

    let mut connect = |from: NodeId, to: NodeId| {
        let _ = snarl.connect(
            OutPinId {
                node: from,
                output: 0,
            },
            InPinId { node: to, input: 0 },
        );
    };
    connect(lfo_1, targets[0]);
    connect(lfo_2, lfo_1);
    connect(lfo_2, targets[2]);
    connect(pen, slew);
    connect(slew, targets[3]);
    connect(lfo_1, targets[1]);
    snarl
}

/// The paint a brush control target hands on: what the brush carries for size, load and
/// the finishes; the one pigment for a pigment's load.
pub fn target_recipe(key: &str, brush: [f32; 8]) -> [f32; 8] {
    match TARGETS.iter().position(|(have, _)| *have == key) {
        Some(index) if index < 8 => {
            let mut recipe = [0.0; 8];
            recipe[index] = 1.0;
            recipe
        }
        _ => brush,
    }
}

/// What each node of the patch carries on toward the brush: the paint loads of every
/// target it ends up driving, through any modules between, summed. A node that drives
/// no target carries nothing.
pub fn carried(snarl: &Snarl<Node>, brush: [f32; 8]) -> HashMap<NodeId, [f32; 8]> {
    let mut carried: HashMap<NodeId, [f32; 8]> = snarl
        .node_ids()
        .filter_map(|(id, node)| match node.kind {
            Kind::Target(key) => Some((id, target_recipe(key, brush))),
            _ => None,
        })
        .collect();
    let edges: Vec<(NodeId, NodeId)> = snarl
        .wires()
        .map(|(out_pin, in_pin)| (out_pin.node, in_pin.node))
        .collect();
    // Back from the targets one hop per pass; a patch is at most as deep as it has
    // nodes, and a loop stops there.
    for _ in 0..snarl.node_ids().count() {
        let mut next = carried.clone();
        for (id, node) in snarl.node_ids() {
            if matches!(node.kind, Kind::Target(_)) {
                continue;
            }
            let mut sum = [0.0_f32; 8];
            for (_, target) in edges.iter().filter(|(source, _)| *source == id) {
                if let Some(down) = carried.get(target) {
                    for (total, load) in sum.iter_mut().zip(down) {
                        *total += load;
                    }
                }
            }
            if sum.iter().any(|load| *load > 0.0) {
                let _ = next.insert(id, sum);
            }
        }
        if next == carried {
            break;
        }
        carried = next;
    }
    carried
}

/// What the connection from `source` into `target` carries under `light` with `brush`
/// on the brush.
pub fn carry(
    snarl: &Snarl<Node>,
    source: NodeId,
    target: NodeId,
    light: Light,
    brush: [f32; 8],
) -> Carry {
    let carried = carried(snarl, brush);
    let accent = |node: NodeId| {
        snarl
            .node_ids()
            .filter(|(_, node)| matches!(node.kind, Kind::Operator(_)))
            .position(|(id, _)| id == node)
            .map_or(PEN_COLOUR, |index| {
                carried
                    .get(&node)
                    .map_or_else(|| module_colour(index), |recipe| light.colour(*recipe))
            })
    };
    let conduit = match snarl.get_node(target).map(|node| node.kind) {
        Some(Kind::Target(_)) => Conduit::Pipe,
        _ => Conduit::Hose,
    };
    let recipe = carried.get(&target).copied();
    let into = recipe.map_or_else(|| accent(target), |recipe| light.colour(recipe));
    let from = carried
        .get(&source)
        .map_or_else(|| accent(source), |recipe| light.colour(*recipe));
    let (emitted, glow) = recipe.map_or(([0.0; 3], 0.0), |recipe| {
        (light.emission(recipe), light.glow(recipe))
    });
    Carry {
        conduit,
        recipe,
        from,
        into,
        emitted,
        glow,
    }
}

/// The colour of a module that carries no paint. A stand-in for the product's palette.
fn module_colour(index: usize) -> [f32; 3] {
    const SWEEP: [[f32; 3]; 4] = [
        [0.35, 0.62, 0.95],
        [0.95, 0.55, 0.3],
        [0.5, 0.85, 0.55],
        [0.8, 0.5, 0.9],
    ];
    SWEEP[index % SWEEP.len()]
}

/// A stand-in for the light the product sees its paints in.
///
/// It answers the three questions the recipe asks of the product's `Light`, with
/// numbers made up for this example: eight colours mixed by their loads, and for the
/// five fluorescent ones a light of their own that grows with the ultraviolet lamp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// How bright the ultraviolet lamp is, from off (zero) to full (one).
    pub uv: f32,
}

/// Made up: what each pigment shows, and what the fluorescent ones give off.
const SHOWN: [[f32; 3]; 8] = [
    [0.80, 0.12, 0.22],
    [0.10, 0.22, 0.75],
    [0.92, 0.78, 0.10],
    [0.20, 0.35, 0.95],
    [0.95, 0.12, 0.55],
    [1.00, 0.42, 0.08],
    [0.85, 0.95, 0.15],
    [0.20, 0.90, 0.30],
];
const GIVEN_OFF: [[f32; 3]; 8] = [
    [0.0; 3],
    [0.0; 3],
    [0.0; 3],
    [0.25, 0.45, 1.00],
    [1.00, 0.20, 0.70],
    [1.00, 0.50, 0.10],
    [0.80, 1.00, 0.20],
    [0.25, 1.00, 0.40],
];

impl Light {
    fn mix(recipe: [f32; 8], colours: &[[f32; 3]; 8]) -> [f32; 3] {
        let total: f32 = recipe.iter().sum::<f32>().max(1.0e-3);
        std::array::from_fn(|c| {
            recipe
                .iter()
                .zip(colours)
                .map(|(load, colour)| load * colour[c])
                .sum::<f32>()
                / total
        })
    }

    /// The colour `recipe` shows.
    pub fn colour(self, recipe: [f32; 8]) -> [f32; 3] {
        Self::mix(recipe, &SHOWN)
    }

    /// The light `recipe` gives off of its own.
    pub fn emission(self, recipe: [f32; 8]) -> [f32; 3] {
        Self::mix(recipe, &GIVEN_OFF).map(|c| c * self.uv * 0.5)
    }

    /// How brightly `recipe` gives off its own light, from none (zero) up: the
    /// luminance of [`Light::emission`] over that of the colour it shows. This much is
    /// the product's arithmetic.
    pub fn glow(self, recipe: [f32; 8]) -> f32 {
        let luma = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let own = luma(self.emission(recipe));
        let shown = luma(self.colour(recipe)).max(1.0e-3);
        (own / shown).clamp(0.0, 2.0) * own.min(1.0).sqrt() * 2.0
    }
}

/// A stand-in for the product's graph session: what each node puts out, and so what
/// each wire carries.
#[derive(Clone, Debug)]
pub struct Signals {
    /// The pen's pressure, from none (zero) to full (one).
    pub pressure: f32,

    /// How fast the oscillators turn, in turns a second, before anything bends them.
    pub rate: f32,

    turn: HashMap<NodeId, f32>,
    out: HashMap<NodeId, f32>,

    /// What each wire carries, as the recipe draws it.
    pub streams: HashMap<WirePins, Stream>,
}

impl Default for Signals {
    fn default() -> Self {
        Self {
            pressure: 0.7,
            rate: 0.22,
            turn: HashMap::new(),
            out: HashMap::new(),
            streams: HashMap::new(),
        }
    }
}

impl Signals {
    /// What `node` put out last.
    pub fn output(&self, node: NodeId) -> f32 {
        self.out.get(&node).copied().unwrap_or(0.0)
    }

    pub fn stream(&self, wire: WirePins) -> Stream {
        self.streams.get(&wire).copied().unwrap_or_default()
    }

    /// Runs the patch for `seconds`, then lets every wire follow what its source put
    /// out, as the product does after each hop of its graph.
    pub fn advance(&mut self, snarl: &Snarl<Node>, seconds: f32) {
        let seconds = seconds.max(0.0);
        let edges: Vec<WirePins> = snarl
            .wires()
            .map(|(out_pin, in_pin)| WirePins::new(out_pin, in_pin))
            .collect();

        let before = self.out.clone();
        for (id, node) in snarl.node_ids() {
            let input = edges
                .iter()
                .filter(|wire| wire.in_pin.node == id)
                .map(|wire| before.get(&wire.out_pin.node).copied().unwrap_or(0.0))
                .sum::<f32>();
            let out = match node.kind {
                Kind::Source => self.pressure,
                Kind::Operator(Operator::Lfo) => {
                    let turn = self.turn.entry(id).or_insert(0.0);
                    *turn = (*turn + seconds * self.rate * input.mul_add(0.8, 1.0)).fract();
                    (std::f32::consts::TAU * *turn).sin()
                }
                Kind::Operator(Operator::Slew) => {
                    let last = before.get(&id).copied().unwrap_or(0.0);
                    last + (input - last) * (1.0 - (-seconds * 3.0).exp())
                }
                Kind::Target(_) => input,
            };
            let _ = self.out.insert(id, out);
        }

        for wire in &edges {
            let signal = self.output(wire.out_pin.node);
            self.streams
                .entry(*wire)
                .or_default()
                .follow(signal, seconds);
        }
        self.streams.retain(|wire, _| edges.contains(wire));
    }
}
