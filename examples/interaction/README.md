# `interaction` example and adoption note

A graph that exercises measured pins, single-path wires, wire actions, a graph view inside
a node and a searchable insert menu. `graph.rs` holds the saved state, the viewer and the
view; `tube.rs` holds a wire look that a product used to paint in an overlay; `main.rs`
holds the window; `fixture.json` is the state it starts from.

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

### Nothing changes until a graph asks

A viewer that overrides none of the wire hooks and a style that sets none of the fields
below gets the graph it had before: the same shapes, the same callbacks, the same keys
left alone. `tests/plain_graph.rs` checks each of these off and on.

| `SnarlStyle` field | Off, the default | On |
| --- | --- | --- |
| `wire_picking` | A click, a double click or a tap on a wire does nothing; no key is taken; a touch held on a wire belongs to the canvas | They select the wire; Delete, Backspace, Enter, Space and Escape act on the selected wire; a held touch is reported for the wire |
| `nodes_cover_wires` | A wire is hovered and acted on along its whole path, also under a node | A node takes the pointer from the wires it covers |
| `wire_smoothness_on_screen` | `wire_smoothness` is a distance in graph units at every zoom | It is divided by the zoom in a magnified view |
| `output_rows_clear_pins` | With pins inside the node, what an output row shows can run under its pin | The row's content ends where the pin starts |

A secondary click on a wire disconnects it either way, as it always has, unless the viewer
overrides `wire_action` or gives the wire a menu.

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
  `route.to`, the measured pin centres. The request is in graph coordinates;
  `WireRoute::scale` is the zoom, for a route that keeps a length the same on screen.
- `SnarlStyle::wire_slack` hangs the middle of the wire, `wire_sway` lets it lag behind
  a moving pin (bounded, deterministic, off when egui's `animation_time` is zero),
  `wire_hit_tolerance` is the pick distance in screen points at every zoom.
- `SnarlViewer::wire_paint(&WireView, default, ..) -> WirePaint` themes each wire:
  `WirePaint::solid(core).under(casing).with_glow(color, spread).over(highlight)`.
  `WireView` says which wire, whether it is hovered or selected, its pin colour, its path
  and the zoom. Widths are in graph units; divide by `WireView::scale` for a width on screen.
- `WirePaint::with_bead(share, radius, fill)` puts a disc on the same path, for a pulse or
  a marker. `with_layer(WireLayer::AboveNodes)` paints one wire above the nodes while the
  style decides for the rest. `with_hit_width` sets the pick band for a wide tube.
- To keep a painter of your own (a mesh, a shader), return
  `WirePaint::hidden().with_hit_width(w)` and draw from `MeasuredWire::geometry` in
  `measured`; hover, selection, actions and anchors still follow that path.

### Wires that are tubes

For a wire with a width that varies along it, or with something of the viewer's own
painted on it. None of this is used unless a viewer calls it.

- `CableGeometry::with_radii(|share| half_width)` turns a path into a tube. Return it
  from `wire_route`. The view then picks the wire anywhere between its walls, culls it by
  the bounds of the walls, and hands the same tube to `wire_paint`.
- `CableGeometry::subdivided(spacing)` gives a straight or coarsely sampled path enough
  points for a profile; `offset(|share| distance)` moves a path sideways, for a route that
  bends between fixed ends; `section(from, to)` cuts out a stretch.
- `CableGeometry::walls()` are the two edges and `ribbon(|share| colour)` is the area
  between them as one mesh with a colour per point, for contents painted as a connected
  band.
- `WirePaint::under_shape(shape)` / `over_shape(shape)` paint any `egui::Shape`, in graph
  coordinates, with the wire: in its layer, inside the clip of the view, behind and in
  front of its strokes and beads.
- `wire_route` is asked on every pass, so a tube may change from frame to frame; the pass
  that paints a shape is the pass that picks along it. Ask for the next frame yourself
  (`Context::request_repaint`) while something moves.

### Replacing a tube overlay painted over the canvas

`tube.rs` is `paint_energy` from `renderlab-native` (`crates/renderlab-dx12/src/paint_graph.rs`
at `13e1e70`) given through the hooks. Tick Pulses in the example to see it.
`tests/interaction_demo.rs` carries that function's body and checks, at six zoom levels
and seven moments each, in the top-level view and in the view inside the group, that the
view paints what the overlay would have: the same four strokes and three discs, in the
same order, to within a fiftieth of a point.

| `paint_energy` | Through the hooks |
| --- | --- |
| Cubic between the measured pin centres, reach at least 24 points, flattened to half a point | `wire_route`; both lengths divided by `WireRoute::scale` |
| Width `8 * zoom.clamp(0.6, 1.5)` in screen points | `wire_paint`; that width divided by `WireView::scale` |
| Four strokes, widest first | `WirePaint::hidden().over(stroke)` four times |
| Three discs hopping from point to point of the curve | `with_bead(wire.geometry.closest(point).share, ..)` |
| A painter in `Order::Foreground`, clipped to the canvas | `with_layer(WireLayer::AboveNodes)`; the view clips its wires |
| Only edges into a `Target` that has a colour | The viewer returns `default` for every other wire |
| `ui.input(|input| input.time)` | The viewer keeps the time of the pass |

What a product changes, in this order:

1. The wrapper that owns the viewer and the mapping from its document to `WirePins`
   takes a per-edge look from the product (a closure or a trait object: edge, zoom and
   time in, route and paint out) and calls it from its `wire_route` and `wire_paint`.
   A drawer title is one more value it takes from the product.
2. The product builds that look from its own colour lookup and the numbers above, as
   `tube.rs` does, then removes the `paint_energy` call and the function.

What then differs on screen from the overlay, by design:

- The canvas no longer paints a wire of its own under the tube. Where its route differed
  from the overlay's curve there were two curves; there is now one.
- The tube ends on the pins the edge is attached to. The overlay always used output 0 and
  input 0.
- The tube is in the canvas layer, above the nodes. An open menu, a tooltip or a window
  over the canvas now covers it; the overlay painted over them.
- Where two tubes cross, they stack in pin order, not in document order.
- The view rounds its viewport to a thirty-second of a graph unit, so the clip rectangle
  can sit that far from the canvas rectangle.
- The tube can be hovered and given a menu along the curve that is painted, and selected
  where the style turns `wire_picking` on. A node it passes over takes the pointer where
  the style turns `nodes_cover_wires` on.

Not checked here: the product's colour lookup and its function that turns a colour and an
opacity into a paint colour, for which `tube.rs` has stand-ins; the wrapper's types; and
any build of the product.

### Wire actions that leave reconnecting alone

- `SnarlViewer::wire_action(&WireAction, ..)` reports `Click`, `DoubleClick`,
  `SecondaryClick`, `LongTouch`, and `Delete` / `Activate` from the keyboard for the
  selected wire, with the point on the wire and an `InsertionAnchor`. Unless overridden,
  `SecondaryClick` and `Delete` disconnect, as a right-click did before.
- With `SnarlStyle::wire_picking` on, the view selects the wire on `Click` and takes the
  keys while a wire is selected. With it off, the view selects nothing on its own and
  takes no key; `LongTouch` is reported only for a wire that has a menu.
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
- Wires are painted in pin order. Before, the order changed from run to run.
- A wire whose paint is fully transparent and has no width is not painted at all.
- `set_selected_wire` selects a wire whatever the style says; keys then act on it.
- A view inside a node paints above every node of its host view, including a node
  dragged over the host node.

## What this example is and is not

It is a runnable view built only from the hooks above, with a fixture that round-trips.
It is not an adoption: no downstream product was changed or built against this branch.
Names such as a drawer title, what a colour means, brush shapes and node catalogues stay
with the product that owns them.
