//! A graph whose viewer and style ask for nothing new keeps behaving as it did before wires
//! could be picked, routed and painted by the viewer. Each new behaviour is checked off by
//! default and on when its style flag is set.

use std::sync::Arc;

use egui::{
    Context, Event, FullOutput, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape,
    TouchDeviceId, TouchId, TouchPhase, Ui, Vec2, pos2, vec2,
};
use egui_snarl::{
    InPin, InPinId, NodeId, OutPin, OutPinId, Snarl,
    ui::{
        PinInfo, SnarlAnatomy, SnarlPin, SnarlStyle, SnarlViewer, SnarlWidget, WireLayer, WirePins,
        WireStyle, get_selected_wire, get_snarl_anatomy,
    },
};

const SCREEN: Vec2 = vec2(1000.0, 700.0);

fn view() -> Id {
    Id::unique("plain graph")
}

struct Node {
    label: &'static str,
    inputs: usize,
    outputs: usize,
}

/// Implements what a viewer must, and a menu of the canvas when asked to. Wires are left to
/// the defaults.
#[derive(Default)]
struct Viewer {
    graph_menu: bool,
    graph_menu_shown: bool,

    /// Where the latest output row put its label, in graph coordinates.
    output_label: Option<Rect>,
}

impl SnarlViewer<Node> for Viewer {
    fn title(&mut self, node: &Node) -> String {
        node.label.to_owned()
    }

    fn inputs(&mut self, node: &Node) -> usize {
        node.inputs
    }

    fn outputs(&mut self, node: &Node) -> usize {
        node.outputs
    }

    fn show_input(
        &mut self,
        _pin: &InPin,
        ui: &mut Ui,
        _snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        ui.label("in");
        PinInfo::square()
    }

    fn show_output(
        &mut self,
        _pin: &OutPin,
        ui: &mut Ui,
        _snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        self.output_label = Some(ui.label("out").rect);
        PinInfo::circle()
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<Node>) -> bool {
        self.graph_menu
    }

    fn show_graph_menu(
        &mut self,
        _pos: Pos2,
        _selected_nodes: &[NodeId],
        ui: &mut Ui,
        _snarl: &mut Snarl<Node>,
    ) {
        self.graph_menu_shown = true;
        ui.label("Menu");
    }
}

struct World {
    ctx: Context,
    time: f64,
    snarl: Snarl<Node>,
    viewer: Viewer,
    style: SnarlStyle,
    wire: WirePins,

    /// Whether the application still saw Delete pressed after the graph was shown.
    delete_left_for_the_application: bool,
}

impl World {
    /// Two nodes with one wire between them, far enough apart that the middle of the wire is
    /// over empty canvas, or under a third node when `covered`.
    fn new(style: SnarlStyle, covered: bool) -> Self {
        let mut snarl = Snarl::new();
        let source = snarl.insert_node(
            pos2(0.0, 0.0),
            Node {
                label: "source",
                inputs: 0,
                outputs: 1,
            },
        );
        let sink = snarl.insert_node(
            pos2(320.0, 170.0),
            Node {
                label: "sink",
                inputs: 1,
                outputs: 0,
            },
        );
        let wire = WirePins::new(
            OutPinId {
                node: source,
                output: 0,
            },
            InPinId {
                node: sink,
                input: 0,
            },
        );
        let _ = snarl.connect(wire.out_pin, wire.in_pin);

        let mut world = Self {
            ctx: Context::default(),
            time: 0.0,
            snarl,
            viewer: Viewer::default(),
            style,
            wire,
            delete_left_for_the_application: false,
        };
        world.settle();

        if covered {
            let middle = world.anatomy().wire(wire).unwrap().midpoint().pos;
            let _ = world.snarl.insert_node(
                middle - vec2(30.0, 20.0),
                Node {
                    label: "cover",
                    inputs: 0,
                    outputs: 0,
                },
            );
            world.settle();
            assert!(
                world.anatomy().is_occluded_by_node(middle),
                "the third node lies over the middle of the wire"
            );
        }

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
            delete_left_for_the_application,
            ..
        } = self;

        let mut output = ctx.run_ui(input, |ui| {
            SnarlWidget::new()
                .id(view())
                .style(*style)
                .show(snarl, viewer, ui);

            if ui.input(|input| input.key_pressed(Key::Delete)) {
                *delete_left_for_the_application = true;
            }
        });
        output.textures_delta.clear();
        output
    }

    fn settle(&mut self) -> FullOutput {
        for _ in 0..6 {
            let _ = self.frame(Vec::new());
        }
        self.frame(Vec::new())
    }

    fn anatomy(&self) -> Arc<SnarlAnatomy> {
        get_snarl_anatomy(view(), &self.ctx).expect("the view has been shown")
    }

    /// The middle of the wire, on screen.
    fn on_wire(&self) -> Pos2 {
        let anatomy = self.anatomy();
        anatomy
            .transform
            .graph_to_screen_pos(anatomy.wire(self.wire).unwrap().midpoint().pos)
    }

    fn has_wire(&self) -> bool {
        self.snarl.wires().count() == 1
    }

    fn move_to(&mut self, pos: Pos2) {
        let _ = self.frame(vec![Event::PointerMoved(pos)]);
        let _ = self.frame(Vec::new());
    }

    fn click(&mut self, pos: Pos2, button: PointerButton) {
        self.move_to(pos);
        for pressed in [true, false] {
            let _ = self.frame(vec![Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            }]);
        }
        let _ = self.frame(Vec::new());
    }

    fn pointer_gone(&mut self) -> FullOutput {
        let _ = self.frame(vec![Event::PointerGone]);
        self.settle()
    }

    fn touch(&mut self, phase: TouchPhase, pos: Pos2) {
        let mut events = vec![Event::Touch {
            device_id: TouchDeviceId(0),
            id: TouchId(0),
            phase,
            pos,
            force: None,
        }];

        let pressed = matches!(phase, TouchPhase::Start);
        if pressed {
            events.push(Event::PointerMoved(pos));
        }
        events.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        if !pressed {
            events.push(Event::PointerGone);
        }

        let _ = self.frame(events);
    }

    fn tap(&mut self, pos: Pos2) {
        self.touch(TouchPhase::Start, pos);
        let _ = self.frame(Vec::new());
        self.touch(TouchPhase::End, pos);
        let _ = self.frame(Vec::new());
    }

    fn hold(&mut self, pos: Pos2) {
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

    fn zoom_at(&mut self, pos: Pos2, factor: f32) {
        self.move_to(pos);
        let _ = self.frame(vec![Event::Zoom(factor)]);
        let _ = self.settle();
    }
}

fn plain() -> SnarlStyle {
    SnarlStyle {
        // A double click must not move the view under the pointer in these tests.
        centering: Some(false),
        ..SnarlStyle::new()
    }
}

fn picking() -> SnarlStyle {
    SnarlStyle {
        wire_picking: Some(true),
        ..plain()
    }
}

/// What a pass paints, without the wrapping of layers.
fn painted(output: &FullOutput) -> Vec<Shape> {
    fn take_apart(shape: &Shape, shapes: &mut Vec<Shape>) {
        match shape {
            Shape::Noop => {}
            Shape::Vec(inner) => {
                for shape in inner {
                    take_apart(shape, shapes);
                }
            }
            shape => shapes.push(shape.clone()),
        }
    }

    let mut shapes = Vec::new();
    for clipped in &output.shapes {
        take_apart(&clipped.shape, &mut shapes);
    }
    shapes
}

#[test]
fn a_click_a_tap_and_a_double_click_on_a_wire_leave_a_plain_graph_as_it_was() {
    let mut world = World::new(plain(), false);
    let at_rest = painted(&world.pointer_gone());
    let on_wire = world.on_wire();

    world.click(on_wire, PointerButton::Primary);
    assert_eq!(get_selected_wire(view(), &world.ctx), None, "after a click");
    assert_eq!(painted(&world.pointer_gone()), at_rest, "after a click");

    world.tap(on_wire);
    assert_eq!(get_selected_wire(view(), &world.ctx), None, "after a tap");
    assert_eq!(painted(&world.pointer_gone()), at_rest, "after a tap");

    world.click(on_wire, PointerButton::Primary);
    world.click(on_wire, PointerButton::Primary);
    assert_eq!(
        get_selected_wire(view(), &world.ctx),
        None,
        "after a double click"
    );
    assert_eq!(
        painted(&world.pointer_gone()),
        at_rest,
        "after a double click"
    );
    assert!(world.has_wire());

    // With picking on, the same click selects the wire and the selection is painted.
    let mut world = World::new(picking(), false);
    let at_rest = painted(&world.pointer_gone());
    let on_wire = world.on_wire();

    world.click(on_wire, PointerButton::Primary);
    assert_eq!(get_selected_wire(view(), &world.ctx), Some(world.wire));
    assert_ne!(painted(&world.pointer_gone()), at_rest);
}

#[test]
fn a_plain_graph_takes_no_key_for_a_wire() {
    let mut world = World::new(plain(), false);
    let on_wire = world.on_wire();

    world.click(on_wire, PointerButton::Primary);
    for key in [Key::Delete, Key::Backspace, Key::Enter, Key::Space] {
        world.key(key);
    }

    assert!(world.has_wire(), "no key removes a wire of a plain graph");
    assert!(
        world.delete_left_for_the_application,
        "the key is left for whatever is shown beside the graph"
    );

    // With picking on, Delete goes to the selected wire, and by default removes it.
    let mut world = World::new(picking(), false);
    let on_wire = world.on_wire();

    world.click(on_wire, PointerButton::Primary);
    world.key(Key::Delete);

    assert!(!world.has_wire());
    assert!(!world.delete_left_for_the_application);
}

#[test]
fn a_secondary_click_removes_a_wire_also_where_a_node_lies_over_it() {
    let mut world = World::new(plain(), false);
    let on_wire = world.on_wire();
    world.click(on_wire, PointerButton::Secondary);
    assert!(!world.has_wire(), "over empty canvas");

    for layer in [WireLayer::BehindNodes, WireLayer::AboveNodes] {
        let style = SnarlStyle {
            wire_layer: Some(layer),
            ..plain()
        };

        let mut world = World::new(style, true);
        let on_wire = world.on_wire();

        world.move_to(on_wire);
        assert!(
            world.anatomy().wire(world.wire).unwrap().hovered,
            "{layer:?}: the wire is hovered through the node over it"
        );

        world.click(on_wire, PointerButton::Secondary);
        assert!(!world.has_wire(), "{layer:?}: under a node");

        // A style may ask for nodes to take the pointer from the wires they cover.
        let style = SnarlStyle {
            nodes_cover_wires: Some(true),
            ..style
        };

        let mut world = World::new(style, true);
        let on_wire = world.on_wire();

        world.move_to(on_wire);
        assert!(!world.anatomy().wire(world.wire).unwrap().hovered);

        world.click(on_wire, PointerButton::Secondary);
        assert!(world.has_wire(), "{layer:?}: the node took the click");
    }
}

#[test]
fn a_finger_held_on_a_wire_of_a_plain_graph_opens_the_menu_of_the_canvas() {
    let mut world = World::new(plain(), false);
    world.viewer.graph_menu = true;
    let on_wire = world.on_wire();

    world.hold(on_wire);
    let _ = world.settle();

    assert!(world.viewer.graph_menu_shown);
    assert!(world.has_wire());

    // With picking on, the held touch is reported for the wire instead.
    let mut world = World::new(picking(), false);
    world.viewer.graph_menu = true;
    let on_wire = world.on_wire();

    world.hold(on_wire);
    let _ = world.settle();

    assert!(!world.viewer.graph_menu_shown);
    assert!(world.has_wire());
}

#[test]
fn wire_smoothness_is_a_graph_distance_unless_a_style_asks_for_the_screen() {
    let points = |world: &World| {
        world
            .anatomy()
            .wire(world.wire)
            .unwrap()
            .geometry
            .points()
            .len()
    };

    let mut world = World::new(plain(), false);
    let at_rest = points(&world);
    let on_wire = world.on_wire();
    world.zoom_at(on_wire, 1.6);
    world.zoom_at(on_wire, 1.6);

    assert!(world.anatomy().transform.to_global.scaling > 1.5);
    assert_eq!(points(&world), at_rest);

    let style = SnarlStyle {
        wire_smoothness_on_screen: Some(true),
        ..plain()
    };
    let mut world = World::new(style, false);
    let at_rest = points(&world);
    let on_wire = world.on_wire();
    world.zoom_at(on_wire, 1.6);
    world.zoom_at(on_wire, 1.6);

    assert!(world.anatomy().transform.to_global.scaling > 1.5);
    assert!(points(&world) > at_rest);
}

#[test]
fn a_straight_wire_is_one_segment_and_corners_are_drawn_finely() {
    let style = SnarlStyle {
        wire_style: Some(WireStyle::Line),
        ..plain()
    };
    let mut world = World::new(style, false);
    let output = world.pointer_gone();

    let anatomy = world.anatomy();
    let geometry = &anatomy.wire(world.wire).unwrap().geometry;
    let ends = [
        anatomy.transform.graph_to_screen_pos(geometry.start()),
        anatomy.transform.graph_to_screen_pos(geometry.end()),
    ];

    let segments: Vec<[Pos2; 2]> = painted(&output)
        .iter()
        .filter_map(|shape| match shape {
            Shape::LineSegment { points, .. } => Some(*points),
            _ => None,
        })
        .filter(|points| {
            points[0].distance(ends[0]) < 0.5 && points[1].distance(ends[1]) < 0.5
                || points[0].distance(ends[1]) < 0.5 && points[1].distance(ends[0]) < 0.5
        })
        .collect();
    assert_eq!(segments.len(), 1, "one segment from pin to pin");

    // However coarse the smoothness asked for, the corners of a wire with turns stay round.
    let style = SnarlStyle {
        wire_style: Some(WireStyle::AxisAligned {
            corner_radius: 12.0,
        }),
        wire_smoothness: Some(10.0),
        ..plain()
    };
    let world = World::new(style, false);
    let anatomy = world.anatomy();
    let points = anatomy.wire(world.wire).unwrap().geometry.points().len();
    assert!(points > 100, "{points} points along a wire with two turns");
}

#[test]
fn an_output_row_is_laid_out_as_before_unless_a_style_asks_for_room_around_the_pin() {
    let measure = |style: SnarlStyle| {
        let world = World::new(style, false);
        let anatomy = world.anatomy();
        let pin = anatomy.out_pin(world.wire.out_pin).unwrap().graph_rect;
        let node = anatomy.node(world.wire.out_pin.node).unwrap().final_rect;
        (world.viewer.output_label.unwrap(), pin, node)
    };

    let (label, pin, node) = measure(plain());
    assert!(
        label.right() > pin.left() && label.right() < node.right(),
        "the label ends under the pin, at {}; the pin is {pin:?}",
        label.right()
    );

    let (clear_label, clear_pin, clear_node) = measure(SnarlStyle {
        output_rows_clear_pins: Some(true),
        ..plain()
    });
    assert_eq!(clear_pin, pin, "the pin stays where it was");
    assert_eq!(clear_node, node, "the node is as large as it was");
    assert!(
        clear_label.right() <= clear_pin.left(),
        "the label ends at {} and the pin starts at {}",
        clear_label.right(),
        clear_pin.left()
    );
    assert!(clear_label.right() < label.right());
}
