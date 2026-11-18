//! Runs the graph of the `interaction` example through headless egui passes.

#![cfg(feature = "serde")]

#[allow(dead_code)]
#[path = "../examples/interaction/graph.rs"]
mod graph;

use egui::{Context, Pos2, RawInput, Rect, vec2};
use egui_snarl::{
    InPinId, NodeId, OutPinId, Snarl,
    ui::{WirePins, get_snarl_anatomy},
};
use graph::{Block, Document, Viewer};

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

fn show(ctx: &Context, document: &mut Document, viewer: &mut Viewer, passes: usize) {
    for pass in 0..passes {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
            time: Some(pass as f64 / 60.0),
            ..RawInput::default()
        };

        ctx.run_ui(input, |ui| {
            graph::show(document, viewer, ui);
        })
        .drop_without_applying_deltas();
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
