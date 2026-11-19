# Conduits: the RenderLab pipes and hoses as wires

```text
cargo run --example conduits
```

This example carries RenderLab's pipe and hose recipe over to this crate's wire hooks,
so the recipe can be read, run and tested beside the canvas it is drawn on. It is an
example. Nothing in it is on unless a viewer asks for it, and no graph that does not ask
looks any different.

It shows one patch twice. Both views are the same graph, so a connection can be selected
or removed in either.

| View | What it is |
| --- | --- |
| A, drawer recipe | What the product's graph drawer lays on a wire: strokes across it and discs along it. |
| A, plain | The same graph with nothing asked of its wires or pins. |
| B, rack recipe | What the product's rack paints with a painter of its own, a lit tube of glass or rubber with collars, here as wires. |

The patch is the product's example patch: two oscillators, the second bending the rate of
the first, and the pen's pressure through a slew. A connection into a brush control is a
glass **pipe** that ends in brass. A connection into a module is a rubber **hose** that
ends in steel, with a window onto the paint its signal ends up moving.

## Source

| Repository | Commit | File | SHA-256 |
| --- | --- | --- | --- |
| `renderlab-native` | `265515a1a37ab949bdc6b049636b0cb31957fb44` | `crates/renderlab-dx12/src/paint_graph.rs` | `d623d933c188ed3cba9c7b09463f193be0f58ca28a3a4b3ddb59452015d1d2d6` |
| | | `crates/renderlab-dx12/src/patchbay.rs` | `b401b4cdd4309a351aaaef83141c5af42395319fe42b339e81d99dd7235076e7` |
| | | `crates/renderlab-dx12/src/paint_tubes.rs` | `cea29463035d55a50893bd9f1adc612a492114952ee39e7bcf458fb0785d992f` |
| | | `crates/renderlab-dx12/src/paint_light.rs` | `c68058fabd7f870243a5669a94448adc1dc2252a0135a0a805f116351d43a2d6` |
| `av-native` | `b6665d44485792b01d2640420e4b68ac669ffcac` | `crates/av-native-ui/src/widgets/control_graph.rs` | `0f6ab399e043a8821dff29b7c965715d1d7c8239b61c145097f54b8948fcb987` |

These came as a source excerpt, not as a tree that builds. Neither product was built or
run for this example.

## What is the product's and what is not

| Here | From | How it was carried over |
| --- | --- | --- |
| `recipe::conduit`, `PipeView` | `paint_graph.rs`, `conduit`, `PipeView` | The body as it is. `tests/conduits_demo.rs` keeps a second copy and compares the two over 768 cases. |
| `recipe::socket`, `Socket` | `paint_graph.rs`, `socket`; `control_graph.rs`, how a pin face is drawn | The body as it is, taking what the pin's connections carry in place of the product's document. |
| `recipe::Stream`, `Conduit`, `Carry`, `BRASS`, `RUBBER`, `STEEL`, `shown` | `patchbay.rs`, `paint_tubes.rs` | As they are. |
| `recipe::tube`, `Flow`, `arrival` | `patchbay.rs`, the same names | The body as it is, returning its shapes in place of painting them. |
| `recipe::pipe`, `hose` | `patchbay.rs`, the same names | Adapted. See below. |
| `recipe::rack_route`, `rack` | Nothing | New: they make a wire of the rack's painters. |
| `patch::target_recipe`, `carried`, `carry`, `seed` | `patchbay.rs`, `target_recipe`, `carried`, `carry`, `example` | The same steps over a `Snarl` in place of the product's document. |
| `patch::Light` | `paint_light.rs`, `Light` | **Stand-in.** Only `glow` is the product's arithmetic. |
| `patch::Signals` | The product's graph session | **Stand-in.** |
| `graph::canvas` | `control_graph.rs`, the canvas style | The same numbers. |

**Colour is a stand-in throughout.** The product gets the colour a paint shows, and the
light it gives off under its lamps, from a spectral model that is not in this crate and
was not in the excerpt. `patch::Light` mixes eight made-up colours. No colour in this
example, or in any picture of it, is evidence of what the product shows. A module that
carries no paint takes its colour from a palette of the product that was not in the
excerpt either; four made-up colours stand in for it.

**The signals are a stand-in.** The product runs its graph and reads what each edge
carried. `patch::Signals` makes the same shape of patch move. What a wire does with a
signal, `Stream::follow`, is the product's.

**The rack recipe is adapted in three ways**, because the rack lays its pipes on square
paths in a rack and this is a wire between two pins:

- The rack rounds each corner of a pipe's path and puts a coupling to either side of it.
  A wire has no corners. It keeps the two collars at its ends, where the rack has them.
- The rack paints over everything. Here the tube is in the wire's layer, behind the
  nodes until the wire is selected, and the bloom of arriving paint is in front of the
  node the pipe runs into.
- Two things are added: a tube under the pointer or selected is outlined, and a tube
  keeps about nine points across when the view is far out. Both are what the drawer
  recipe does for its strokes.

The rack's fold, which draws its pipes thinner and fainter when the rack is shut, and its
cable in hand are not carried over. A wire being dragged from a pin is drawn by the view.

## How the recipe meets the view

| The recipe needs | The view gives |
| --- | --- |
| Strokes across a wire, back to front, a glow, discs along it | `WirePaint::solid`, `over`, `with_glow`, `with_bead` |
| A wire picked as wide as it is drawn | `WirePaint::with_hit_width`; for a tube, `CableGeometry::with_radii` |
| A selected wire above the nodes, the rest behind | `WirePaint::with_layer`, `SnarlStyle::wire_layer` |
| To know a wire is under the pointer or selected, its length and the zoom | `WireView` |
| A pin drawn as a fitting | `SnarlPin::draw` |
| A lit tube in place of strokes | `WirePaint::hidden`, `over_shape`, on the path `wire_route` returned |
| Paint blooming on the node a pipe runs into | `WirePaint::front_shape` |
| Nodes that hide the wires behind them from the pointer; wires that can be selected | `SnarlStyle::nodes_cover_wires`, `SnarlStyle::wire_picking`, both off unless asked for |

Nothing the drawer recipe or the socket asks for is missing from the view.

The rack's painters do four things the drawer recipe cannot do with strokes and discs:
shade across the width, change colour along the length from the source's colour to the
paint's, fill the bore as far as the signal is strong, and put metal and bloom in front of
a node. When the drawer recipe was written a wire could be given none of these. Now it
can be given all four, and view B is that. Whether the product's drawer should look like
its rack is the product's decision, not this crate's.

## For the applications that use this crate

- `av-native` at `b6665d4` asks for this crate at `a39dca34`. Everything the drawer
  recipe and the socket use is in that revision. This example was not built against
  either application, and running here is not proof that it runs there.
- The recipe raises a wire when it is `selected`. In this crate, at this branch, a wire is
  only selected by a click when the style sets `wire_picking`. A canvas that moves to
  this branch and wants the selected pipe raised has to set it, and `nodes_cover_wires`
  if a node should hide the pipe behind it from the pointer. Both are off by default.
- The rack recipe needs `with_radii`, `front_shape` and `over_shape`, which are newer
  than `a39dca34`.

## What is checked, and what is not

`tests/conduits_demo.rs` holds the product's own tests of this recipe, carried over with
it, a comparison of `conduit` against a second copy of the source's body, and headless
passes of the patch: the drawer's strokes, discs and sockets are where the wire and pins
are; a tube is drawn between the walls it is picked between; a pipe's bloom is in front
of its node and a hose has none; a selected connection is raised; the plain look is asked
nothing.

Not checked: colour, against anything. The shading of `tube` against the product's
pixels. Either product, built or run. A picture of the product beside a picture of this:
the excerpt came without one. Pictures of this example are drawn in software and say
nothing about the product's GPU renderer.
