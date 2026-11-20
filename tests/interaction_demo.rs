//! Runs the graph of the `interaction` example through headless egui passes.

#![cfg(feature = "serde")]

#[allow(dead_code)]
#[path = "../examples/interaction/graph.rs"]
mod graph;

use egui::{
    Color32, Context, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape,
    Stroke, epaint::CubicBezierShape, vec2,
};
use egui_snarl::{
    InPinId, NodeId, OutPinId, Snarl,
    ui::{SnarlAnatomy, WirePins, get_snarl_anatomy},
};
use graph::{Block, Document, Viewer};

const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1280.0, 800.0));

fn endpoints(snarl: &Snarl<Block>) -> Vec<(OutPinId, InPinId)> {
    let mut wires: Vec<_> = snarl.wires().collect();
    wires.sort();
    wires
}

fn group(document: &Document) -> (NodeId, &Snarl<Block>) {
    document
        .graph
        .node_ids()
        .find_map(|(id, block)| match block {
            Block::Group { graph, .. } => Some((id, graph.as_ref())),
            _ => None,
        })
        .expect("the fixture holds a group")
}

/// One pass of the example's graph, `seconds` after the last one.
fn pass(
    ctx: &Context,
    document: &mut Document,
    viewer: &mut Viewer,
    seconds: f64,
    events: Vec<Event>,
) -> FullOutput {
    let input = RawInput {
        screen_rect: Some(SCREEN),
        time: Some(ctx.input(|input| input.time) + seconds),
        predicted_dt: 1.0 / 60.0,
        focused: true,
        events,
        ..RawInput::default()
    };

    let mut output = ctx.run_ui(input, |ui| {
        graph::show(document, viewer, ui);
    });

    // Nothing is rendered here, so the font atlas has nowhere to go.
    output.textures_delta.clear();
    output
}

fn show(ctx: &Context, document: &mut Document, viewer: &mut Viewer, passes: usize) {
    for _ in 0..passes {
        let _ = pass(ctx, document, viewer, 1.0 / 60.0, Vec::new());
    }
}

#[test]
fn the_shipped_fixture_is_the_state_the_example_seeds() {
    // Regenerate with:
    // cargo run --example interaction --features serde -- --write-fixture examples/interaction/fixture.json
    assert_eq!(Document::seed().to_json(), graph::FIXTURE);
}

#[test]
fn the_fixture_is_measured_as_saved_and_comes_back_the_same() {
    let mut document = Document::from_json(graph::FIXTURE).unwrap();
    let mut viewer = Viewer::new(document.settings);
    viewer.fit();

    let ctx = Context::default();
    show(&ctx, &mut document, &mut viewer, 8);

    let saved_wires = endpoints(&document.graph);
    let (group_node, inner) = group(&document);
    let saved_inner_wires = endpoints(inner);

    // Every saved wire was laid out between two measured pins, in the top-level view and
    // in the view inside the group node.
    let outer = get_snarl_anatomy(graph::root_view_id(), &ctx).unwrap();
    let embedded = get_snarl_anatomy(graph::embedded_view_id(group_node), &ctx).unwrap();

    let measured = |wires: &[egui_snarl::ui::MeasuredWire]| -> Vec<(OutPinId, InPinId)> {
        wires
            .iter()
            .map(|wire| (wire.pins.out_pin, wire.pins.in_pin))
            .collect()
    };
    assert_eq!(measured(&outer.wires), saved_wires);
    assert_eq!(measured(&embedded.wires), saved_inner_wires);
    assert!(!outer.embedded);
    assert!(embedded.embedded);

    for wire in outer.wires.iter().chain(&embedded.wires) {
        let anatomy = if outer.wires.iter().any(|other| std::ptr::eq(other, wire)) {
            &outer
        } else {
            &embedded
        };
        let from = anatomy.out_pin(wire.pins.out_pin).unwrap();
        let to = anatomy.in_pin(wire.pins.in_pin).unwrap();

        assert_eq!(wire.geometry.start(), from.graph_center);
        assert_eq!(wire.geometry.end(), to.graph_center);
    }

    // The view inside the group is clipped to the group node.
    let group_rect = outer.node(group_node).unwrap().screen_rect;
    assert!(group_rect.contains_rect(embedded.transform.global_clip_rect));

    // Closing the group keeps the wires on its ports and leaves its graph alone.
    let through_group: Vec<WirePins> = outer
        .wires
        .iter()
        .map(|wire| wire.pins)
        .filter(|pins| pins.out_pin.node == group_node || pins.in_pin.node == group_node)
        .collect();
    assert_eq!(through_group.len(), 2);

    document.graph.open_node(group_node, false);
    show(&ctx, &mut document, &mut viewer, 30);

    let closed = get_snarl_anatomy(graph::root_view_id(), &ctx).unwrap();
    assert!(!closed.node(group_node).unwrap().open);
    for pins in &through_group {
        assert!(closed.wire(*pins).is_some(), "{pins:?} is still shown");
    }
    assert_eq!(endpoints(&document.graph), saved_wires);
    assert_eq!(endpoints(group(&document).1), saved_inner_wires);

    // Opened again and asked to frame its nodes, the view inside the group shows all of
    // them, although the group was still opening when it was asked.
    document.graph.open_node(group_node, true);
    viewer.fit();
    show(&ctx, &mut document, &mut viewer, 90);

    let reopened = get_snarl_anatomy(graph::embedded_view_id(group_node), &ctx).unwrap();
    assert_eq!(reopened.nodes.len(), 4);
    for node in &reopened.nodes {
        assert!(
            reopened.transform.viewport.contains_rect(node.final_rect),
            "{:?} at {:?} is inside {:?}",
            node.id,
            node.final_rect,
            reopened.transform.viewport
        );
    }

    // What is saved after all that reads back with the same wires and the same nodes.
    let restored = Document::from_json(&document.to_json()).unwrap();
    assert_eq!(endpoints(&restored.graph), saved_wires);
    assert_eq!(endpoints(group(&restored).1), saved_inner_wires);
    assert_eq!(
        restored.graph.node_ids().count(),
        document.graph.node_ids().count()
    );
    assert!(restored.settings == document.settings);
}

/// What `paint_energy` gave its painter for one tube: four strokes, then three discs.
struct OverlayTube {
    wire: WirePins,
    shapes: Vec<Shape>,
}

/// The painter a product laid over its canvas, with what it painted collected instead.
///
/// The body is `paint_energy` from `crates/renderlab-dx12/src/paint_graph.rs` of
/// `renderlab-native` at 13e1e703ccb91eae4c259fbb603ceb470690f40f, as handed over for this
/// test, with its comments left out. It differs from that source only where it reads the
/// graph and paints:
///
/// - `doc.edges` and `NodeKind::Target` are the example's wires and its `Block::Sink`;
/// - `target_colour(key, uv)` is the example's one tube colour;
/// - `layout.outputs`, `layout.inputs`, `layout.canvas` and `layout.zoom` are read from the
///   view's measured anatomy;
/// - `crate::paint_tubes::shown` is the stand-in in the example's `tube.rs`;
/// - `painter.add` and `painter.circle_filled` push onto a list.
///
/// Returns the painter's clip rectangle and the tubes in the order they were painted.
fn paint_energy(
    ui_clip_rect: Rect,
    canvas: &SnarlAnatomy,
    doc: &Snarl<Block>,
    time: f32,
) -> (Rect, Vec<OverlayTube>) {
    let layout = canvas;
    let clip = layout.transform.global_ui_rect;
    let clip_rect = clip.intersect(ui_clip_rect);
    let zoom = layout.transform.scale();
    let screen_rect =
        |pin: &egui_snarl::ui::MeasuredPin| layout.transform.to_global * pin.graph_rect;

    let mut tubes = Vec::new();

    for (source, target) in endpoints(doc) {
        let Block::Sink { .. } = &doc[target.node] else {
            continue;
        };
        let (Some(colour), Some(from), Some(to)) = (
            Some(graph::SINK_TUBE),
            layout.out_pin(OutPinId {
                node: source.node,
                output: 0,
            }),
            layout.in_pin(InPinId {
                node: target.node,
                input: 0,
            }),
        ) else {
            continue;
        };
        let (a, b) = (screen_rect(from).center(), screen_rect(to).center());
        let reach = ((b.x - a.x).abs() * 0.5).max(24.0);
        let curve = CubicBezierShape::from_points_stroke(
            [a, a + vec2(reach, 0.0), b - vec2(reach, 0.0), b],
            false,
            Color32::TRANSPARENT,
            Stroke::NONE,
        );
        let points = curve.flatten(Some(0.5));
        let width = 8.0 * zoom.clamp(0.6, 1.5);
        let mut shapes = Vec::new();
        for (spread, alpha) in [(3.4, 0.08), (2.2, 0.18), (1.3, 0.45), (0.7, 0.95)] {
            shapes.push(Shape::line(
                points.clone(),
                Stroke::new(width * spread, graph::tube::shown(colour, alpha)),
            ));
        }
        #[allow(clippy::cast_precision_loss)]
        let count = points.len().max(1) as f32;
        for pulse in 0..3 {
            #[allow(clippy::cast_precision_loss, clippy::suboptimal_flops)]
            let at = (time * 0.6 + pulse as f32 / 3.0).fract();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let point = points[((at * count) as usize).min(points.len() - 1)];
            shapes.push(Shape::circle_filled(
                point,
                width * 0.9,
                graph::tube::shown(colour, 0.95),
            ));
        }

        tubes.push(OverlayTube {
            wire: WirePins::new(source, target),
            shapes,
        });
    }

    (clip_rect, tubes)
}

/// Every shape in the output with the rectangle it is clipped to, in the order it is painted,
/// with groups taken apart.
fn painted(output: &FullOutput) -> Vec<(Rect, &Shape)> {
    fn take_apart<'a>(clip: Rect, shape: &'a Shape, shapes: &mut Vec<(Rect, &'a Shape)>) {
        match shape {
            Shape::Vec(group) => {
                for shape in group {
                    take_apart(clip, shape, shapes);
                }
            }
            Shape::Noop => {}
            shape => shapes.push((clip, shape)),
        }
    }

    let mut shapes = Vec::new();
    for clipped in &output.shapes {
        take_apart(clipped.clip_rect, &clipped.shape, &mut shapes);
    }
    shapes
}

/// Whether two shapes put the same thing on screen, to within a fiftieth of a point.
fn same_on_screen(painted: &Shape, expected: &Shape) -> bool {
    const POSITION: f32 = 0.02;
    const SIZE: f32 = 0.005;

    match (painted, expected) {
        (Shape::Path(painted), Shape::Path(expected)) => {
            painted.closed == expected.closed
                && painted.fill == expected.fill
                && painted.stroke.color == expected.stroke.color
                && (painted.stroke.width - expected.stroke.width).abs() < SIZE
                && painted.points.len() == expected.points.len()
                && painted
                    .points
                    .iter()
                    .zip(&expected.points)
                    .all(|(painted, expected)| painted.distance(*expected) < POSITION)
        }
        (Shape::Circle(painted), Shape::Circle(expected)) => {
            painted.fill == expected.fill
                && painted.stroke == expected.stroke
                && (painted.radius - expected.radius).abs() < SIZE
                && painted.center.distance(expected.center) < POSITION
        }
        _ => false,
    }
}

fn same_rect(a: Rect, b: Rect) -> bool {
    a.min.distance(b.min) < 0.02 && a.max.distance(b.max) < 0.02
}

#[test]
fn tubes_given_through_the_wire_hooks_are_the_overlay_they_replace() {
    let mut document = Document::from_json(graph::FIXTURE).unwrap();
    document.settings.pulses = true;

    let mut viewer = Viewer::new(document.settings);
    viewer.fit();

    let ctx = Context::default();
    show(&ctx, &mut document, &mut viewer, 12);

    let (group_node, _) = group(&document);
    let root_tube = endpoints(&document.graph)
        .into_iter()
        .map(|(out_pin, in_pin)| WirePins::new(out_pin, in_pin))
        .find(|wire| matches!(document.graph[wire.in_pin.node], Block::Sink { .. }))
        .expect("the fixture has a wire into a sink");

    let mut zooms = Vec::new();
    let mut compared = [0usize; 2];
    let mut pulse_places = std::collections::BTreeSet::new();

    for zoom in [1.0, 0.45, 0.6, 2.2, 2.0, 1.4] {
        // Zoom about the middle of the tube, so that it stays in view, then take the pointer
        // off it: a wire under the pointer is painted differently.
        let before = get_snarl_anatomy(graph::root_view_id(), &ctx).unwrap();
        let middle = before.transform.to_global * before.wire(root_tube).unwrap().midpoint().pos;
        let away = before.transform.global_clip_rect.left_bottom() + vec2(4.0, -4.0);

        for events in [
            vec![Event::PointerMoved(middle)],
            vec![Event::Zoom(zoom)],
            vec![Event::PointerMoved(away)],
            Vec::new(),
            Vec::new(),
        ] {
            let _ = pass(&ctx, &mut document, &mut viewer, 1.0 / 60.0, events);
        }

        // Several moments, far enough apart for every pulse to be somewhere else.
        for _ in 0..7 {
            let output = pass(&ctx, &mut document, &mut viewer, 0.23, Vec::new());
            let shapes = painted(&output);

            #[allow(clippy::cast_possible_truncation)]
            let time = ctx.input(|input| input.time) as f32;

            let root = get_snarl_anatomy(graph::root_view_id(), &ctx).unwrap();
            let embedded = get_snarl_anatomy(graph::embedded_view_id(group_node), &ctx).unwrap();
            assert!(root.hovered_wire().is_none());
            zooms.push(root.transform.scale());
            let grid = root.transform.scale() / 32.0;

            let views: [(&SnarlAnatomy, &Snarl<Block>, Rect); 2] = [
                (&root, &document.graph, SCREEN),
                // The view inside the group is shown in the group node's body, which is
                // clipped to what the top-level view shows.
                (
                    &embedded,
                    group(&document).1,
                    root.transform.global_clip_rect,
                ),
            ];

            for (index, (anatomy, snarl, ui_clip_rect)) in views.into_iter().enumerate() {
                let (clip_rect, tubes) = paint_energy(ui_clip_rect, anatomy, snarl, time);
                assert_eq!(tubes.len(), 1, "one wire of each graph runs into a sink");

                // The view clips its wires to what the overlay clipped its painter to. A view
                // rounds its viewport to egui's grid of a thirty-second of a graph unit, so
                // the two can sit that far apart: less than a sixteenth of a point on screen.
                let same_clip = |clip: Rect| {
                    clip.min.distance(clip_rect.min) < grid
                        && clip.max.distance(clip_rect.max) < grid
                };
                assert!(
                    same_clip(anatomy.transform.global_clip_rect),
                    "view {index}: {:?} is not {clip_rect:?}",
                    anatomy.transform.global_clip_rect
                );

                for tube in tubes {
                    let Shape::Path(widest) = &tube.shapes[0] else {
                        unreachable!()
                    };
                    let in_view = Rect::from_points(&widest.points)
                        .expand(widest.stroke.width)
                        .intersects(clip_rect);

                    let first = shapes
                        .iter()
                        .position(|(_, shape)| same_on_screen(shape, &tube.shapes[0]));

                    let Some(first) = first else {
                        assert!(
                            !in_view,
                            "view {index} at zoom {}: the tube is in view and was not painted \
                             as the overlay painted it",
                            anatomy.transform.scale()
                        );
                        continue;
                    };
                    compared[index] += 1;

                    // Stroke for stroke and disc for disc, in the overlay's order, with
                    // nothing in between.
                    for (offset, expected) in tube.shapes.iter().enumerate() {
                        let (clip, shape) = shapes[first + offset];
                        assert!(
                            same_on_screen(shape, expected),
                            "view {index} at zoom {}, time {time}, shape {offset}:\n{shape:?}\n\
                             is not\n{expected:?}",
                            anatomy.transform.scale()
                        );
                        assert!(same_clip(clip), "{clip:?} is not {clip_rect:?}");
                    }

                    // It is the wire itself that is painted this way: same pins, same path,
                    // and no other stroke along it.
                    let measured = anatomy
                        .wire(tube.wire)
                        .expect("the tube's wire was measured");
                    let on_screen = measured.geometry.transformed(anatomy.transform.to_global);
                    assert_eq!(on_screen.points().len(), widest.points.len());
                    assert_eq!(measured.paint.strokes().len(), 4);
                    assert_eq!(measured.paint.beads().len(), 3);

                    // Above every node of its view, as the overlay was.
                    let node_frames = shapes.iter().enumerate().filter(|(_, (_, shape))| {
                        matches!(shape, Shape::Rect(rect) if anatomy
                            .nodes
                            .iter()
                            .any(|node| same_rect(rect.rect, node.screen_rect)))
                    });
                    let last_frame = node_frames.map(|(at, _)| at).max().expect("node frames");
                    assert!(
                        first > last_frame,
                        "view {index}: the tube is above the nodes"
                    );

                    if index == 0 {
                        for (_, shape) in &shapes[first + 4..first + 7] {
                            let Shape::Circle(pulse) = shape else {
                                unreachable!()
                            };
                            #[allow(clippy::cast_possible_truncation)]
                            pulse_places.insert((
                                (pulse.center.x * 10.0) as i32,
                                (pulse.center.y * 10.0) as i32,
                            ));
                        }
                    }
                }

                // Every other wire keeps the example's usual look, behind the nodes.
                for wire in &anatomy.wires {
                    let is_tube = matches!(snarl[wire.pins.in_pin.node], Block::Sink { .. });
                    assert_eq!(wire.paint.beads().is_empty(), !is_tube);
                    assert_eq!(wire.paint.layer().is_none(), !is_tube);
                }
            }
        }
    }

    // The widths were compared below, inside and above the range in which they follow the zoom.
    assert!(zooms.iter().any(|&zoom| zoom < 0.6), "{zooms:?}");
    assert!(
        zooms.iter().any(|&zoom| zoom > 0.6 && zoom < 1.5),
        "{zooms:?}"
    );
    assert!(zooms.iter().any(|&zoom| zoom > 1.5), "{zooms:?}");
    assert_eq!(
        compared,
        [zooms.len(); 2],
        "both tubes were in view and compared every time"
    );
    assert!(
        pulse_places.len() > 12,
        "the pulses moved: {pulse_places:?}"
    );

    // The overlay could only be looked at. The tube is the wire, so it is picked along the
    // curve that is painted, wide of where the view's own route would have run.
    let root = get_snarl_anatomy(graph::root_view_id(), &ctx).unwrap();
    let tube = root.wire(root_tube).unwrap();
    let on_tube = root.transform.to_global * tube.midpoint().pos;
    assert!(!root.is_occluded_by_node(tube.midpoint().pos));

    for events in [
        vec![Event::PointerMoved(on_tube)],
        Vec::new(),
        vec![Event::PointerButton {
            pos: on_tube,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
        vec![Event::PointerButton {
            pos: on_tube,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
        Vec::new(),
    ] {
        let _ = pass(&ctx, &mut document, &mut viewer, 1.0 / 60.0, events);
    }

    let root = get_snarl_anatomy(graph::root_view_id(), &ctx).unwrap();
    assert_eq!(root.hovered_wire().map(|wire| wire.pins), Some(root_tube));
    assert_eq!(root.selected_wire, Some(root_tube));
    assert!(
        viewer.status.starts_with("Selected Mix → Output"),
        "{}",
        viewer.status
    );
    assert_eq!(root.wire(root_tube).unwrap().paint.strokes().len(), 5);
}
