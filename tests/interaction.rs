//! Runs graph views through real egui passes, without a window, and checks what they measured.

use std::sync::Arc;

use egui::{
    Color32, Context, Event, FullOutput, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect,
    Shape, TouchDeviceId, TouchId, TouchPhase, Ui, Vec2, emath::TSTransform, pos2, vec2,
};
use egui_snarl::{
    InPin, InPinId, NodeId, OutPin, OutPinId, Snarl,
    ui::{
        AnyPin, CableGeometry, InsertionAnchor, PinInfo, SnarlAnatomy, SnarlPin, SnarlStyle,
        SnarlViewer, SnarlWidget, WireAction, WireLayer, WirePaint, WirePins, WireRoute,
        WireTrigger, WireView, get_selected_nodes, get_selected_wire, get_snarl_anatomy,
        set_selected_wire,
    },
};

const SCREEN: Vec2 = vec2(1000.0, 700.0);
const OUTER: &str = "outer";
const INNER: &str = "inner";
const INNER_SIZE: Vec2 = vec2(260.0, 170.0);

/// A colour no pin is drawn in.
const BEAD: Color32 = Color32::from_rgb(250, 240, 20);
const BEAD_RADIUS: f32 = 5.0;

/// Colours no pin, node or default wire is drawn in.
const TUBE_FILL: Color32 = Color32::from_rgb(20, 250, 240);
const TUBE_WALL: Color32 = Color32::from_rgb(240, 20, 250);
const TUBE_PLUG: Color32 = Color32::from_rgb(250, 240, 20);

fn view(name: &str) -> Id {
    Id::unique(name)
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Node {
    label: String,
    inputs: usize,
    outputs: usize,

    /// Extra widgets shown before the label of the first input row.
    leading_widgets: usize,

    /// A graph shown inside this node.
    inner: Option<Box<Snarl<Node>>>,
}

impl Node {
    fn plain(label: &str, inputs: usize, outputs: usize) -> Self {
        Self {
            label: label.to_owned(),
            inputs,
            outputs,
            leading_widgets: 0,
            inner: None,
        }
    }
}

#[derive(Default)]
struct Viewer {
    glow: bool,
    custom_route: bool,

    /// Length, in screen points, of the straight lead the route leaves each pin with.
    screen_lead: Option<f32>,

    /// Where along every wire a disc is placed, as shares of its length.
    beads: Vec<f32>,

    /// A wire routed and painted as a tube that bulges and bends, and the moment it is at:
    /// where along the wire the bulge is, as a share of its length.
    tube: Option<(WirePins, f32)>,

    /// A wire painted in a layer of its own choosing, and that layer.
    layered: Option<(WirePins, WireLayer)>,
    wire_menu: bool,
    graph_menu: bool,

    actions: Vec<WireAction>,
    connects: usize,
    disconnects: usize,
    moved: Vec<(Vec<NodeId>, Vec2, bool)>,
    passes: Vec<Arc<SnarlAnatomy>>,

    /// Where the label of each output row was laid out in the last pass.
    output_labels: Vec<(OutPinId, Rect)>,

    graph_menu_positions: Vec<Pos2>,
    graph_menu_popup_corners: Vec<Pos2>,
    graph_menu_section: Option<Rect>,
    wire_menu_anchors: Vec<(WirePins, InsertionAnchor)>,

    inner: Option<Box<Self>>,
}

impl SnarlViewer<Node> for Viewer {
    fn title(&mut self, node: &Node) -> String {
        node.label.clone()
    }

    fn inputs(&mut self, node: &Node) -> usize {
        node.inputs
    }

    fn outputs(&mut self, node: &Node) -> usize {
        node.outputs
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        let node = &snarl[pin.id.node];
        ui.set_max_width(140.0);

        if pin.id.input == 0 {
            for _ in 0..node.leading_widgets {
                let _ = ui.small_button("+");
            }
            ui.add(egui::Label::new(node.label.as_str()).wrap());
        } else {
            ui.label("in");
        }

        PinInfo::circle().with_fill(Color32::LIGHT_BLUE)
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        _snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        let label = ui.label("out").rect;
        self.output_labels.retain(|(id, _)| *id != pin.id);
        self.output_labels.push((pin.id, label));
        PinInfo::circle().with_fill(Color32::LIGHT_RED)
    }

    fn has_body(&mut self, node: &Node) -> bool {
        node.inner.is_some()
    }

    fn show_body(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) {
        let (Some(inner), Some(viewer)) = (snarl[node].inner.as_deref_mut(), self.inner.as_mut())
        else {
            return;
        };

        SnarlWidget::new()
            .id(view(INNER))
            .style(style())
            .min_size(INNER_SIZE)
            .max_size(INNER_SIZE)
            .show(inner, viewer.as_mut(), ui);
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<Node>) {
        self.connects += 1;
        snarl.connect(from.id, to.id);
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<Node>) {
        self.disconnects += 1;
        snarl.disconnect(from.id, to.id);
    }

    fn nodes_moved(
        &mut self,
        nodes: &[NodeId],
        delta: Vec2,
        finished: bool,
        _snarl: &mut Snarl<Node>,
    ) {
        self.moved.push((nodes.to_vec(), delta, finished));
    }

    fn wire_route(&mut self, route: &WireRoute, _snarl: &Snarl<Node>) -> Option<CableGeometry> {
        if let Some((pins, moment)) = self.tube
            && route.pins == Some(pins)
        {
            // The default path, bowed sideways between its fixed ends, with a bulge on it.
            let path = route.default_route().subdivided(3.0);
            let bowed = CableGeometry::from_points(
                path.offset(|share| 18.0 * moment * (std::f32::consts::PI * share).sin()),
            );
            return Some(bowed.with_radii(|share| {
                let from_bulge = (share - moment) / 0.07;
                14.0f32.mul_add((-from_bulge * from_bulge).exp(), 1.5)
            }));
        }

        if let Some(lead) = self.screen_lead {
            let lead = vec2(lead / route.scale, 0.0);
            return Some(CableGeometry::from_points([
                route.from,
                route.from + lead,
                route.to - lead,
                route.to,
            ]));
        }

        self.custom_route.then(|| {
            let corner = pos2(route.to.x, route.from.y);
            CableGeometry::from_points([route.from, corner, route.to])
        })
    }

    fn wire_paint(
        &mut self,
        wire: &WireView<'_>,
        default: WirePaint,
        _snarl: &Snarl<Node>,
    ) -> WirePaint {
        let paint = if wire.geometry.radii().is_some() {
            let [near, far] = wire.geometry.walls();
            WirePaint::hidden()
                .under_shape(wire.geometry.ribbon(|_| TUBE_FILL))
                .over_shape(Shape::line(near, (1.0, TUBE_WALL)))
                .over_shape(Shape::line(far, (1.0, TUBE_WALL)))
                .front_shape(Shape::circle_filled(wire.geometry.end(), 5.0, TUBE_PLUG))
        } else if self.glow {
            WirePaint::solid((6.0, wire.color)).with_glow(Color32::from_rgb(255, 140, 60), 8.0)
        } else {
            default
        };

        let paint = self.beads.iter().fold(paint, |paint, &share| {
            paint.with_bead(share, BEAD_RADIUS, BEAD)
        });

        match self.layered {
            Some((pins, layer)) if wire.pins == Some(pins) => paint.with_layer(layer),
            _ => paint,
        }
    }

    fn wire_action(&mut self, action: &WireAction, _ui: &mut Ui, snarl: &mut Snarl<Node>) {
        self.actions.push(*action);

        // What the trait does when this method is not overridden.
        if matches!(
            action.trigger,
            WireTrigger::SecondaryClick | WireTrigger::Delete
        ) {
            let from = snarl.out_pin(action.wire.out_pin);
            let to = snarl.in_pin(action.wire.in_pin);
            self.disconnect(&from, &to, snarl);
        }
    }

    fn has_wire_menu(&mut self, _wire: WirePins, _snarl: &mut Snarl<Node>) -> bool {
        self.wire_menu
    }

    fn show_wire_menu(
        &mut self,
        wire: WirePins,
        anchor: InsertionAnchor,
        ui: &mut Ui,
        _snarl: &mut Snarl<Node>,
    ) {
        self.wire_menu_anchors.push((wire, anchor));
        ui.label("Insert on wire");
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<Node>) -> bool {
        self.graph_menu
    }

    fn show_graph_menu(
        &mut self,
        pos: Pos2,
        _selected_nodes: &[NodeId],
        ui: &mut Ui,
        _snarl: &mut Snarl<Node>,
    ) {
        self.graph_menu_positions.push(pos);
        self.graph_menu_popup_corners.push(ui.cursor().min);

        ui.set_min_size(vec2(220.0, 180.0));
        let section = ui.collapsing("Sources", |ui| {
            ui.label("Oscillator");
            ui.label("Envelope");
        });
        self.graph_menu_section = Some(section.header_response.rect);
    }

    fn menu_close_behavior(&self) -> egui::PopupCloseBehavior {
        egui::PopupCloseBehavior::CloseOnClickOutside
    }

    fn measured(&mut self, anatomy: &SnarlAnatomy, _ui: &mut Ui, _snarl: &mut Snarl<Node>) {
        self.passes.push(Arc::new(anatomy.clone()));
    }
}

fn style() -> SnarlStyle {
    SnarlStyle {
        wire_width: Some(2.0),
        wire_hit_tolerance: Some(6.0),
        // What these tests are about is off unless a style asks for it.
        wire_picking: Some(true),
        nodes_cover_wires: Some(true),
        wire_smoothness_on_screen: Some(true),
        output_rows_clear_pins: Some(true),
        // A double click must not move the view under the pointer in these tests.
        centering: Some(false),
        ..SnarlStyle::new()
    }
}

struct World {
    ctx: Context,
    time: f64,
    snarl: Snarl<Node>,
    viewer: Viewer,
    style: SnarlStyle,
}

impl World {
    fn new(snarl: Snarl<Node>, viewer: Viewer) -> Self {
        let mut world = Self {
            ctx: Context::default(),
            time: 0.0,
            snarl,
            viewer,
            style: style(),
        };
        world.settle();
        world
    }

    fn frame(&mut self, events: Vec<Event>) -> FullOutput {
        self.time += 1.0 / 60.0;

        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            focused: true,
            events,
            ..RawInput::default()
        };

        let Self {
            ctx,
            snarl,
            viewer,
            style,
            ..
        } = self;

        let mut output = ctx.run_ui(input, |ui| {
            SnarlWidget::new()
                .id(view(OUTER))
                .style(*style)
                .show(snarl, viewer, ui);
        });

        // Nothing is rendered here, so the font atlas has nowhere to go.
        output.textures_delta.clear();
        output
    }

    fn settle(&mut self) -> FullOutput {
        for _ in 0..5 {
            let _ = self.frame(Vec::new());
        }
        self.frame(Vec::new())
    }

    fn outer(&self) -> Arc<SnarlAnatomy> {
        get_snarl_anatomy(view(OUTER), &self.ctx).expect("the outer view was shown")
    }

    fn inner(&self) -> Arc<SnarlAnatomy> {
        get_snarl_anatomy(view(INNER), &self.ctx).expect("the inner view was shown")
    }

    fn move_to(&mut self, pos: Pos2) {
        let _ = self.frame(vec![Event::PointerMoved(pos)]);
        let _ = self.frame(Vec::new());
    }

    fn button(&mut self, pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) {
        let _ = self.frame(vec![Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        }]);
    }

    fn click(&mut self, pos: Pos2, button: PointerButton) {
        self.click_with(pos, button, Modifiers::NONE);
    }

    fn click_with(&mut self, pos: Pos2, button: PointerButton, modifiers: Modifiers) {
        let _ = self.frame(vec![Event::ModifiersChanged(modifiers)]);
        self.move_to(pos);
        self.button(pos, button, true, modifiers);
        self.button(pos, button, false, modifiers);
        let _ = self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.move_to(from);
        self.button(from, PointerButton::Primary, true, Modifiers::NONE);
        for step in 1..=6 {
            #[allow(clippy::cast_precision_loss)]
            let pos = from.lerp(to, step as f32 / 6.0);
            let _ = self.frame(vec![Event::PointerMoved(pos)]);
        }
        let _ = self.frame(Vec::new());
        self.button(to, PointerButton::Primary, false, Modifiers::NONE);
        let _ = self.frame(Vec::new());
    }

    fn zoom_at(&mut self, pos: Pos2, factor: f32) {
        self.move_to(pos);
        let _ = self.frame(vec![Event::Zoom(factor)]);
        let _ = self.frame(Vec::new());
    }

    /// One finger going down, moving or lifting, with the pointer events a windowing
    /// backend sends alongside it.
    fn touch(&mut self, phase: TouchPhase, pos: Pos2) {
        let mut events = vec![Event::Touch {
            device_id: TouchDeviceId(0),
            id: TouchId(0),
            phase,
            pos,
            force: None,
        }];

        match phase {
            TouchPhase::Start => {
                events.push(Event::PointerMoved(pos));
                events.push(Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                });
            }
            TouchPhase::Move => events.push(Event::PointerMoved(pos)),
            TouchPhase::End | TouchPhase::Cancel => {
                events.push(Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                });
                events.push(Event::PointerGone);
            }
        }

        let _ = self.frame(events);
    }

    /// Puts a finger down on `pos` and lifts it again at once.
    fn tap(&mut self, pos: Pos2) {
        self.touch(TouchPhase::Start, pos);
        let _ = self.frame(Vec::new());
        self.touch(TouchPhase::End, pos);
        let _ = self.frame(Vec::new());
    }

    /// Holds a finger still on `pos` for longer than a tap may last, then lifts it.
    fn long_press(&mut self, pos: Pos2) {
        self.touch(TouchPhase::Start, pos);
        for _ in 0..60 {
            let _ = self.frame(Vec::new());
        }
        self.touch(TouchPhase::End, pos);
        let _ = self.frame(Vec::new());
    }

    fn key(&mut self, key: Key) {
        for pressed in [true, false] {
            let _ = self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }
    }

    fn wires(&self) -> Vec<WirePins> {
        let mut wires: Vec<WirePins> = self
            .snarl
            .wires()
            .map(|(out_pin, in_pin)| WirePins::new(out_pin, in_pin))
            .collect();
        wires.sort_unstable();
        wires
    }
}

fn out_pin(node: NodeId, output: usize) -> OutPinId {
    OutPinId { node, output }
}

fn in_pin(node: NodeId, input: usize) -> InPinId {
    InPinId { node, input }
}

/// Two nodes far enough apart that the middle of the wire between them is over empty canvas.
fn pair(connected: bool) -> (Snarl<Node>, WirePins) {
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(pos2(0.0, 0.0), Node::plain("source", 0, 1));
    let sink = snarl.insert_node(pos2(320.0, 170.0), Node::plain("sink", 2, 0));
    let wire = WirePins::new(out_pin(source, 0), in_pin(sink, 0));
    if connected {
        snarl.connect(wire.out_pin, wire.in_pin);
    }
    (snarl, wire)
}

/// A group node holding a graph of its own, wired to a plain node.
fn nested() -> (Snarl<Node>, NodeId, WirePins, WirePins) {
    let mut inner = Snarl::new();
    let a = inner.insert_node(pos2(0.0, 0.0), Node::plain("a", 0, 1));
    let b = inner.insert_node(pos2(190.0, 110.0), Node::plain("b", 1, 0));
    let inner_wire = WirePins::new(out_pin(a, 0), in_pin(b, 0));
    inner.connect(inner_wire.out_pin, inner_wire.in_pin);

    let mut outer = Snarl::new();
    let group = outer.insert_node(
        pos2(0.0, 0.0),
        Node {
            inner: Some(Box::new(inner)),
            ..Node::plain("group", 1, 1)
        },
    );
    let sink = outer.insert_node(pos2(560.0, 330.0), Node::plain("sink", 1, 0));
    let outer_wire = WirePins::new(out_pin(group, 0), in_pin(sink, 0));
    outer.connect(outer_wire.out_pin, outer_wire.in_pin);

    (outer, group, outer_wire, inner_wire)
}

fn nested_viewer() -> Viewer {
    Viewer {
        inner: Some(Box::default()),
        ..Viewer::default()
    }
}

fn open_paths(shape: &Shape, paths: &mut Vec<Vec<Pos2>>) {
    match shape {
        Shape::Vec(shapes) => {
            for shape in shapes {
                open_paths(shape, paths);
            }
        }
        Shape::Path(path) if !path.closed => paths.push(path.points.clone()),
        _ => {}
    }
}

/// Every shape in the output, in the order it is painted, with groups taken apart.
fn painted(output: &FullOutput) -> Vec<&Shape> {
    fn take_apart<'a>(shape: &'a Shape, shapes: &mut Vec<&'a Shape>) {
        match shape {
            Shape::Vec(group) => {
                for shape in group {
                    take_apart(shape, shapes);
                }
            }
            Shape::Noop => {}
            shape => shapes.push(shape),
        }
    }

    let mut shapes = Vec::new();
    for clipped in &output.shapes {
        take_apart(&clipped.shape, &mut shapes);
    }
    shapes
}

/// Where in paint order the first open path through exactly `points` comes.
fn first_painted_along(shapes: &[&Shape], points: &[Pos2]) -> Option<usize> {
    shapes.iter().position(|shape| {
        matches!(shape, Shape::Path(path) if !path.closed
            && path.points.len() == points.len()
            && path
                .points
                .iter()
                .zip(points)
                .all(|(painted, expected)| painted.distance(*expected) < 0.05))
    })
}

/// How many open paths in the output run through exactly `points`.
fn painted_along(output: &FullOutput, points: &[Pos2]) -> usize {
    let mut paths = Vec::new();
    for clipped in &output.shapes {
        open_paths(&clipped.shape, &mut paths);
    }

    paths
        .iter()
        .filter(|path| {
            path.len() == points.len()
                && path
                    .iter()
                    .zip(points)
                    .all(|(painted, expected)| painted.distance(*expected) < 0.05)
        })
        .count()
}

/// A point of the screen that is on the canvas of `anatomy` and on none of its nodes.
fn empty_spot(anatomy: &SnarlAnatomy) -> Pos2 {
    let visible = anatomy.transform.global_clip_rect.shrink(6.0);

    [
        visible.right_bottom(),
        visible.left_bottom(),
        visible.right_top(),
        visible.left_top(),
    ]
    .into_iter()
    .find(|&spot| {
        !anatomy
            .nodes
            .iter()
            .any(|node| node.screen_rect.expand(4.0).contains(spot))
    })
    .expect("a corner of the view is free of nodes")
}

fn assert_close(actual: Pos2, expected: Pos2, slack: f32, what: &str) {
    assert!(
        actual.distance(expected) <= slack,
        "{what}: {actual:?} is not within {slack} of {expected:?}"
    );
}

fn assert_same_transform(actual: TSTransform, expected: TSTransform, what: &str) {
    assert!(
        (actual.scaling - expected.scaling).abs() < 1e-4
            && (actual.translation - expected.translation).length() < 1e-2,
        "{what}: {actual:?} is not {expected:?}"
    );
}

#[test]
fn a_wire_is_painted_and_hit_tested_along_one_path_at_every_zoom() {
    for (glow, custom_route, strokes) in [(false, false, 1), (true, false, 4), (true, true, 4)] {
        let (snarl, wire) = pair(true);
        let mut world = World::new(
            snarl,
            Viewer {
                glow,
                custom_route,
                ..Viewer::default()
            },
        );

        for zoom in [1.0, 0.6, 1.9] {
            let spot = empty_spot(&world.outer());
            world.zoom_at(spot, zoom);
            world.move_to(spot);

            let output = world.settle();
            let anatomy = world.outer();
            let measured = anatomy.wire(wire).expect("the wire was measured");
            let to_screen = anatomy.transform.to_global;
            let on_screen = measured.geometry.transformed(to_screen);

            // The wire runs from one measured pin centre to the other.
            let from = anatomy.out_pin(wire.out_pin).unwrap();
            let to = anatomy.in_pin(wire.in_pin).unwrap();
            assert_eq!(measured.geometry.start(), from.graph_center);
            assert_eq!(measured.geometry.end(), to.graph_center);
            assert_close(on_screen.start(), from.screen_center, 0.01, "wire start");

            if custom_route {
                assert_eq!(
                    measured.geometry.points().len(),
                    3,
                    "the viewer's route is used"
                );
            }

            // Every stroke of the theme lies along the measured path, and nothing else does.
            assert_eq!(measured.paint.strokes().len(), strokes);
            assert_eq!(
                painted_along(&output, on_screen.points()),
                strokes,
                "glow {glow}, custom route {custom_route}, scale {}",
                anatomy.transform.scale()
            );

            // The pointer picks the wire within the same band of screen points at every zoom.
            let band = measured.hit_tolerance * anatomy.transform.scale();
            let expected_band = if glow {
                6.0f32.max(3.0 * anatomy.transform.scale())
            } else {
                6.0
            };
            assert!((band - expected_band).abs() < 1e-3, "band {band}");

            let middle = on_screen.midpoint();
            for (reach, on_wire) in [(0.0, true), (0.8, true), (1.4, false)] {
                world.move_to(middle.pos + middle.normal * (band * reach));
                let hovered = world.outer().hovered_wire().map(|wire| wire.pins);
                assert_eq!(
                    hovered,
                    on_wire.then_some(wire),
                    "reach {reach} at scale {}",
                    anatomy.transform.scale()
                );
            }
        }
    }
}

#[test]
fn a_route_can_keep_a_length_on_screen_at_every_zoom() {
    let (snarl, wire) = pair(true);
    let mut world = World::new(
        snarl,
        Viewer {
            screen_lead: Some(24.0),
            ..Viewer::default()
        },
    );

    let mut scales = Vec::new();
    for zoom in [1.0, 0.3, 2.5] {
        let spot = empty_spot(&world.outer());
        world.zoom_at(spot, zoom);
        world.settle();

        let anatomy = world.outer();
        let scale = anatomy.transform.scale();
        scales.push(scale);

        let on_screen = anatomy
            .wire(wire)
            .expect("the wire was measured")
            .geometry
            .transformed(anatomy.transform.to_global);

        let [from, lead_out, lead_in, to] = on_screen.points() else {
            panic!("the viewer's route has four points");
        };
        assert!(
            (from.distance(*lead_out) - 24.0).abs() < 1e-2,
            "lead out of the pin at scale {scale}"
        );
        assert!(
            (lead_in.distance(*to) - 24.0).abs() < 1e-2,
            "lead into the pin at scale {scale}"
        );
    }

    assert!(
        scales[1] < 0.75 && scales[2] > 1.25,
        "the view was zoomed out and in: {scales:?}"
    );
}

#[test]
fn beads_ride_on_the_path_a_wire_is_painted_and_picked_along() {
    const SHARES: [f32; 3] = [0.0, 0.4, 1.0];

    for custom_route in [false, true] {
        let (snarl, wire) = pair(true);
        let mut world = World::new(
            snarl,
            Viewer {
                beads: SHARES.to_vec(),
                custom_route,
                ..Viewer::default()
            },
        );

        for zoom in [1.0, 0.3, 2.5] {
            let spot = empty_spot(&world.outer());
            world.zoom_at(spot, zoom);
            world.move_to(spot);

            let output = world.settle();
            let anatomy = world.outer();
            let scale = anatomy.transform.scale();
            let measured = anatomy.wire(wire).expect("the wire was measured");
            let on_screen = measured.geometry.transformed(anatomy.transform.to_global);
            assert_eq!(measured.paint.beads().len(), SHARES.len());

            let shapes = painted(&output);
            let discs: Vec<(usize, &egui::epaint::CircleShape)> = shapes
                .iter()
                .enumerate()
                .filter_map(|(index, shape)| match shape {
                    Shape::Circle(disc) if disc.fill == BEAD => Some((index, disc)),
                    _ => None,
                })
                .collect();
            assert_eq!(discs.len(), SHARES.len(), "scale {scale}");

            let stroke = first_painted_along(&shapes, on_screen.points())
                .expect("the wire's stroke was painted");

            for (&(index, disc), share) in discs.iter().zip(SHARES) {
                assert_close(
                    disc.center,
                    on_screen.anchor_at(share).pos,
                    0.02,
                    "bead centre",
                );
                assert!(
                    (disc.radius - BEAD_RADIUS * scale).abs() < 1e-3,
                    "bead radius {} at scale {scale}",
                    disc.radius
                );
                assert!(index > stroke, "a bead is painted in front of its wire");
            }

            // The middle bead sits where the pointer picks the wire.
            world.move_to(discs[1].1.center);
            assert_eq!(
                world.outer().hovered_wire().map(|wire| wire.pins),
                Some(wire),
                "custom route {custom_route}, scale {scale}"
            );
        }
    }
}

#[test]
fn one_wire_can_be_painted_above_the_nodes_while_the_rest_stay_behind() {
    for (style_layer, own_layer) in [
        (WireLayer::BehindNodes, WireLayer::AboveNodes),
        (WireLayer::AboveNodes, WireLayer::BehindNodes),
    ] {
        // Two wires from the left node to the right one, both passing the node in between.
        let mut snarl = Snarl::new();
        let left = snarl.insert_node(pos2(0.0, 0.0), Node::plain("left", 0, 2));
        let between = snarl.insert_node(pos2(190.0, 0.0), Node::plain("between", 2, 0));
        let right = snarl.insert_node(pos2(420.0, 0.0), Node::plain("right", 2, 0));

        let plain = WirePins::new(out_pin(left, 0), in_pin(right, 0));
        let layered = WirePins::new(out_pin(left, 1), in_pin(right, 1));
        snarl.connect(plain.out_pin, plain.in_pin);
        snarl.connect(layered.out_pin, layered.in_pin);

        let mut world = World::new(
            snarl,
            Viewer {
                layered: Some((layered, own_layer)),
                ..Viewer::default()
            },
        );
        world.style.wire_layer = Some(style_layer);

        let spot = empty_spot(&world.outer());
        world.move_to(spot);
        let output = world.settle();
        let anatomy = world.outer();
        let to_screen = anatomy.transform.to_global;
        let shapes = painted(&output);

        let frame = anatomy.node(between).unwrap().screen_rect;
        let node = shapes
            .iter()
            .position(|shape| {
                matches!(shape, Shape::Rect(rect)
                if rect.rect.min.distance(frame.min) < 0.5 && rect.rect.max.distance(frame.max) < 0.5)
            })
            .expect("the frame of the node in between was painted");

        let order = |pins: WirePins| {
            let wire = anatomy.wire(pins).expect("the wire was measured");
            let on_screen = wire.geometry.transformed(to_screen);
            assert!(
                on_screen
                    .points()
                    .iter()
                    .any(|&point| frame.shrink(2.0).contains(point)),
                "the wire passes the node in between"
            );
            first_painted_along(&shapes, on_screen.points()).expect("the wire was painted")
        };

        let (behind, above) = match own_layer {
            WireLayer::AboveNodes => (plain, layered),
            WireLayer::BehindNodes => (layered, plain),
        };
        assert!(order(behind) < node, "style {style_layer:?}");
        assert!(order(above) > node, "style {style_layer:?}");
        assert_eq!(
            anatomy.wire(layered).unwrap().paint.layer(),
            Some(own_layer)
        );
        assert_eq!(anatomy.wire(plain).unwrap().paint.layer(), None);

        // Painting a wire above a node does not take the pointer from that node.
        let over_node = anatomy
            .wire(above)
            .unwrap()
            .geometry
            .transformed(to_screen)
            .points()
            .iter()
            .copied()
            .find(|&point| frame.shrink(2.0).contains(point))
            .unwrap();
        world.move_to(over_node);
        assert_eq!(world.outer().hovered_wire().map(|wire| wire.pins), None);
    }
}

/// The meshes in the output with the clip rectangle each is painted in.
fn meshes(output: &FullOutput) -> Vec<(Rect, Arc<egui::Mesh>)> {
    fn take_apart(shape: &Shape, clip: Rect, meshes: &mut Vec<(Rect, Arc<egui::Mesh>)>) {
        match shape {
            Shape::Vec(group) => {
                for shape in group {
                    take_apart(shape, clip, meshes);
                }
            }
            Shape::Mesh(mesh) => meshes.push((clip, mesh.clone())),
            _ => {}
        }
    }

    let mut found = Vec::new();
    for clipped in &output.shapes {
        take_apart(&clipped.shape, clipped.clip_rect, &mut found);
    }
    found
}

#[test]
fn a_tube_that_moves_is_picked_between_the_walls_it_is_painted_with() {
    let (snarl, wire) = pair(true);
    let mut world = World::new(snarl, Viewer::default());

    let mut scales = Vec::new();

    for zoom in [1.0, 0.4, 1.7] {
        let spot = empty_spot(&world.outer());
        world.zoom_at(spot, zoom);
        scales.push(world.outer().transform.scale());

        let mut bulges = Vec::new();

        for moment in [0.35, 0.5, 0.65] {
            world.viewer.tube = Some((wire, moment));
            world.move_to(spot);
            let output = world.settle();

            let anatomy = world.outer();
            let scale = anatomy.transform.scale();
            let measured = anatomy.wire(wire).expect("the wire was measured");
            let on_screen = measured.geometry.transformed(anatomy.transform.to_global);

            // The tube still runs from one measured pin centre to the other.
            assert_eq!(
                measured.geometry.start(),
                anatomy.out_pin(wire.out_pin).unwrap().graph_center
            );
            assert_eq!(
                measured.geometry.end(),
                anatomy.in_pin(wire.in_pin).unwrap().graph_center
            );

            // What is painted is the area between the walls of the measured path, as it is
            // at this moment, and the two walls themselves.
            let [near, far] = on_screen.walls();
            let found = meshes(&output);
            assert_eq!(found.len(), 1, "one ribbon");
            let (clip, mesh) = &found[0];
            assert_eq!(mesh.vertices.len(), 2 * near.len());
            for (index, pair) in mesh.vertices.chunks(2).enumerate() {
                assert_close(pair[0].pos, near[index], 0.05, "ribbon edge");
                assert_close(pair[1].pos, far[index], 0.05, "ribbon edge");
                assert_eq!(pair[0].color, TUBE_FILL);
            }
            assert_eq!(painted_along(&output, &near), 1, "one wall");
            assert_eq!(painted_along(&output, &far), 1, "the other wall");
            assert_eq!(painted_along(&output, on_screen.points()), 0, "no stroke");

            // It is clipped to the view, as any wire is.
            assert!(
                clip.min.distance(anatomy.transform.global_clip_rect.min) < 0.5
                    && clip.max.distance(anatomy.transform.global_clip_rect.max) < 0.5,
                "{clip:?} is not {:?}",
                anatomy.transform.global_clip_rect
            );

            // The pointer is on the tube anywhere inside the bulge, well beyond the band a
            // thin wire is picked in, and off it at the same distance where the tube is thin.
            let bulge = on_screen.anchor_at(moment);
            let reach = on_screen.radius_at(moment) - 1.5;
            assert!((on_screen.radius_at(moment) - 15.5 * scale).abs() < 0.2 * scale);
            assert!(
                reach > 6.0 + 1.0,
                "scale {scale}: the bulge is wider than the band"
            );

            let thin = on_screen.anchor_at(moment - 0.25);
            assert!(on_screen.radius_at(moment - 0.25) < 2.0 * scale);

            for side in [-1.0, 1.0] {
                world.move_to(bulge.pos + bulge.normal * (side * reach));
                let hovered = world.outer().hovered_wire().map(|wire| wire.pins);
                assert_eq!(hovered, Some(wire), "moment {moment}, scale {scale}");

                world.move_to(thin.pos + thin.normal * (side * reach));
                let hovered = world.outer().hovered_wire().map(|wire| wire.pins);
                assert_eq!(hovered, None, "moment {moment}, scale {scale}");
            }

            // Just outside the wall of the bulge the pointer is off the tube.
            world.move_to(bulge.pos + bulge.normal * (reach + 3.0));
            assert_eq!(world.outer().hovered_wire().map(|wire| wire.pins), None);

            bulges.push(bulge.pos + bulge.normal * reach);
        }

        // The bulge has moved on: where it was at the first moment, the tube is now thin
        // and bowed away, and the pointer is no longer on it.
        assert!(bulges[0].distance(bulges[2]) > 20.0);
        world.move_to(bulges[0]);
        assert_eq!(world.outer().hovered_wire().map(|wire| wire.pins), None);
        world.move_to(bulges[2]);
        assert_eq!(
            world.outer().hovered_wire().map(|wire| wire.pins),
            Some(wire)
        );
    }

    assert!(
        scales[1] < 0.9 && scales[2] > 1.1,
        "checked both reduced and magnified: {scales:?}"
    );

    // A click inside the bulge acts on the wire, at the point of its path nearest to it.
    let anatomy = world.outer();
    let on_screen = anatomy
        .wire(wire)
        .unwrap()
        .geometry
        .transformed(anatomy.transform.to_global);
    let bulge = on_screen.anchor_at(0.65);
    let inside = bulge.pos + bulge.normal * (on_screen.radius_at(0.65) - 1.5);

    world.click(inside, PointerButton::Primary);
    assert_eq!(world.viewer.actions.len(), 1);
    assert_eq!(world.viewer.actions[0].wire, wire);
    assert_eq!(world.viewer.actions[0].trigger, WireTrigger::Click);
    assert_close(
        world.viewer.actions[0].insertion.screen_pos,
        bulge.pos,
        1.0,
        "the point on the path under the click",
    );

    // The same graph without the tube paints no shape of the viewer's.
    world.viewer.tube = None;
    world.move_to(empty_spot(&world.outer()));
    let output = world.settle();
    assert!(meshes(&output).is_empty());
}

#[test]
fn shapes_a_viewer_adds_to_a_wire_are_painted_in_the_layer_of_that_wire() {
    for (style_layer, own_layer) in [
        (WireLayer::BehindNodes, None),
        (WireLayer::AboveNodes, None),
        (WireLayer::BehindNodes, Some(WireLayer::AboveNodes)),
        (WireLayer::AboveNodes, Some(WireLayer::BehindNodes)),
    ] {
        // A tube from the left node to the right one, passing the node in between.
        let mut snarl = Snarl::new();
        let left = snarl.insert_node(pos2(0.0, 0.0), Node::plain("left", 0, 1));
        let between = snarl.insert_node(pos2(190.0, 0.0), Node::plain("between", 1, 0));
        let right = snarl.insert_node(pos2(420.0, 0.0), Node::plain("right", 1, 0));

        let tube = WirePins::new(out_pin(left, 0), in_pin(right, 0));
        snarl.connect(tube.out_pin, tube.in_pin);

        let mut world = World::new(
            snarl,
            Viewer {
                tube: Some((tube, 0.5)),
                layered: own_layer.map(|layer| (tube, layer)),
                ..Viewer::default()
            },
        );
        world.style.wire_layer = Some(style_layer);

        let spot = empty_spot(&world.outer());
        world.move_to(spot);
        let output = world.settle();
        let anatomy = world.outer();
        let shapes = painted(&output);

        let frame = anatomy.node(between).unwrap().screen_rect;
        let node = shapes
            .iter()
            .position(|shape| {
                matches!(shape, Shape::Rect(rect)
                if rect.rect.min.distance(frame.min) < 0.5 && rect.rect.max.distance(frame.max) < 0.5)
            })
            .expect("the frame of the node in between was painted");

        let on_screen = anatomy
            .wire(tube)
            .unwrap()
            .geometry
            .transformed(anatomy.transform.to_global);
        assert!(
            on_screen
                .points()
                .iter()
                .any(|&point| frame.shrink(2.0).contains(point)),
            "the tube passes the node in between"
        );

        let [near, far] = on_screen.walls();
        let ribbon = shapes
            .iter()
            .position(|shape| matches!(shape, Shape::Mesh(_)))
            .expect("the ribbon was painted");
        let near = first_painted_along(&shapes, &near).expect("a wall was painted");
        let far = first_painted_along(&shapes, &far).expect("the other wall was painted");

        // Back to front, as the viewer added them.
        assert!(ribbon < near && near < far);

        let what = format!("style {style_layer:?}, wire {own_layer:?}");
        match own_layer.unwrap_or(style_layer) {
            WireLayer::BehindNodes => assert!(far < node, "{what}"),
            WireLayer::AboveNodes => assert!(ribbon > node, "{what}"),
        }

        // What the wire puts in front of the nodes is in front of them, and of the rest
        // of the wire, whichever layer the wire is in.
        let plug = shapes
            .iter()
            .position(|shape| matches!(shape, Shape::Circle(circle) if circle.fill == TUBE_PLUG))
            .expect("the plug was painted");
        let end = anatomy.in_pin(tube.in_pin).unwrap().screen_center;
        assert!(
            matches!(shapes[plug], Shape::Circle(circle) if circle.center.distance(end) < 0.5),
            "{what}: the plug sits on the pin the tube ends at"
        );

        let last_frame = [left, between, right]
            .into_iter()
            .map(|id| {
                let frame = anatomy.node(id).unwrap().screen_rect;
                shapes
                    .iter()
                    .rposition(|shape| {
                        matches!(shape, Shape::Rect(rect)
                            if rect.rect.min.distance(frame.min) < 0.5
                                && rect.rect.max.distance(frame.max) < 0.5)
                    })
                    .expect("the frame of every node was painted")
            })
            .max()
            .unwrap();
        assert!(plug > last_frame && plug > far, "{what}");
    }
}

#[test]
fn a_row_that_resizes_carries_its_pin_in_the_same_pass() {
    let (snarl, wire) = pair(true);
    let mut world = World::new(snarl, Viewer::default());
    let sink = wire.in_pin.node;
    let first = wire.in_pin;
    let second = in_pin(sink, 1);

    let before = world.outer();
    let first_before = *before.in_pin(first).unwrap();
    let second_before = *before.in_pin(second).unwrap();
    assert!(!before.node(sink).unwrap().remeasured);

    // The row gains widgets and its label wraps onto several lines.
    world.snarl[sink].label = "a much longer label that has to wrap over several lines".to_owned();
    world.snarl[sink].leading_widgets = 2;
    world.viewer.passes.clear();
    let _ = world.frame(Vec::new());

    let resized = Arc::clone(&world.viewer.passes[0]);
    let first_resized = *resized.in_pin(first).unwrap();

    assert!(resized.node(sink).unwrap().remeasured);
    assert!(
        first_resized.row_rect.height() > first_before.row_rect.height() * 2.0,
        "the row grew: {} -> {}",
        first_before.row_rect.height(),
        first_resized.row_rect.height()
    );

    // The pin sits on the row as measured in this very pass...
    assert!((first_resized.graph_center.y - first_resized.row_rect.center().y).abs() < 1e-3);
    assert!(
        first_resized
            .graph_rect
            .contains(first_resized.graph_center)
    );

    // ...which is not where the height remembered from the last pass would have put it.
    assert!(first_resized.graph_center.y - first_before.graph_center.y > 8.0);

    // The wire ends on the pin where it now is, in the same pass.
    let measured = resized.wire(wire).unwrap();
    assert_eq!(measured.geometry.end(), first_resized.graph_center);

    // The pin keeps its identity although the row now holds more widgets.
    assert_eq!(first_resized.widget_id, first_before.widget_id);
    assert_eq!(first_resized.widget_id, resized.view_ids.in_pin_id(first));
    assert_eq!(first_resized.pin, AnyPin::In(first));

    // Once the layout settles the rows below have moved down and nothing is left to remeasure.
    let _ = world.settle();
    let after = world.outer();
    assert!(!after.node(sink).unwrap().remeasured);
    assert!(after.in_pin(second).unwrap().graph_center.y > second_before.graph_center.y + 8.0);
    assert_eq!(
        after.in_pin(second).unwrap().widget_id,
        second_before.widget_id
    );
    assert_eq!(
        after.wire(wire).unwrap().geometry.end(),
        after.in_pin(first).unwrap().graph_center
    );

    // Shrinking back is measured the same way.
    world.snarl[sink].label = "sink".to_owned();
    world.snarl[sink].leading_widgets = 0;
    world.viewer.passes.clear();
    let _ = world.frame(Vec::new());
    let shrunk = *world.viewer.passes[0].in_pin(first).unwrap();
    assert!((shrunk.graph_center.y - shrunk.row_rect.center().y).abs() < 1e-3);
    assert!(shrunk.row_rect.height() < first_resized.row_rect.height() / 2.0);
}

#[test]
fn what_an_output_row_shows_stops_short_of_its_pin() {
    let (snarl, wire) = pair(true);
    let mut world = World::new(snarl, Viewer::default());
    let _ = world.settle();

    let anatomy = world.outer();
    let pin = *anatomy.out_pin(wire.out_pin).unwrap();
    let (_, label) = *world
        .viewer
        .output_labels
        .iter()
        .find(|(id, _)| *id == wire.out_pin)
        .unwrap();

    assert!(
        label.right() <= pin.graph_rect.left(),
        "the label ends at {} and the pin starts at {}",
        label.right(),
        pin.graph_rect.left()
    );
    assert!(!label.intersects(pin.graph_rect));
}

#[test]
fn pins_may_differ_in_size_and_sit_anywhere_on_their_row() {
    struct Sized;

    impl SnarlViewer<Node> for Sized {
        fn title(&mut self, node: &Node) -> String {
            node.label.clone()
        }

        fn inputs(&mut self, node: &Node) -> usize {
            node.inputs
        }

        fn outputs(&mut self, node: &Node) -> usize {
            node.outputs
        }

        fn show_input(
            &mut self,
            pin: &InPin,
            ui: &mut Ui,
            _snarl: &mut Snarl<Node>,
        ) -> impl SnarlPin + 'static {
            ui.add_sized(vec2(60.0, 48.0), egui::Label::new("in"));
            match pin.id.input {
                0 => PinInfo::circle().with_size(22.0).with_position(0.0),
                1 => PinInfo::circle().with_size(6.0).with_position(1.0),
                _ => PinInfo::circle(),
            }
        }

        fn show_output(
            &mut self,
            _pin: &OutPin,
            ui: &mut Ui,
            _snarl: &mut Snarl<Node>,
        ) -> impl SnarlPin + 'static {
            ui.label("out");
            PinInfo::circle()
        }
    }

    let mut snarl = Snarl::new();
    let node = snarl.insert_node(pos2(0.0, 0.0), Node::plain("sized", 3, 0));
    let ctx = Context::default();

    for frame in 0..6 {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(f64::from(frame) / 60.0),
            ..RawInput::default()
        };
        ctx.run_ui(input, |ui| {
            SnarlWidget::new()
                .id(view(OUTER))
                .style(style())
                .show(&mut snarl, &mut Sized, ui);
        })
        .drop_without_applying_deltas();
    }

    let anatomy = get_snarl_anatomy(view(OUTER), &ctx).unwrap();
    let top = anatomy.in_pin(in_pin(node, 0)).unwrap();
    let bottom = anatomy.in_pin(in_pin(node, 1)).unwrap();
    let middle = anatomy.in_pin(in_pin(node, 2)).unwrap();

    assert!((top.graph_rect.width() - 22.0).abs() < 1e-3);
    assert!((bottom.graph_rect.width() - 6.0).abs() < 1e-3);
    assert!(middle.graph_rect.width() > 6.0 && middle.graph_rect.width() < 22.0);

    assert!((top.graph_center.y - top.row_rect.top()).abs() < 1e-3);
    assert!((bottom.graph_center.y - bottom.row_rect.bottom()).abs() < 1e-3);
    assert!((middle.graph_center.y - middle.row_rect.center().y).abs() < 1e-3);
}

#[test]
fn a_view_inside_a_node_owns_its_pan_zoom_and_wires() {
    let (snarl, group, outer_wire, inner_wire) = nested();
    let mut world = World::new(snarl, nested_viewer());

    let outer = world.outer();
    let inner = world.inner();
    assert!(inner.embedded && !outer.embedded);

    // The inner view composes its own transform with the transform of the view it is in...
    assert_same_transform(
        inner.transform.to_global,
        outer.transform.to_global * inner.transform.to_parent,
        "composed transform",
    );
    assert_same_transform(
        inner.transform.parent_to_global,
        outer.transform.to_global,
        "parent transform",
    );

    // ...and is clipped to the node that holds it.
    let group_rect = outer.node(group).unwrap().screen_rect;
    assert!(
        group_rect
            .expand(0.5)
            .contains_rect(inner.transform.global_clip_rect)
    );
    assert!(
        (inner.transform.global_ui_rect.size() - INNER_SIZE * outer.transform.scale()).length()
            < 20.0
    );

    // Nothing in one view shares an id or a layer with the other.
    assert_ne!(inner.view_ids.layer_id, outer.view_ids.layer_id);
    let mut ids: Vec<Id> = [&outer, &inner]
        .into_iter()
        .flat_map(|anatomy| {
            anatomy
                .input_pins
                .values()
                .chain(anatomy.output_pins.values())
                .map(|pin| pin.widget_id)
                .chain(
                    anatomy
                        .nodes
                        .iter()
                        .map(|node| anatomy.view_ids.node_id(node.id)),
                )
                .chain(
                    anatomy
                        .wires
                        .iter()
                        .map(|wire| anatomy.view_ids.wire_id(wire.pins)),
                )
        })
        .collect();
    let total = ids.len();
    ids.sort_unstable_by_key(Id::value);
    ids.dedup();
    assert_eq!(ids.len(), total);
    assert_eq!(total, 5 + 4 + 2, "pins, nodes and wires of both views");

    // Zooming with the pointer over the inner view zooms the inner view only.
    let inside = empty_spot(&inner);
    world.zoom_at(inside, 0.7);
    let (outer_zoomed, inner_zoomed) = (world.outer(), world.inner());
    assert!(outer_zoomed.pointer_in_embedded_view);
    assert_eq!(outer_zoomed.transform.to_parent, outer.transform.to_parent);
    assert!(
        (inner_zoomed.transform.local_scale() - inner.transform.local_scale() * 0.7).abs() < 1e-3,
        "{} -> {}",
        inner.transform.local_scale(),
        inner_zoomed.transform.local_scale()
    );

    // The same holds on the very edge of the inner view, where the pointer is within reach of
    // the canvas under it.
    let visible = inner_zoomed.transform.global_clip_rect;
    let on_edge = pos2(visible.right() - 1.0, visible.center().y);
    world.zoom_at(on_edge, 1.0 / 0.7);
    assert_eq!(world.outer().transform.to_parent, outer.transform.to_parent);
    assert!(
        (world.inner().transform.local_scale() - inner.transform.local_scale()).abs() < 1e-3,
        "{}",
        world.inner().transform.local_scale()
    );
    world.zoom_at(inside, 1.0);
    world.zoom_at(inside, 0.7);
    let inner_zoomed = world.inner();

    // Dragging the inner canvas pans the inner view, by the drag as seen through the outer view.
    let drag = vec2(-40.0, -25.0);
    world.drag(inside, inside + drag);
    let (outer_panned, inner_panned) = (world.outer(), world.inner());
    assert_eq!(outer_panned.transform.to_parent, outer.transform.to_parent);
    let panned =
        inner_panned.transform.to_parent.translation - inner_zoomed.transform.to_parent.translation;
    assert!(
        (panned - drag / outer.transform.scale()).length() < 0.5,
        "panned by {panned:?}"
    );
    assert!(world.viewer.moved.is_empty(), "the group node stayed put");
    assert_eq!(
        world.snarl.get_node_info(group).unwrap().pos,
        pos2(0.0, 0.0)
    );

    // Zooming with the pointer over the outer canvas zooms the outer view only.
    let outside = empty_spot(&outer_panned);
    assert!(!inner_panned.transform.global_clip_rect.contains(outside));
    world.zoom_at(outside, 1.25);
    let (outer_scaled, inner_scaled) = (world.outer(), world.inner());
    assert!(!outer_scaled.pointer_in_embedded_view);
    assert!(
        (outer_scaled.transform.scale() - outer.transform.scale() * 1.25).abs() < 1e-3,
        "{} -> {}",
        outer.transform.scale(),
        outer_scaled.transform.scale()
    );
    assert_eq!(
        inner_scaled.transform.to_parent,
        inner_panned.transform.to_parent
    );
    assert_same_transform(
        inner_scaled.transform.to_global,
        outer_scaled.transform.to_global * inner_scaled.transform.to_parent,
        "composed transform after zooming the outer view",
    );

    // A wire of the inner view is picked by the inner view, never by the view under it.
    let inner_now = world.inner();
    let on_inner_wire = inner_now
        .wire(inner_wire)
        .unwrap()
        .geometry
        .transformed(inner_now.transform.to_global)
        .midpoint()
        .pos;
    assert!(inner_now.transform.global_clip_rect.contains(on_inner_wire));
    world.move_to(on_inner_wire);
    assert_eq!(
        world.inner().hovered_wire().map(|wire| wire.pins),
        Some(inner_wire)
    );
    assert!(world.outer().hovered_wire().is_none());

    // Acting on it changes the inner graph and leaves the outer graph alone.
    world.click(on_inner_wire, PointerButton::Secondary);
    let inner_viewer = world.viewer.inner.as_ref().unwrap();
    assert_eq!(inner_viewer.disconnects, 1);
    assert_eq!(inner_viewer.actions[0].wire, inner_wire);
    assert_eq!(
        world.snarl[group].inner.as_ref().unwrap().wires().count(),
        0
    );
    assert_eq!(world.viewer.disconnects, 0);
    assert!(world.viewer.actions.is_empty());
    assert_eq!(world.wires(), vec![outer_wire]);
}

#[test]
fn reconnect_select_move_and_zoom_keep_working_beside_wire_actions() {
    let (snarl, wire) = pair(false);
    let mut world = World::new(snarl, Viewer::default());
    let (source, sink) = (wire.out_pin.node, wire.in_pin.node);

    let connect = |world: &mut World| {
        let anatomy = world.outer();
        let from = anatomy.out_pin(wire.out_pin).unwrap().screen_center;
        let to = anatomy.in_pin(wire.in_pin).unwrap().screen_center;
        world.drag(from, to);
        let _ = world.settle();
        assert_eq!(world.wires(), vec![wire], "dragging pin to pin connects");
    };

    let on_wire = |world: &World| {
        let anatomy = world.outer();
        anatomy
            .wire(wire)
            .unwrap()
            .geometry
            .transformed(anatomy.transform.to_global)
            .midpoint()
            .pos
    };

    // Dragging from a pin still makes a wire, and the wire being there takes nothing from the pin.
    connect(&mut world);
    assert_eq!(world.viewer.connects, 1);
    assert!(world.viewer.actions.is_empty());

    // A click on the wire selects it and is reported. It does not disconnect anything.
    world.click(on_wire(&world), PointerButton::Primary);
    assert_eq!(get_selected_wire(view(OUTER), &world.ctx), Some(wire));
    assert_eq!(world.viewer.actions.len(), 1);
    assert_eq!(world.viewer.actions[0].trigger, WireTrigger::Click);
    assert_eq!(world.viewer.actions[0].wire, wire);
    assert!(world.outer().wire(wire).unwrap().selected);
    assert_eq!(world.wires(), vec![wire]);

    // The action says where on the wire it happened.
    let anatomy = world.outer();
    let reported = world.viewer.actions[0].anchor.pos;
    assert!(
        anatomy
            .wire(wire)
            .unwrap()
            .geometry
            .closest(reported)
            .distance
            < 1e-3
    );
    assert_close(
        world.viewer.actions[0].insertion.screen_pos,
        on_wire(&world),
        1.0,
        "action position",
    );

    // Delete acts on the selected wire.
    world.key(Key::Delete);
    assert!(world.wires().is_empty());
    assert_eq!(world.viewer.disconnects, 1);
    assert_eq!(world.viewer.actions[1].trigger, WireTrigger::Delete);
    let _ = world.settle();
    assert_eq!(get_selected_wire(view(OUTER), &world.ctx), None);

    // A secondary click still disconnects, as it did before wires had actions.
    connect(&mut world);
    world.click(on_wire(&world), PointerButton::Secondary);
    assert!(world.wires().is_empty());
    assert_eq!(world.viewer.disconnects, 2);
    assert_eq!(world.viewer.actions[2].trigger, WireTrigger::SecondaryClick);

    // With a wire menu, the same click opens the menu instead and the wire stays.
    connect(&mut world);
    world.viewer.wire_menu = true;
    let clicked = on_wire(&world);
    world.click(clicked, PointerButton::Secondary);
    assert_eq!(world.wires(), vec![wire]);
    assert_eq!(world.viewer.actions.len(), 3, "the menu took the click");

    let (menu_wire, anchor) = *world
        .viewer
        .wire_menu_anchors
        .first()
        .expect("the menu opened");
    assert_eq!(menu_wire, wire);
    assert_close(anchor.screen_pos, clicked, 1.0, "wire menu anchor");
    assert!(
        world
            .outer()
            .wire(wire)
            .unwrap()
            .geometry
            .closest(anchor.graph_pos)
            .distance
            < 1e-3
    );
    assert_eq!(world.outer().active_insertion_anchor, Some(anchor));

    // The anchor stays where the menu was opened while the pointer wanders off.
    world.move_to(clicked + vec2(60.0, -90.0));
    let _ = world.settle();
    assert!(world.viewer.wire_menu_anchors.len() > 3);
    assert!(
        world
            .viewer
            .wire_menu_anchors
            .iter()
            .all(|&(pins, later)| pins == wire && later == anchor)
    );

    // Escape closes the menu and leaves the wire alone.
    world.key(Key::Escape);
    let shown = world.viewer.wire_menu_anchors.len();
    let _ = world.settle();
    assert_eq!(world.viewer.wire_menu_anchors.len(), shown);
    assert_eq!(world.wires(), vec![wire]);

    // A wire can be chosen without a pointer, and Enter opens its menu at its midpoint.
    set_selected_wire(view(OUTER), &world.ctx, Some(wire));
    let _ = world.settle();
    world.key(Key::Enter);
    let _ = world.settle();
    let (_, keyboard_anchor) = *world.viewer.wire_menu_anchors.last().unwrap();
    assert_close(
        keyboard_anchor.graph_pos,
        world.outer().wire(wire).unwrap().geometry.midpoint().pos,
        1e-3,
        "keyboard menu anchor",
    );
    world.key(Key::Escape);
    world.viewer.wire_menu = false;

    // Dragging a node moves it, and its wire follows.
    let before = world.snarl.get_node_info(sink).unwrap().pos;
    let grab = world.outer().node(sink).unwrap().screen_rect.center_top() + vec2(0.0, 12.0);
    let drag = vec2(48.0, -36.0);
    world.viewer.moved.clear();
    world.drag(grab, grab + drag);
    let _ = world.settle();

    let scale = world.outer().transform.scale();
    let after = world.snarl.get_node_info(sink).unwrap().pos;
    assert_close(after, before + drag / scale, 1.0, "node position");
    assert!(
        world
            .viewer
            .moved
            .iter()
            .all(|(nodes, _, _)| nodes == &[sink])
    );
    assert!(
        world.viewer.moved.last().unwrap().2,
        "the move was reported as finished"
    );
    let anatomy = world.outer();
    assert_eq!(
        anatomy.wire(wire).unwrap().geometry.end(),
        anatomy.in_pin(wire.in_pin).unwrap().graph_center
    );
    assert_eq!(world.wires(), vec![wire], "moving a node keeps its wires");

    // Shift-click selects a node; a click on empty canvas clears both selections.
    let on_source = world.outer().node(source).unwrap().screen_rect.center_top() + vec2(0.0, 12.0);
    world.click_with(on_source, PointerButton::Primary, Modifiers::SHIFT);
    let _ = world.frame(vec![Event::PointerMoved(on_source)]);
    assert_eq!(get_selected_nodes(view(OUTER), &world.ctx), vec![source]);
    assert_eq!(world.outer().selected_nodes, vec![source]);

    world.click(on_wire(&world), PointerButton::Primary);
    assert_eq!(get_selected_wire(view(OUTER), &world.ctx), Some(wire));
    world.click(empty_spot(&world.outer()), PointerButton::Primary);
    let _ = world.settle();
    assert_eq!(get_selected_wire(view(OUTER), &world.ctx), None);
    assert!(get_selected_nodes(view(OUTER), &world.ctx).is_empty());

    // Zooming keeps every measured pin under the transform that was applied.
    let before = world.outer().transform.scale();
    world.zoom_at(empty_spot(&world.outer()), 0.75);
    let anatomy = world.outer();
    assert!((anatomy.transform.scale() - before * 0.75).abs() < 1e-3);
    for pin in anatomy
        .input_pins
        .values()
        .chain(anatomy.output_pins.values())
    {
        assert_close(
            pin.screen_center,
            anatomy.transform.to_global * pin.graph_center,
            1e-3,
            "pin on screen",
        );
    }
    assert_eq!(world.wires(), vec![wire]);
}

#[test]
fn a_finger_held_on_a_wire_acts_on_it_and_a_finger_dragged_still_pans() {
    let (snarl, wire) = pair(true);
    let mut world = World::new(snarl, Viewer::default());
    let _ = world.settle();

    let on_wire = |world: &World| {
        let anatomy = world.outer();
        anatomy
            .wire(wire)
            .unwrap()
            .geometry
            .transformed(anatomy.transform.to_global)
            .midpoint()
            .pos
    };

    // A tap selects the wire, although no pointer is left once the finger is lifted.
    let tapped = on_wire(&world);
    world.tap(tapped);

    let triggers: Vec<WireTrigger> = world.viewer.actions.iter().map(|a| a.trigger).collect();
    assert_eq!(triggers, vec![WireTrigger::Click]);
    assert_eq!(get_selected_wire(view(OUTER), &world.ctx), Some(wire));
    assert_close(
        world.viewer.actions[0].insertion.screen_pos,
        tapped,
        1.0,
        "where the finger tapped",
    );
    world.viewer.actions.clear();

    // Without a wire menu a hold is reported as its own trigger, once, and the wire stays.
    let held = on_wire(&world);
    world.long_press(held);

    let triggers: Vec<WireTrigger> = world.viewer.actions.iter().map(|a| a.trigger).collect();
    assert_eq!(triggers, vec![WireTrigger::LongTouch]);
    assert_eq!(world.viewer.actions[0].wire, wire);
    assert_close(
        world.viewer.actions[0].insertion.screen_pos,
        held,
        1.0,
        "where the finger was held",
    );
    assert_eq!(world.wires(), vec![wire]);
    assert_eq!(world.viewer.disconnects, 0);
    assert!(world.viewer.moved.is_empty());

    // With a wire menu the hold opens the menu on the wire instead.
    world.viewer.wire_menu = true;
    let held = on_wire(&world);
    world.long_press(held);

    assert_eq!(world.viewer.actions.len(), 1, "the menu took the hold");
    let (menu_wire, anchor) = *world
        .viewer
        .wire_menu_anchors
        .first()
        .expect("the menu opened");
    assert_eq!(menu_wire, wire);
    assert_close(anchor.screen_pos, held, 1.0, "wire menu anchor");
    assert_eq!(world.wires(), vec![wire]);

    // A finger dragged across the canvas, starting on the wire, still pans the view.
    world.key(Key::Escape);
    let _ = world.settle();
    let before = world.outer().transform.to_global;
    let from = on_wire(&world);
    let by = vec2(60.0, 35.0);

    world.touch(TouchPhase::Start, from);
    for step in 1..=6 {
        #[allow(clippy::cast_precision_loss)]
        world.touch(TouchPhase::Move, from + by * (step as f32 / 6.0));
    }
    world.touch(TouchPhase::End, from + by);
    let _ = world.settle();

    let after = world.outer().transform.to_global;
    assert_close(
        after.translation.to_pos2(),
        (before.translation + by).to_pos2(),
        1.0,
        "the view followed the finger",
    );
    assert!((after.scaling - before.scaling).abs() < 1e-4);
    assert_eq!(world.wires(), vec![wire]);
    assert_eq!(world.viewer.actions.len(), 1, "a drag is not a wire action");
}

#[test]
fn a_finger_dragged_from_pin_to_pin_connects_them() {
    let (snarl, wire) = pair(false);
    let mut world = World::new(snarl, Viewer::default());
    let _ = world.settle();

    let anatomy = world.outer();
    let from = anatomy.out_pin(wire.out_pin).unwrap().screen_center;
    let to = anatomy.in_pin(wire.in_pin).unwrap().screen_center;

    world.touch(TouchPhase::Start, from);
    for step in 1..=6 {
        #[allow(clippy::cast_precision_loss)]
        world.touch(TouchPhase::Move, from.lerp(to, step as f32 / 6.0));
    }
    world.touch(TouchPhase::End, to);
    let _ = world.settle();

    assert_eq!(world.wires(), vec![wire]);
    assert_eq!(world.viewer.connects, 1);
    assert!(world.viewer.actions.is_empty());
}

#[test]
fn the_graph_menu_inserts_where_it_was_opened_even_when_the_popup_is_moved() {
    let (snarl, _) = pair(true);
    let mut world = World::new(
        snarl,
        Viewer {
            graph_menu: true,
            ..Viewer::default()
        },
    );

    // Open the menu so close to the corner of the screen that the popup cannot hang below
    // and to the right of the pointer.
    let clicked = SCREEN.to_pos2() - vec2(14.0, 14.0);
    world.click(clicked, PointerButton::Secondary);
    let _ = world.settle();

    let anatomy = world.outer();
    let expected = anatomy.transform.from_global * clicked;
    let corner = *world
        .viewer
        .graph_menu_popup_corners
        .last()
        .expect("the menu opened");

    assert!(
        clicked.x - corner.x > 150.0 && clicked.y - corner.y > 100.0,
        "the popup was moved away from the pointer: {corner:?}"
    );
    assert_close(
        *world.viewer.graph_menu_positions.last().unwrap(),
        expected,
        1e-2,
        "insertion point",
    );
    assert_eq!(
        anatomy
            .active_insertion_anchor
            .map(|anchor| anchor.graph_pos),
        world.viewer.graph_menu_positions.last().copied()
    );

    // Opening a section inside the popup neither closes it nor moves the insertion point.
    let section = world.viewer.graph_menu_section.unwrap().center();
    let shown = world.viewer.graph_menu_positions.len();
    world.click(section, PointerButton::Primary);
    world.move_to(pos2(300.0, 500.0));
    let _ = world.settle();

    assert!(
        world.viewer.graph_menu_positions.len() > shown + 5,
        "the menu stayed open"
    );
    for &pos in &world.viewer.graph_menu_positions {
        assert_close(
            pos,
            expected,
            1e-2,
            "insertion point while the menu is open",
        );
    }
}

#[test]
fn collapsing_a_group_keeps_its_wires_and_the_graph_inside_it() {
    let (snarl, group, outer_wire, inner_wire) = nested();
    let mut world = World::new(snarl, nested_viewer());

    let open = world.outer();
    let open_pin = *open.out_pin(outer_wire.out_pin).unwrap();
    assert!(world.viewer.inner.as_ref().unwrap().passes.len() > 2);

    world.snarl.open_node(group, false);
    for _ in 0..40 {
        let _ = world.frame(Vec::new());
    }
    let _ = world.settle();

    // Collapsed: the exposed pins are the same pins, measured where they are now drawn.
    let closed = world.outer();
    let closed_node = closed.node(group).unwrap();
    assert!(!closed_node.open);
    assert!(closed_node.final_rect.height() < open.node(group).unwrap().final_rect.height() / 2.0);

    let closed_pin = *closed.out_pin(outer_wire.out_pin).unwrap();
    assert_eq!(closed_pin.widget_id, open_pin.widget_id);
    assert_eq!(closed_pin.pin, open_pin.pin);
    assert!(
        closed_node
            .final_rect
            .expand(12.0)
            .contains(closed_pin.graph_center)
    );

    let wire = closed.wire(outer_wire).expect("the wire is still shown");
    assert_eq!(wire.geometry.start(), closed_pin.graph_center);
    assert_eq!(world.wires(), vec![outer_wire]);

    // The graph inside is neither shown nor touched.
    let inner_graph = world.snarl[group].inner.as_ref().unwrap();
    assert_eq!(
        inner_graph.wires().collect::<Vec<_>>(),
        vec![(inner_wire.out_pin, inner_wire.in_pin)]
    );
    assert_eq!(inner_graph.node_ids().count(), 2);

    // Expanded again: same pins, same wires, and the inner view is back.
    world.snarl.open_node(group, true);
    world.viewer.inner.as_mut().unwrap().passes.clear();
    for _ in 0..40 {
        let _ = world.frame(Vec::new());
    }
    let _ = world.settle();

    let reopened = world.outer();
    let reopened_pin = *reopened.out_pin(outer_wire.out_pin).unwrap();
    assert_eq!(reopened_pin.widget_id, open_pin.widget_id);
    assert_close(
        reopened_pin.graph_center,
        open_pin.graph_center,
        0.5,
        "pin after reopening",
    );
    assert_eq!(world.wires(), vec![outer_wire]);
    assert!(world.inner().wire(inner_wire).is_some());
    assert!(!world.viewer.inner.as_ref().unwrap().passes.is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn a_saved_graph_comes_back_with_the_same_endpoints() {
    fn endpoints(snarl: &Snarl<Node>) -> Vec<(OutPinId, InPinId)> {
        let mut wires: Vec<_> = snarl.wires().collect();
        wires.sort_unstable();
        wires
    }

    fn layout(snarl: &Snarl<Node>) -> Vec<(NodeId, Pos2, bool, String)> {
        let mut nodes: Vec<_> = snarl
            .nodes_ids_data()
            .map(|(id, node)| (id, node.pos, node.open, node.value.label.clone()))
            .collect();
        nodes.sort_unstable_by_key(|(id, ..)| *id);
        nodes
    }

    let (mut snarl, group, outer_wire, inner_wire) = nested();
    snarl.open_node(group, false);

    let saved = serde_json::to_string(&snarl).unwrap();
    let loaded: Snarl<Node> = serde_json::from_str(&saved).unwrap();

    assert_eq!(endpoints(&loaded), endpoints(&snarl));
    assert_eq!(
        endpoints(&loaded),
        vec![(outer_wire.out_pin, outer_wire.in_pin)]
    );
    assert_eq!(layout(&loaded), layout(&snarl));
    assert!(!loaded.get_node_info(group).unwrap().open);

    let inner = loaded[group].inner.as_ref().unwrap();
    let inner_before = snarl[group].inner.as_ref().unwrap();
    assert_eq!(
        endpoints(inner),
        vec![(inner_wire.out_pin, inner_wire.in_pin)]
    );
    assert_eq!(layout(inner), layout(inner_before));

    // Saving what was loaded gives the same document: nothing the view measured leaks into it.
    assert_eq!(serde_json::to_string(&loaded).unwrap(), saved);

    // Showing the loaded graph measures the same wire between the same pins.
    let mut world = World::new(loaded, nested_viewer());
    let _ = world.settle();
    assert!(world.outer().wire(outer_wire).is_some());
    assert_eq!(serde_json::to_string(&world.snarl).unwrap(), saved);
}
