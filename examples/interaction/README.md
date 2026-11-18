# `interaction` example and adoption note

A graph that exercises measured pins, single-path wires, wire actions, a graph view inside
a node and a searchable insert menu. `graph.rs` holds the saved state, the viewer and the
view; `main.rs` holds the window; `fixture.json` is the state it starts from.

```sh
cargo run --example interaction --features serde                      # 1440x900
cargo run --example interaction --features serde -- --size 390x844    # phone sized
cargo run --example interaction --features serde -- --fixture my-state.json
cargo test --all-features                                             # includes tests/interaction_demo.rs
```

Save writes the current state to `--save PATH` (default: `egui-snarl-interaction.json` in
the system temp directory) and Load reads it back. To regenerate the shipped fixture after
changing `Document::seed`:

```sh
cargo run --example interaction --features serde -- --write-fixture examples/interaction/fixture.json
```

## What a downstream graph adopts

Everything below is in `egui_snarl::ui`. Nothing in it knows what a wire carries; colour,
width and glow are whatever the viewer returns.

### Stable identity

- Give every view an explicit id: `SnarlWidget::new().id(id)`. Pins, nodes, wires and
  popups derive theirs from it (`SnarlViewIds`, available as `SnarlAnatomy::view_ids`).
  A pin's id depends on the view and the pin, not on what its row shows.
- A view shown inside a node needs an id of its own, derived from the host node
  (`embedded_view_id` in `graph.rs`).
- Wires are named by `WirePins { out_pin, in_pin }`, which is what is saved.

### Measured geometry, never offsets

- Read positions from `SnarlViewer::measured(&SnarlAnatomy, ..)` at the end of each pass,
  or from `get_snarl_anatomy(id, ctx)` afterwards: `MeasuredPin::{graph_center,
  screen_center, graph_rect, row_rect}`, `MeasuredNode::{final_rect, screen_rect,
  remeasured}`, `MeasuredWire::{geometry, paint, hit_tolerance, hovered, selected}`.
- A pin is centred on its row as the row measures in the same pass, so a row that wraps
  or gains widgets carries its pin and its wires with it at once.
- Per-pin size and placement: `PinInfo::with_size(points)`, `PinInfo::with_position(0..=1)`,
  or `SnarlPin::pin_size` on a custom pin.

### One path per wire

- The view builds one `CableGeometry` per wire and uses it for painting, hit testing,
  bounds, and midpoint and tangent anchors. Supply your own from
  `SnarlViewer::wire_route(&WireRoute, ..)`; it must start at `route.from` and end at
  `route.to`, the measured pin centres.
- `SnarlStyle::wire_slack` hangs the middle of the wire, `wire_sway` lets it lag behind
  a moving pin (bounded, deterministic, off when egui's `animation_time` is zero),
  `wire_hit_tolerance` is the pick distance in screen points at every zoom.
- `SnarlViewer::wire_paint(&WireView, default, ..) -> WirePaint` themes each wire:
  `WirePaint::solid(core).under(casing).with_glow(color, spread).over(highlight)`.
  `WireView` says which wire, whether it is hovered or selected, its pin colour and the
  zoom. `with_hit_width` widens the pick band for a wide tube.

Replacing an overlay that repaints glowing tubes over the view's own wires:

```diff
 impl SnarlViewer<Node> for Viewer {
+    fn wire_paint(&mut self, wire: &WireView<'_>, default: WirePaint, snarl: &Snarl<Node>) -> WirePaint {
+        let Some(pins) = wire.pins else { return default };       // a wire being dragged
+        let (color, width, glow) = self.look_of(pins, snarl);      // product-owned
+        WirePaint::solid(Stroke::new(width, color))
+            .with_glow(color, if wire.hovered { glow * 1.5 } else { glow })
+            .with_hit_width(width + glow)
+    }
 }
-// after the graph is shown: look pins up on the canvas and paint tubes on top
-paint_overlay(ui, &canvas_pins, &wires);
```

To keep a painter of your own (a mesh, a shader), return
`WirePaint::hidden().with_hit_width(w)` and draw from `MeasuredWire::geometry` in
`measured`; hover, selection, actions and anchors still follow that path.

### Wire actions that leave reconnecting alone

- `SnarlViewer::wire_action(&WireAction, ..)` reports `Click` (the view has selected the
  wire), `DoubleClick`, `SecondaryClick`, `LongTouch`, and `Delete` / `Activate` from the
  keyboard for the selected wire, with the point on the wire and an `InsertionAnchor`.
  Unless overridden, `SecondaryClick` and `Delete` disconnect, as a right-click did before.
- `has_wire_menu` / `show_wire_menu` host a product-owned menu at that point.
  `has_wire_widget` / `show_wire_widget` put a widget on the wire's midpoint.
- Dragging from a pin, dragging a node and panning never reach these hooks.
- Hooks run inside the UI pass. Record the intent or edit the `Snarl` and return; do not
  wait on anything.
- `get_selected_wire` / `set_selected_wire` read and set the selection from outside.

### Pan, zoom, clipping and views inside nodes

- Show another `SnarlWidget` from `show_body` with its own id and a fixed
  `min_size`/`max_size`. It pans, zooms and clips on its own; while the pointer is over
  it the host view neither pans, zooms nor hovers wires
  (`SnarlAnatomy::pointer_in_embedded_view`).
- `SnarlTransform` converts between graph and screen for either kind of view and gives
  the visible rect (`viewport`, `global_clip_rect`).
- Collapsing the host node changes nothing in either graph: the wires stay on the same
  pins of the host node, and the graph inside is untouched.

### Popups that insert where they were opened

- `show_graph_menu(pos, ..)` and `show_wire_menu(.., anchor, ..)` receive the graph point
  the menu was opened at. It does not move when egui relocates the popup at a screen
  edge or when the popup changes size.
- Return `PopupCloseBehavior::CloseOnClickOutside` from `menu_close_behavior` for menus
  with a search field or collapsible sections.

## Behaviour to check when updating an existing viewer

- The graph menu's popup id is now `SnarlViewIds::graph_menu_popup_id()`.
- `SnarlViewer::current_transform` receives the view's transform relative to the layer
  it is shown in. For a top-level view that is what it received before.
- `has_wire_widget` / `show_wire_widget` are now called.
- An output row's content no longer runs under its pin, so output rows are wider by up
  to the pin size.
- `wire_smoothness` is divided by the zoom in a magnified view.
- Wires react to the pointer only where the canvas itself is under it, not through a node.
- Keys act on the selected wire only; select one by click, tap or `set_selected_wire`.
- A view inside a node paints above every node of its host view, including a node
  dragged over the host node.

## What this example is and is not

It is a runnable view built only from the hooks above, with a fixture that round-trips.
It is not an adoption: no downstream product was changed or built against this branch.
Names such as a drawer title, what a colour means, brush shapes and node catalogues stay
with the product that owns them.
