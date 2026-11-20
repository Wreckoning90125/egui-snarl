//! Checks the RenderLab pipe and hose recipe of the `conduits` example against the source
//! it was taken from, and runs its patch through headless egui passes to check that each
//! connection is drawn where it is picked.
//!
//! The source is `renderlab-native` at `265515a1a37ab949bdc6b049636b0cb31957fb44`. Tests
//! named as a test of that source are that test, over this crate's graph in place of the
//! product's document. [`reference`] is the body of its `conduit`, kept apart from the
//! example's so that neither can drift unnoticed.
//!
//! No test here says anything about colour: the example's light is a stand-in.

#[allow(dead_code)]
#[path = "../examples/conduits/graph.rs"]
mod graph;
#[allow(dead_code)]
#[path = "../examples/conduits/patch.rs"]
mod patch;
#[allow(dead_code)]
#[path = "../examples/conduits/recipe.rs"]
mod recipe;

use std::sync::Arc;

use egui::{
    Color32, Context, Event, FullOutput, Id, Mesh, PointerButton, Pos2, RawInput, Rect, Shape,
    Stroke,
};
use egui_snarl::{
    InPinId, NodeId, OutPinId, Snarl,
    ui::{
        CableGeometry, SnarlAnatomy, WireLayer, WirePaint, WirePins, get_selected_wire,
        get_snarl_anatomy,
    },
};
use graph::{Look, Viewer};
use patch::{Kind, Light, Node, Signals, carried, carry, target_recipe};
use recipe::{ACROSS, BRASS, Carry, Conduit, PipeView, STEEL, Stream, conduit, shown, socket};

const ROSE: [f32; 8] = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

fn edges(snarl: &Snarl<Node>) -> Vec<WirePins> {
    let mut wires: Vec<WirePins> = snarl
        .wires()
        .map(|(out_pin, in_pin)| WirePins::new(out_pin, in_pin))
        .collect();
    wires.sort();
    wires
}

fn node(snarl: &Snarl<Node>, label: &str) -> NodeId {
    snarl
        .node_ids()
        .find(|(_, node)| node.label == label)
        .map(|(id, _)| id)
        .expect("the patch has the node")
}

fn is_module(snarl: &Snarl<Node>, id: NodeId) -> bool {
    matches!(snarl[id].kind, Kind::Operator(_))
}

/// `patchbay.rs`, `a_pipe_runs_as_full_and_as_fast_as_its_signal`.
#[test]
fn a_pipe_runs_as_full_and_as_fast_as_its_signal() {
    let mut strong = Stream::default();
    let mut weak = Stream::default();
    let mut back = Stream::default();
    for _ in 0..60 {
        strong.follow(0.9, 1.0 / 60.0);
        weak.follow(0.1, 1.0 / 60.0);
        back.follow(-0.9, 1.0 / 60.0);
    }
    assert!((strong.level - 0.9).abs() < 0.01, "{strong:?}");
    assert!((weak.level - 0.1).abs() < 0.01, "{weak:?}");
    assert!(
        strong.phase > weak.phase * 4.0,
        "a strong signal runs faster"
    );
    // A negative signal draws the paint back toward its source.
    assert!(back.phase > 60.0, "{back:?}");
    assert!((back.phase - (64.0 - strong.phase)).abs() < 1.0e-3);
}

/// `patchbay.rs`, `colour_flows_back_from_the_paints_a_patch_drives`.
#[test]
fn colour_flows_back_from_the_paints_a_patch_drives() {
    let mut snarl = Snarl::new();
    let mut add = |kind, label: &str, inputs| {
        snarl.insert_node(
            Pos2::ZERO,
            Node {
                kind,
                label: label.to_owned(),
                inputs,
            },
        )
    };
    let first = add(Kind::Operator(patch::Operator::Lfo), "first", &["rate"]);
    let second = add(Kind::Operator(patch::Operator::Lfo), "second", &["rate"]);
    let lemon_target = add(Kind::Target("paint.uv_lemon"), "UV Lemon", &["in"]);
    let size_target = add(Kind::Target("brush.size"), "Size", &["in"]);
    for (from, to) in [
        (first, lemon_target),
        (second, first),
        (second, size_target),
    ] {
        assert!(snarl.connect(
            OutPinId {
                node: from,
                output: 0
            },
            InPinId { node: to, input: 0 }
        ));
    }

    let flow = carried(&snarl, ROSE);
    // The first oscillator carries lemon on; the second carries lemon through the first
    // and the brush's rose through Size.
    let lemon = target_recipe("paint.uv_lemon", ROSE);
    assert_eq!(flow.get(&first), Some(&lemon));
    let both: [f32; 8] = std::array::from_fn(|i| lemon[i] + ROSE[i]);
    assert_eq!(flow.get(&second), Some(&both));

    let light = Light { uv: 1.0 };
    for wire in edges(&snarl) {
        let carry = carry(&snarl, wire.out_pin.node, wire.in_pin.node, light, ROSE);
        assert_eq!(
            carry.conduit == Conduit::Hose,
            is_module(&snarl, wire.in_pin.node),
            "signals into modules run in hoses, paint into the brush in pipes"
        );
        let recipe = carry.recipe.expect("every edge here moves paint");
        assert_eq!(carry.into, light.colour(recipe));
    }
    // Under the lamp lemon gives off its own light; rose does not.
    assert!(light.glow(lemon) > 0.3);
    assert!(light.glow(ROSE) < 1.0e-3);
}

/// `paint_graph.rs`, `each_pin_is_the_socket_of_what_plugs_into_it`.
#[test]
fn each_pin_is_the_socket_of_what_plugs_into_it() {
    let snarl = patch::seed();
    let light = Light { uv: 0.0 };
    let brass = shown(BRASS, 1.0);
    let ends = |on_pin: &dyn Fn(&WirePins) -> bool| -> Vec<Carry> {
        edges(&snarl)
            .into_iter()
            .filter(|wire| on_pin(wire))
            .map(|wire| carry(&snarl, wire.out_pin.node, wire.in_pin.node, light, ROSE))
            .collect()
    };

    let lemon = node(&snarl, "UV Lemon");
    let face = socket(&ends(&|wire| wire.in_pin.node == lemon), true);
    assert_eq!(face.rim, brass, "a paint pipe ends in brass");
    assert_eq!(
        face.bore,
        shown(light.colour(target_recipe("paint.uv_lemon", ROSE)), 1.0),
        "its bore shows the paint flowing in"
    );

    let module_input = edges(&snarl)
        .into_iter()
        .find(|wire| is_module(&snarl, wire.in_pin.node))
        .expect("the example bends a rate");
    let face = socket(&ends(&|wire| wire.in_pin == module_input.in_pin), true);
    assert_ne!(face.rim, brass, "a signal hose ends in steel");
    assert_eq!(face.rim, shown(STEEL, 1.0));

    // Nothing plugged in: steel, and a dark bore.
    let face = socket(&[], false);
    assert_eq!(face.rim, shown(STEEL, 1.0));
    assert_eq!(face.bore, Color32::from_gray(10));
}

/// `paint_graph.rs`, `each_wire_is_a_pipe_or_a_hose_of_what_it_carries`.
#[test]
fn each_wire_is_a_pipe_or_a_hose_of_what_it_carries() {
    let snarl = patch::seed();
    let light = Light { uv: 1.0 };
    let view = PipeView {
        lit: false,
        raised: false,
        scale: 1.0,
        length: 230.0,
        hit: 2.0,
    };
    let (mut pipes, mut hoses) = (0, 0);
    for wire in edges(&snarl) {
        let carried = carry(&snarl, wire.out_pin.node, wire.in_pin.node, light, ROSE);
        let paint = conduit(carried, view, Stream::default());
        match carried.conduit {
            Conduit::Pipe => {
                pipes += 1;
                // Halo, rim, wall, paint, core and streak.
                assert_eq!(paint.strokes().len(), 8);
                let recipe = carried.recipe.expect("a pipe moves paint");
                assert_eq!(carried.into, light.colour(recipe));
                assert_eq!(paint.strokes()[6].color, shown(carried.into, 1.0));
            }
            Conduit::Hose => hoses += 1,
        }
        // Five slugs along 230 units; the pins are the fittings.
        assert_eq!(paint.beads().len(), 5);
        assert!(paint.hit_width() >= 13.0);
        assert_eq!(paint.layer(), None);
    }
    assert!(pipes > 0 && hoses > 0, "the example patch has both");
}

/// `paint_graph.rs`, `a_running_patch_fills_its_pipes`, with the example's signals in
/// place of the product's graph session.
#[test]
fn a_running_patch_fills_its_pipes() {
    let snarl = patch::seed();
    let mut signals = Signals::default();
    for _ in 0..60 {
        signals.advance(&snarl, 1.0 / 60.0);
    }
    let levels: Vec<f32> = signals.streams.values().map(|s| s.level).collect();
    assert_eq!(levels.len(), snarl.wires().count());
    assert!(levels.iter().any(|level| *level > 0.1), "{levels:?}");
}

/// The body of `conduit` in `paint_graph.rs` at the pinned commit, with the product's
/// names for this crate's types.
fn reference(carry: Carry, view: PipeView, stream: Stream) -> WirePaint {
    use WireLayer as GraphWireLayer;
    use egui_snarl::ui::WirePaint;

    let rgb = |c: [f32; 3], alpha: f32| shown(c, alpha);
    let scaled = |c: [f32; 3], k: f32| c.map(|v| (v * k).min(1.0));
    let lit = if view.lit { 1.0 } else { 0.0 };
    let radius = 6.5_f32.max(4.5 / view.scale.max(0.05));
    let into = carry.into;
    let mut paint = match carry.conduit {
        Conduit::Pipe => {
            let wall: [f32; 3] =
                std::array::from_fn(|c| carry.from[c].mul_add(0.18, into[c] * 0.12) + 0.06);
            let peak = carry.emitted.iter().fold(1.0e-3_f32, |a, c| a.max(*c));
            let share = (carry.glow * stream.level.mul_add(0.7, 0.3) / 1.5).clamp(0.0, 1.0);
            let halo: [f32; 3] =
                std::array::from_fn(|c| into[c] + (carry.emitted[c] / peak - into[c]) * share);
            WirePaint::solid(Stroke::new(
                radius * 2.0,
                rgb(scaled(wall, 1.6 + 0.3 * lit), 0.9),
            ))
            .over(Stroke::new(radius * 1.8, rgb(wall, 1.0)))
            .over(Stroke::new(radius * 1.28, rgb(scaled(into, 0.85), 1.0)))
            .over(Stroke::new(radius * 0.8, rgb(into, 1.0)))
            .over(Stroke::new(
                radius * 0.18,
                Color32::from_white_alpha(if view.lit { 150 } else { 95 }),
            ))
            .with_glow(
                rgb(halo, 0.25 + 0.5 * share),
                radius * (0.8 + 1.6 * share + 0.4 * lit),
            )
        }
        Conduit::Hose => {
            let rubber = [0.085, 0.085, 0.095];
            let mut hose = WirePaint::solid(Stroke::new(
                radius * 1.7,
                rgb(scaled(rubber, 2.2 + 0.8 * lit), 1.0),
            ))
            .over(Stroke::new(radius * 1.45, rgb(rubber, 1.0)));
            if carry.recipe.is_some() {
                hose = hose
                    .over(Stroke::new(radius * 0.5, rgb(scaled(into, 0.8), 1.0)))
                    .over(Stroke::new(radius * 0.12, Color32::from_white_alpha(70)));
            }
            hose
        }
    }
    .with_hit_width(view.hit.max(radius * 2.0));
    if carry.recipe.is_some() {
        let length = view.length.max(1.0);
        let spacing = 46.0 / length;
        let waves = (length / 46.0).ceil().clamp(1.0, 24.0);
        let offset = (stream.phase * spacing).rem_euclid(1.0);
        let size = match carry.conduit {
            Conduit::Pipe => radius * stream.level.mul_add(0.4, 0.22),
            Conduit::Hose => radius * stream.level.mul_add(0.22, 0.12),
        };
        let mut wave = 0.0;
        while wave < waves {
            let share = wave.mul_add(spacing, offset).fract();
            paint = paint.with_bead(share, size, rgb(scaled(into, 1.15), 0.9));
            wave += 1.0;
        }
    }
    if view.raised {
        paint = paint.with_layer(GraphWireLayer::AboveNodes);
    }
    paint
}

/// What a paint lays, to compare two of them by.
fn laid(
    paint: &WirePaint,
) -> (
    Vec<Stroke>,
    Vec<(u32, u32, Color32)>,
    u32,
    Option<WireLayer>,
) {
    (
        paint.strokes().to_vec(),
        paint
            .beads()
            .iter()
            .map(|bead| (bead.share.to_bits(), bead.radius.to_bits(), bead.fill))
            .collect(),
        paint.hit_width().to_bits(),
        paint.layer(),
    )
}

#[test]
fn the_drawer_recipe_lays_what_the_source_lays() {
    let paint = Carry {
        conduit: Conduit::Pipe,
        recipe: Some(ROSE),
        from: [0.3, 0.6, 0.9],
        into: [0.9, 0.2, 0.5],
        emitted: [0.8, 0.1, 0.6],
        glow: 0.9,
    };
    let carries = [
        paint,
        Carry {
            emitted: [0.0; 3],
            glow: 0.0,
            ..paint
        },
        Carry {
            conduit: Conduit::Hose,
            ..paint
        },
        Carry {
            conduit: Conduit::Hose,
            recipe: None,
            ..paint
        },
    ];

    let mut compared = 0;
    for carry in carries {
        for (lit, raised) in [(false, false), (true, false), (true, true)] {
            for scale in [0.2, 0.5, 1.0, 2.5] {
                for length in [0.5, 40.0, 230.0, 2000.0] {
                    for signal in [-0.9, 0.0, 0.3, 1.0] {
                        let mut stream = Stream::default();
                        for _ in 0..90 {
                            stream.follow(signal, 1.0 / 60.0);
                        }
                        let view = PipeView {
                            lit,
                            raised,
                            scale,
                            length,
                            hit: 9.0,
                        };
                        assert_eq!(
                            laid(&conduit(carry, view, stream)),
                            laid(&reference(carry, view, stream)),
                            "{carry:?} lit {lit} raised {raised} scale {scale} length {length}"
                        );
                        compared += 1;
                    }
                }
            }
        }
    }
    assert_eq!(compared, 4 * 3 * 4 * 4 * 4);

    // What the comparison stands on: the looks differ where the source says they do.
    let view = PipeView {
        lit: false,
        raised: false,
        scale: 1.0,
        length: 230.0,
        hit: 2.0,
    };
    let stream = Stream::default();
    let pipe = conduit(carries[0], view, stream);
    let dry_hose = conduit(carries[3], view, stream);
    assert_eq!(dry_hose.strokes().len(), 2, "rubber, and no window");
    assert!(dry_hose.beads().is_empty(), "nothing runs in it");
    assert_eq!(pipe.strokes().last().unwrap().width, 6.5 * 0.18);
    // Zoomed out, a connection keeps about nine points across.
    let far = conduit(
        carries[0],
        PipeView {
            scale: 0.25,
            ..view
        },
        stream,
    );
    assert_eq!(far.hit_width(), 2.0 * 4.5 / 0.25);
    // A selected one is raised; one under the pointer is lit and no more.
    assert_eq!(
        conduit(
            carries[0],
            PipeView {
                raised: true,
                ..view
            },
            stream
        )
        .layer(),
        Some(WireLayer::AboveNodes)
    );
    let lit = conduit(carries[0], PipeView { lit: true, ..view }, stream);
    assert_eq!(lit.layer(), None);
    assert_ne!(lit.strokes(), pipe.strokes());
    // A stream that has run moves the slugs and swells them.
    let mut run = Stream::default();
    for _ in 0..30 {
        run.follow(1.0, 1.0 / 60.0);
    }
    let running = conduit(carries[0], view, run);
    assert!(running.beads()[0].radius > pipe.beads()[0].radius);
    assert!(running.beads()[0].share > pipe.beads()[0].share);
}

// The patch in a view.

const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1280.0, 720.0));

fn view() -> Id {
    Id::unique("conduits")
}

struct World {
    ctx: Context,
    time: f64,
    snarl: Snarl<Node>,
    viewer: Viewer,
}

impl World {
    fn new(look: Look) -> Self {
        let mut viewer = Viewer::new();
        viewer.look = look;
        viewer.fit();

        let mut world = Self {
            ctx: Context::default(),
            time: 0.0,
            snarl: patch::seed(),
            viewer,
        };
        for _ in 0..10 {
            let _ = world.pass(0.0, Vec::new());
        }
        world
    }

    /// One frame: the signals run on by `seconds`, then the graph is shown.
    fn pass(&mut self, seconds: f32, events: Vec<Event>) -> FullOutput {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(SCREEN),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            focused: true,
            events,
            ..RawInput::default()
        };

        let Self {
            ctx, snarl, viewer, ..
        } = self;
        let look = viewer.look;
        viewer.advance(snarl, seconds);

        let mut output = ctx.run_ui(input, |ui| {
            graph::show(snarl, viewer, look, view(), ui);
        });
        output.textures_delta.clear();
        output
    }

    fn run(&mut self, seconds: f32) -> FullOutput {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames = (seconds * 60.0).round() as usize;
        for _ in 1..frames {
            let _ = self.pass(1.0 / 60.0, Vec::new());
        }
        self.pass(1.0 / 60.0, Vec::new())
    }

    /// Moves the pointer without letting the signals run on.
    fn point_at(&mut self, pos: Pos2) -> FullOutput {
        let _ = self.pass(0.0, vec![Event::PointerMoved(pos)]);
        self.pass(0.0, Vec::new())
    }

    fn click(&mut self, pos: Pos2, button: PointerButton) -> FullOutput {
        let _ = self.point_at(pos);
        for pressed in [true, false] {
            let _ = self.pass(
                0.0,
                vec![Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        self.pass(0.0, Vec::new())
    }

    fn anatomy(&self) -> Arc<SnarlAnatomy> {
        get_snarl_anatomy(view(), &self.ctx).expect("the view has been shown")
    }

    /// A wire as it lies on the screen.
    fn on_screen(&self, wire: WirePins) -> CableGeometry {
        let anatomy = self.anatomy();
        anatomy
            .wire(wire)
            .expect("the wire was measured")
            .geometry
            .transformed(anatomy.transform.to_global)
    }

    fn scale(&self) -> f32 {
        self.anatomy().transform.to_global.scaling
    }

    fn carry(&self, wire: WirePins) -> Carry {
        self.viewer.carry(wire).expect("the wire carries something")
    }

    /// The first wire of the patch that is a pipe, or a hose.
    fn first(&self, conduit: Conduit) -> WirePins {
        edges(&self.snarl)
            .into_iter()
            .find(|wire| self.carry(*wire).conduit == conduit)
            .expect("the patch has both")
    }
}

/// Every shape of a pass, in the order they are painted.
fn painted(output: &FullOutput) -> Vec<Shape> {
    fn take_apart(shape: &Shape, shapes: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(group) => {
                for shape in group {
                    take_apart(shape, shapes);
                }
            }
            Shape::Noop => {}
            shape => shapes.push(shape.clone()),
        }
    }

    let mut shapes = Vec::new();
    for clipped in &output.shapes {
        take_apart(&clipped.shape, &mut shapes);
    }
    shapes
}

fn meshes(output: &FullOutput) -> Vec<Arc<Mesh>> {
    painted(output)
        .into_iter()
        .filter_map(|shape| match shape {
            Shape::Mesh(mesh) => Some(mesh),
            _ => None,
        })
        .collect()
}

/// Filled discs of a pass: where, how large and what colour, with where in the order of
/// painting each came.
fn discs(output: &FullOutput) -> Vec<(usize, Pos2, f32, Color32)> {
    painted(output)
        .iter()
        .enumerate()
        .filter_map(|(order, shape)| match shape {
            Shape::Circle(circle) if circle.fill != Color32::TRANSPARENT => {
                Some((order, circle.center, circle.radius, circle.fill))
            }
            _ => None,
        })
        .collect()
}

/// Where in the order of painting the first shape with `text` in it came.
fn text_at(output: &FullOutput, text: &str) -> usize {
    painted(output)
        .iter()
        .position(|shape| matches!(shape, Shape::Text(shape) if shape.galley.text() == text))
        .unwrap_or_else(|| panic!("{text} is painted"))
}

/// Whether `pos` lies between the walls of `tube`, give or take `slack`.
fn within(tube: &CableGeometry, pos: Pos2, slack: f32) -> bool {
    let hit = tube.closest(pos);
    hit.distance <= tube.radius_at(hit.share) + slack
}

#[test]
fn the_patch_is_the_example_patch_of_the_source() {
    let snarl = patch::seed();
    let name = |id: NodeId| snarl[id].label.clone();
    let mut wires: Vec<(String, String)> = edges(&snarl)
        .into_iter()
        .map(|wire| (name(wire.out_pin.node), name(wire.in_pin.node)))
        .collect();
    wires.sort();
    let expected = [
        ("LFO 1", "Gloss"),
        ("LFO 1", "UV Cerise"),
        ("LFO 2", "LFO 1"),
        ("LFO 2", "UV Lemon"),
        ("Pen Pressure", "Slew"),
        ("Slew", "Size"),
    ]
    .map(|(from, to)| (from.to_owned(), to.to_owned()));
    assert_eq!(wires, expected);

    // Four pipes into the brush and two hoses into modules.
    let world = World::new(Look::Drawer);
    let kinds: Vec<Conduit> = edges(&world.snarl)
        .into_iter()
        .map(|wire| world.carry(wire).conduit)
        .collect();
    assert_eq!(
        kinds.iter().filter(|kind| **kind == Conduit::Pipe).count(),
        4
    );
    assert_eq!(
        kinds.iter().filter(|kind| **kind == Conduit::Hose).count(),
        2
    );

    // The nodes start clear of each other.
    let nodes = world.anatomy().nodes.clone();
    assert_eq!(nodes.len(), 8);
    for (index, node) in nodes.iter().enumerate() {
        for other in &nodes[index + 1..] {
            assert!(!node.screen_rect.intersects(other.screen_rect));
        }
    }
}

#[test]
fn the_drawer_look_lays_the_recipe_on_each_wire_and_a_socket_on_each_pin() {
    let mut world = World::new(Look::Drawer);
    let output = world.run(1.5);
    let anatomy = world.anatomy();
    let scale = world.scale();

    for wire in edges(&world.snarl) {
        let measured = anatomy.wire(wire).unwrap();
        let carry = world.carry(wire);
        let stream = world.viewer.signals.stream(wire);
        assert!(stream.level > 0.0);

        // The wire has the paint the recipe gives for what it carries, on the path the
        // view laid: the recipe asks for no path of its own.
        assert!(measured.geometry.radii().is_none());
        let expected = reference(
            carry,
            PipeView {
                lit: false,
                raised: false,
                scale,
                length: measured.geometry.length(),
                hit: 0.0,
            },
            stream,
        );
        assert_eq!(measured.paint.strokes(), expected.strokes());
        assert_eq!(measured.paint.beads().len(), expected.beads().len());
        assert_eq!(measured.paint.layer(), None);
        assert!(measured.paint.hit_width() >= 13.0);

        // Each slug is painted as a disc on the wire.
        let path = world.on_screen(wire);
        for bead in measured.paint.beads() {
            let at = path.anchor_at(bead.share).pos;
            assert!(
                discs(&output).iter().any(|&(_, centre, radius, fill)| {
                    centre.distance(at) < 0.05
                        && (radius - bead.radius * scale).abs() < 0.01
                        && fill == bead.fill
                }),
                "a slug at {at:?}"
            );
        }
    }

    // Each pin is three discs: rim, face and bore of the fitting that seats on it.
    let all = discs(&output);
    let face_at = |centre: Pos2| -> [Color32; 3] {
        [1.0, 0.72, 0.34].map(|share| {
            let radius = recipe::PIN_VISUAL_SIZE * 0.75 * share * scale;
            all.iter()
                .find(|&&(_, at, size, _)| {
                    at.distance(centre) < 0.05 && (size - radius).abs() < 0.01
                })
                .unwrap_or_else(|| panic!("a disc of {radius} at {centre:?}"))
                .3
        })
    };
    for wire in edges(&world.snarl) {
        let carry = world.carry(wire);
        let into = face_at(anatomy.in_pin(wire.in_pin).unwrap().screen_center);
        let metal = match carry.conduit {
            Conduit::Pipe => BRASS,
            Conduit::Hose => STEEL,
        };
        assert_eq!(into[0], shown(metal, 1.0));
        assert_eq!(into[1], shown(metal.map(|c| c * 0.45), 1.0));
        assert_eq!(into[2], shown(carry.into, 1.0));
    }
    // An input with nothing in it: steel and a dark bore.
    let unwired = InPinId {
        node: node(&world.snarl, "LFO 2"),
        input: 0,
    };
    let face = face_at(anatomy.in_pin(unwired).unwrap().screen_center);
    assert_eq!(
        face,
        [
            shown(STEEL, 1.0),
            shown(STEEL.map(|c| c * 0.45), 1.0),
            Color32::from_gray(10)
        ]
    );
}

#[test]
fn a_drawer_connection_is_lit_under_the_pointer_and_raised_when_selected() {
    let mut world = World::new(Look::Drawer);
    let _ = world.run(0.5);
    let wire = world.first(Conduit::Pipe);
    let target = world.snarl[wire.in_pin.node].label.clone();
    let rest = world.anatomy().wire(wire).unwrap().paint.strokes().to_vec();

    // Picked as wide as it is drawn: 13 units across, not the hairline of a plain wire.
    let path = world.on_screen(wire);
    let anchor = path.anchor_at(0.5);
    let scale = world.scale();
    let near = anchor.pos + anchor.normal * (6.0 * scale);
    let far = anchor.pos + anchor.normal * (6.5 * scale + 12.0);

    let _ = world.point_at(far);
    assert!(!world.anatomy().wire(wire).unwrap().hovered);

    let _ = world.point_at(near);
    let measured = world.anatomy().wire(wire).unwrap().clone();
    assert!(measured.hovered);
    assert_ne!(measured.paint.strokes(), rest, "lit");
    assert_eq!(measured.paint.layer(), None, "and not raised");

    // Behind the nodes until it is selected, and above them after.
    let order = |output: &FullOutput, world: &World| {
        let path = world.on_screen(wire);
        let first_stroke = painted(output)
            .iter()
            .position(|shape| match shape {
                Shape::Path(shape) => {
                    shape
                        .points
                        .first()
                        .is_some_and(|p| p.distance(path.start()) < 0.05)
                        && shape
                            .points
                            .last()
                            .is_some_and(|p| p.distance(path.end()) < 0.05)
                }
                _ => false,
            })
            .expect("the wire is painted as strokes along its path");
        (first_stroke, text_at(output, &target))
    };
    let output = world.point_at(near);
    let (stroke, title) = order(&output, &world);
    assert!(stroke < title, "behind its node");

    let output = world.click(near, PointerButton::Primary);
    assert_eq!(get_selected_wire(view(), &world.ctx), Some(wire));
    let measured = world.anatomy().wire(wire).unwrap().clone();
    assert!(measured.selected);
    assert_eq!(measured.paint.layer(), Some(WireLayer::AboveNodes));
    let (stroke, title) = order(&output, &world);
    assert!(stroke > title, "above its node");

    // A secondary click on it takes it out.
    let _ = world.click(near, PointerButton::Secondary);
    assert!(!edges(&world.snarl).contains(&wire));
}

#[test]
fn a_rack_connection_is_drawn_between_the_walls_it_is_picked_between() {
    let mut world = World::new(Look::Rack);
    let output = world.run(1.5);
    let anatomy = world.anatomy();
    let scale = world.scale();
    let wires = edges(&world.snarl);

    let tubes: Vec<CableGeometry> = wires.iter().map(|wire| world.on_screen(*wire)).collect();
    for (wire, tube) in wires.iter().zip(&tubes) {
        let carry = world.carry(*wire);
        let measured = anatomy.wire(*wire).unwrap();

        // The path is the view's own, with the half width of the conduit and a collar
        // near each end.
        assert_eq!(
            measured.geometry.start(),
            anatomy.out_pin(wire.out_pin).unwrap().graph_center
        );
        assert_eq!(
            measured.geometry.end(),
            anatomy.in_pin(wire.in_pin).unwrap().graph_center
        );
        let (radius, collar) = match carry.conduit {
            Conduit::Pipe => (recipe::PIPE_RADIUS, 1.32),
            Conduit::Hose => (recipe::HOSE_RADIUS, 1.45),
        };
        let radius = radius.max(radius * (4.5 / 6.5) / scale);
        assert!((measured.geometry.radius_at(0.5) - radius).abs() < 1e-4);
        let length = measured.geometry.length();
        for share in [4.0 / length, 1.0 - 4.0 / length] {
            assert!((measured.geometry.radius_at(share) - radius * collar).abs() < 1e-3);
        }
        // Nothing is laid along it as strokes or discs: the tube is the wire.
        assert!(measured.paint.strokes().is_empty());
        assert!(measured.paint.beads().is_empty());
        assert_eq!(measured.paint.layer(), None);

        // The pointer picks it between those walls and not outside them.
        for share in [0.2, 0.5, 0.8] {
            let anchor = tube.anchor_at(share);
            let wall = tube.radius_at(share);
            let _ = world.point_at(anchor.pos + anchor.normal * (wall - 0.5));
            assert!(world.anatomy().wire(*wire).unwrap().hovered, "{share}");
            let _ = world.point_at(anchor.pos - anchor.normal * (wall + 3.0));
            assert!(!world.anatomy().wire(*wire).unwrap().hovered, "{share}");
        }
    }

    // Every body is a tube of the recipe: a ring of points across for each point along.
    // None of them reaches outside the walls of the wire it is on.
    let found = meshes(&output);
    let ring = ACROSS + 1;
    let bodies: Vec<&Arc<Mesh>> = found
        .iter()
        .filter(|mesh| mesh.vertices.len() % ring == 0 && mesh.indices.len() % (6 * ACROSS) == 0)
        .collect();
    let pipes = wires
        .iter()
        .filter(|w| world.carry(**w).conduit == Conduit::Pipe)
        .count();
    let hoses = wires.len() - pipes;
    // A pipe: glass and two couplings. A hose: rubber, a window and two nuts.
    assert_eq!(bodies.len(), 3 * pipes + 4 * hoses);
    for mesh in &bodies {
        assert!(
            tubes.iter().any(|tube| mesh
                .vertices
                .iter()
                .all(|vertex| within(tube, vertex.pos, 0.2))),
            "a body lies outside the wire it is on"
        );
    }
    // The rest are the halos: one for each pipe and one for each hose's window.
    assert_eq!(found.len() - bodies.len(), pipes + hoses);

    // The glass of each pipe runs its whole length, wall to wall.
    for (wire, tube) in wires.iter().zip(&tubes) {
        let points = tube.points().len();
        let glass = bodies
            .iter()
            .find(|mesh| {
                mesh.vertices.len() == ring * points
                    && mesh.vertices[0].pos.distance(tube.start()) < 2.0 * tube.radius_at(0.0)
                    && mesh.vertices.iter().all(|v| within(tube, v.pos, 0.2))
            })
            .unwrap_or_else(|| panic!("{wire:?} has a body along all of it"));
        let middle = points / 2;
        let across = glass.vertices[middle * ring]
            .pos
            .distance(glass.vertices[middle * ring + ACROSS].pos);
        let share = tube
            .closest(glass.vertices[middle * ring + ACROSS / 2].pos)
            .share;
        assert!(
            (across - 2.0 * tube.radius_at(share)).abs() < 0.3,
            "{across}"
        );
    }
}

#[test]
fn paint_arriving_blooms_in_front_of_the_control_and_a_selected_tube_is_raised() {
    let mut world = World::new(Look::Rack);
    let output = world.run(1.5);
    let scale = world.scale();
    let pipe = world.first(Conduit::Pipe);
    let hose = world.first(Conduit::Hose);

    // The bloom of a pipe: three rings and a bead on its far end, painted after the node
    // it runs into. A hose has none.
    let bloom = |output: &FullOutput, world: &World, wire: WirePins| -> Vec<usize> {
        let end = world.on_screen(wire).end();
        let socket = recipe::PIN_VISUAL_SIZE * 0.75 * scale;
        discs(output)
            .into_iter()
            .filter(|&(_, at, radius, _)| {
                at.distance(end) < 0.05
                    && [1.0, 0.72, 0.34]
                        .iter()
                        .all(|share| (radius - socket * share).abs() > 0.01)
            })
            .map(|(order, ..)| order)
            .collect()
    };
    let stream = world.viewer.signals.stream(pipe);
    assert!(stream.level > 0.02);
    let rings = bloom(&output, &world, pipe);
    assert_eq!(rings.len(), 4);
    let title = text_at(&output, &world.snarl[pipe.in_pin.node].label.clone());
    assert!(
        rings.iter().all(|order| *order > title),
        "in front of its node"
    );
    assert!(bloom(&output, &world, hose).is_empty());

    // Its glass is behind the node until the pipe is selected.
    let glass = |output: &FullOutput, world: &World| {
        let tube = world.on_screen(pipe);
        painted(output)
            .iter()
            .position(|shape| match shape {
                Shape::Mesh(mesh) => {
                    mesh.vertices.len() == (ACROSS + 1) * tube.points().len()
                        && mesh.vertices.iter().all(|v| within(&tube, v.pos, 0.2))
                }
                _ => false,
            })
            .expect("the glass is painted")
    };
    assert!(glass(&output, &world) < title);

    let tube = world.on_screen(pipe);
    let anchor = tube.anchor_at(0.5);
    let inside = anchor.pos + anchor.normal * (tube.radius_at(0.5) - 1.0);
    let output = world.click(inside, PointerButton::Primary);
    assert_eq!(get_selected_wire(view(), &world.ctx), Some(pipe));
    assert_eq!(
        world.anatomy().wire(pipe).unwrap().paint.layer(),
        Some(WireLayer::AboveNodes)
    );
    let title = text_at(&output, &world.snarl[pipe.in_pin.node].label.clone());
    assert!(glass(&output, &world) > title, "raised above its node");

    // A dry pipe has no bloom.
    world.viewer.signals.pressure = 0.0;
    world.viewer.signals.rate = 0.0;
    let size = edges(&world.snarl)
        .into_iter()
        .find(|wire| world.snarl[wire.in_pin.node].label == "Size")
        .unwrap();
    let output = world.run(4.0);
    assert!(world.viewer.signals.stream(size).level < 0.02);
    assert!(bloom(&output, &world, size).is_empty());
}

#[test]
fn a_tube_keeps_its_width_on_the_screen_when_the_view_is_far_out() {
    // The drawer recipe widens its strokes below a scale of 4.5 / 6.5; the route of the
    // rack recipe does the same for its walls.
    let route = |scale: f32, kind| {
        let route = egui_snarl::ui::WireRoute {
            pins: None,
            from: Pos2::new(0.0, 0.0),
            to: Pos2::new(300.0, 80.0),
            style: egui_snarl::ui::WireStyle::Bezier3,
            frame_size: 40.0,
            tolerance: 0.5,
            scale,
            sag: egui::Vec2::ZERO,
        };
        recipe::rack_route(&route, kind)
    };
    assert_eq!(route(1.0, Conduit::Pipe).radius_at(0.5), 6.5);
    assert_eq!(route(1.0, Conduit::Hose).radius_at(0.5), 5.0);
    assert!((route(0.25, Conduit::Pipe).radius_at(0.5) - 18.0).abs() < 1e-4);
    // 4.5 points of half width on the screen, as the drawer's strokes keep.
    assert!((route(0.25, Conduit::Pipe).radius_at(0.5) * 0.25 - 4.5).abs() < 1e-4);

    // A connection too short for collars has none, and is still a tube.
    let short = recipe::rack_route(
        &egui_snarl::ui::WireRoute {
            pins: None,
            from: Pos2::new(0.0, 0.0),
            to: Pos2::new(20.0, 0.0),
            style: egui_snarl::ui::WireStyle::Bezier3,
            frame_size: 4.0,
            tolerance: 0.5,
            scale: 1.0,
            sag: egui::Vec2::ZERO,
        },
        Conduit::Pipe,
    );
    assert!(
        short
            .radii()
            .unwrap()
            .iter()
            .all(|radius| (radius - 6.5).abs() < 1e-6)
    );
}

#[test]
fn a_plain_look_asks_nothing_of_wires_or_pins() {
    let mut world = World::new(Look::Plain);
    let output = world.run(1.0);
    let anatomy = world.anatomy();

    assert!(
        meshes(&output)
            .iter()
            .all(|mesh| mesh.vertices.len() % (ACROSS + 1) != 0
                || mesh.vertices.len() < 3 * (ACROSS + 1))
    );
    for wire in edges(&world.snarl) {
        let measured = anatomy.wire(wire).unwrap();
        assert_eq!(
            measured.paint.strokes().len(),
            1,
            "one stroke, the view's own"
        );
        assert!(measured.paint.beads().is_empty());
        assert_eq!(measured.paint.layer(), None);
        assert!(measured.geometry.radii().is_none());
    }
    // No socket: a pin is the one dot of the view.
    let socket = recipe::PIN_VISUAL_SIZE * 0.75 * world.scale();
    assert!(
        discs(&output)
            .iter()
            .all(|&(_, _, radius, _)| (radius - socket).abs() > 0.01)
    );

    // And a click on a wire selects nothing, as in any graph that has not asked for it.
    let wire = edges(&world.snarl)[0];
    let at = world.on_screen(wire).anchor_at(0.5).pos;
    let _ = world.click(at, PointerButton::Primary);
    assert_eq!(get_selected_wire(view(), &world.ctx), None);
    assert_eq!(edges(&world.snarl).len(), 6);
}
