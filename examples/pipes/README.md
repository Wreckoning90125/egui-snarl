# Pipes

Five kinds of tube motion, beside the travelling beads they are an alternative to.

```sh
cargo run --example pipes
cargo run --example pipes -- --size 1440x900 --sweep
```

The window shows one graph twice. On the left (A) its wires are beads travelling along a
line, the look of `examples/interaction`. On the right (B) the same wires are pipes that
move. Both views are driven by the same things, so each wire can be compared with itself.

- `Pipes:` gives each pipe the mechanism its sink is named after, or all five pipes the
  same one.
- `Compare with:` switches the left view between beads and the plain graph.
- `Ends:` is how a pipe meets a node. `Behind` runs it in behind the node, as before.
  `Fittings` adds a fitting on the front of the node at each end, over the pin. Neither
  replaces the other.
- `Signal:` says what the signal does. As `pumps` it pushes the contents. As `opens a
  valve` it pushes nothing and lets through a share of whatever else does.
- `command` is the signal, from -1 to 1. `Sweep` swings it from one end to the other and
  back every 14 seconds.
- `gravity` is how strong gravity is, as a multiple of free fall. At zero there is none.
- `Pause` and `Reduced motion` hold every pipe and every bead still.
- `Parameters` opens a panel with every number the models and the look use.
- Point at a pipe and the line at the bottom reads out what drives it and where the
  pointer is against the wall as it is at that moment.

Nothing here is the default. The pipes exist only because this example's viewer overrides
`wire_route` and `wire_paint` and its style asks for `wire_picking` and
`nodes_cover_wires`. With `Plain` the viewer overrides neither and the style asks for
nothing about wires; a test checks that the view then paints what a viewer with no wire
hooks paints.

The nodes are laid out the way the consuming application lays its own out: pins on the
edge of the node, the title on the left of the header, rows of equal height, no collapse
button. That is layout only. The greys are egui's and the colours of the pipes are
stand-ins that tell five pipes apart. This is not the Renderlab pipe look and does not
claim to be: that is a separate piece of work, waiting on its source.

## The five mechanisms

Each is a small model on a grid of 128 segments along the pipe, stepped 240 times a second
(`flow.rs`). Each answers the same three questions at every point: how large the
cross-section is, how fast the contents move, and which contents are there. `pipe.rs`
draws the answers.

| Pipe | What moves | What acts on what | Kind |
| --- | --- | --- | --- |
| Carried contents | Contents only. The wall holds still. | The push sets the speed of the contents. | Kinematic |
| Peristalsis | Rings of contraction travel along the wall and push the contents between them. | The wall is prescribed and moves the contents. The contents do not act on the wall. | Kinematic, one-way |
| Pressure wave | The source pushes in beats. Each beat widens the wall where it is and runs ahead of the contents. | Flow widens the wall; a wider wall is under more pressure and drives the flow. Neither is prescribed. | Two-way coupled, in one dimension, linearised |
| Ballooning | One soft stretch holds until the push passes 0.6, then balloons, and stays ballooned until the push has fallen well below that. | One pressure and one volume for the stretch. Its pressure resists the inflow and the pipe past it carries less while it fills. | Lumped, with a threshold and hysteresis |
| Bourdon flex | A curled pipe straightens between its two pins as the push rises, and curls a little tighter when it pulls. | The push bends the centreline through a damped spring. Nothing acts back. | Kinematic |

None of these is fluid–structure interaction in the usual sense of the term, and none is
labelled as such. Two of them have the wall and the contents acting on each other, in
reduced form:

- The pressure wave solves the one-dimensional equations of a compliant tube: a segment
  widens by what flows in less what flows out, and flow grows where the segment behind is
  wider than the one ahead. A change runs along the wall at 330 units a second and the
  contents follow more slowly. The wall has no mass and no stiffness of its own beyond a
  straight-line pressure law, and the equations are linearised, so beats neither steepen
  nor reflect. The far end is matched so that a beat leaves without an echo.
- Ballooning is a single volume with a pressure law that rises, falls and rises again.
  That shape is what gives a threshold and a different way back.

What is not physical, so that nothing is taken for more than it is:

- Carried contents keep their colour exactly. Nothing mixes or diffuses.
- In peristalsis the contents in a throat hold still and the contents between throats are
  carried at the speed of the rings. Continuity holds; forces are not worked out.
- The Bourdon pipe is drawn shorter as it straightens (about a sixth at a full command)
  and longer as it curls. A real tube held at both ends could not lose curl without
  stretching. Its cross-section rounds out by a fixed share. No force is worked out.
- All speeds, sizes and thresholds are chosen to be readable in a graph, not measured.

## Parameters

Every number a model uses is a field of `flow::Params`, and every number the look uses is
a field of `pipe::Style`. Nothing is a constant buried in a model. The `Parameters` panel
has a slider for each, with what it means on hover, and `Defaults` puts them back.

| Group | Fields | What they change |
| --- | --- | --- |
| `Params` | `speed`, `full_drop`, `sag` | Speed of the contents under a full push; how much drop under gravity pushes as hard as a full command; how far a level pipe sags |
| `Peristalsis` | `speed`, `spacing`, `length`, `throat` | How fast the rings travel, how far apart they are, how long each is, how far each closes |
| `Wave` | `speed`, `friction`, `rate`, `share`, `resting`, `pushing` | Wave speed along the wall, loss, beats a second, how much of a beat pushes, flow between beats and at their height |
| `Balloon` | `at`, `reach`, `roundness`, `threshold`, `growth`, `gives_way`, `gives_back` | Where the soft stretch is, how long, how it is shaped, the push it gives way at, how far it grows, the two fills its pressure turns at |
| `Bourdon` | `bow`, `uncurl`, `curl`, `rate`, `damping`, `swell` | Curl at rest, how much is lost under a push and gained under a pull, how fast and how damped, how much the section rounds out |
| `Style` | `radius`, `wall`, `band`, `blend`, `depth`, `ends` | Half width, wall thickness, length of one colour, how far colours run into each other, how round the pipe is lit, how it meets a node |

Two shapes are set by these and started differently from the first version of this
example:

- A ring of peristalsis has a length of its own (`length`, 44) inside the distance to the
  next (`spacing`, 84). The pipe is fully open between rings, so it reads as a row of
  short squeezes with pockets between them. With `length` equal to `spacing` it is the one
  long wave it was.
- The soft stretch of a ballooning pipe is shaped by `roundness`: at 2 a bell, pointed in
  the middle; at 4, where it starts, flat across the middle with round shoulders; higher
  is a sausage. `at` puts it anywhere along the pipe; it starts off centre.

Numbers no slider gives (not a number, zero spacing, a reach of nothing) are replaced or
clamped where they are used. A test runs every model on such numbers.

## What drives a pipe

A pipe can be moved by a signal, by physics, or by both. The models do not know which.
`drive.rs` holds the plug point:

- `Forcing` is everything that acts on one pipe from outside during one step, added up.
- `Influence` is anything that acts on pipes. It adds its part to a `Forcing`, for the
  pipe lying at a `Lay` (which wire, where its two pins are, how long it is).
- `Drives` holds the influences on one graph: the signal, gravity, and `more`, a list any
  other `Influence` is pushed into. Nothing else changes when one is added; a test plugs
  in a wind of its own.

A `Forcing` is split the way a pipe splits it:

| Channel | What it is | What it moves | Who reads it |
| --- | --- | --- | --- |
| `push` | A pump at the inlet, as a share of a full command | The contents, along the pipe | Every model, through `Forcing::drive` |
| `opening` | A valve at the inlet, from shut to open | Lets a share of every push through | Every model, through `Forcing::drive` |
| `body`, along the pipe | An acceleration of the pipe and its contents; `Forcing::head` is the part between the two ends | The contents, along the pipe | Every model, through `Forcing::drive` |
| `body`, across the pipe | The rest of it, `Forcing::load` | The centreline: the pipe hangs | `pipe::route`, through `pipe::hang` |

`Forcing::drive` is `opening * (push + head / full_head)`, held within -1 and 1. So the
ways of combining are:

| Driven by | How | What is seen |
| --- | --- | --- |
| Signal only | `Signal` as pump, gravity 0 | Every pipe follows the command. This is how the example starts |
| Physics only | Command 0, gravity above 0 | Contents run downhill, each pipe as hard as its drop; level pipes hold still; every pipe sags |
| Both, added | `Signal` as pump, gravity above 0 | The pump works with gravity in a pipe that drops and against it in one that rises |
| Both, multiplied | `Signal` as valve, gravity above 0 | Gravity moves the contents and the signal says how much of that is let through. Shut, nothing moves |
| Along and across apart | Turn `gravity angle` | Gravity along a pipe pushes its contents and does not bend it; across it, bends it and does not push |

What gravity does here, and what it does not:

- Along the pipe only the difference in height between the two ends counts, as it does
  for a full pipe with a wall that does not give. `full_drop` is the drop that pushes as
  hard as a full command.
- Across the pipe the centreline hangs as a taut string at rest, heavier where the pipe
  holds more: a ballooned pipe hangs lower at the balloon. It is worked out afresh each
  pass. Nothing swings, and it holds for a sag that is small against the length.
- The contents do not weigh on the wall: a low part of a pipe does not swell. There is no
  free surface: a pipe is always full, and does not drain.

### Influences listed and not built

Each of these is one more `Influence`. The last column says whether `Forcing` can carry
it as it is.

| Influence | What it would write | Status |
| --- | --- | --- |
| A value for each wire | `push` or `opening`, chosen by `Lay::wire` | TODO. Fits as it is: the signal here is one number for all wires only because the example has one slider |
| Inertia of a dragged node | `body`: minus the acceleration of the pins | TODO. Fits as it is. Needs the pins' positions kept from one frame to the next |
| A spinning or shaken frame | `body`, changing in time through `Influence::advance` | TODO. Fits as it is |
| Wind, noise | `body`, differing from pipe to pipe | TODO. Fits for one value a pipe; a gust that varies along a pipe needs a channel for each point |
| Pressure of a network | `push`, from a solve over all pipes that meet at a node | TODO. Fits, but the solve needs the graph, which a `Lay` does not carry. `Drives` would hand it over |
| Weight of the contents on the wall | Nothing new: `body` and the height of each point give it | TODO. Needs each model to let its wall give under it, not a new channel |
| Free surface, draining, partial fill | Nothing new on the way in | TODO. Needs a model that keeps a fill at each point |
| Buoyancy, settling of what is carried | `body`, with a density for each kind of contents | TODO. Needs contents that differ in weight |
| The pointer pinching or pressing a pipe | A squeeze at one point | TODO. Needs a channel for each point of a pipe, which `Forcing` does not have |
| Contact with nodes and other pipes | A limit on where the centreline may go | TODO. Needs a constraint on the route, not a force |
| Heat | Length and width at rest | TODO. Needs a channel for the pipe at rest, not for what pushes it |

What `Forcing` has no room for yet is therefore three things: a value at each point of a
pipe, a limit on where it may lie, and a change of the pipe at rest.

## One pipe for drawing and picking

Each pass, `wire_route` returns the pipe as it is at that moment: a centreline, and a half
width at every point (`CableGeometry::with_radii`). The view picks the wire between those
walls, takes its bounds for culling from them, and hands the same geometry back to
`wire_paint`, which builds every shape from it (`band`, `ribbon`, `offset`, `section`).
The shapes go to the view as `WirePaint::over_shape`, so they are painted in the wire's
layer, inside the view's clip, moved and zoomed with the graph.

So a pipe that has ballooned is picked across the balloon, a pipe that is squeezed is not
picked beside the throat, a pipe that has straightened is not picked where it used to
bow, and a pipe that hangs under gravity is picked where it hangs.

The contents are eight bands side by side from wall to wall (`CableGeometry::band`), each
with a colour at every vertex. Across the pipe they are lit as a round thing is: a lit
side, a shaded side and a highlight. Along it the four colours of a pipe run into each
other evenly, blended as light and not as numbers on a screen, so there is no edge
between two colours and no dark seam. There are no discs. Where the contents are along
the pipe is still kept by distance along the path.

### Ends

With `Ends::Behind` the pipe is painted in its layer, behind the nodes, and runs in under
the node it meets.

With `Ends::Fittings` each end also gets a fitting: a flange against the node, a nut and
a collar, shaded as metal. The fitting is given to the view as `WirePaint::front_shape`,
which paints it in front of the nodes while the rest of the pipe stays behind them.
Inside a fitting the half width of the pipe is the fitting's, so the fitting is picked as
it is drawn, and it does not give with the model under it. A fitting only paints: the pin
under it still takes the pointer, so a wire can be dragged from it as before.

## What is checked

`tests/pipes_demo.rs`, with no window:

- Carried contents move at the commanded speed and come back when it reverses.
- A travelling contraction pushes the contents and obeys continuity, more nearly the
  better the grid resolves a ring.
- A beat of pressure runs along the wall ahead of the contents, at the wave speed, and
  what the pipe gains is what went in less what came out.
- In a pressure wave the wall and the flow drive each other.
- A soft stretch holds below the threshold, balloons above it and stays until the push
  has fallen well below it.
- A curled pipe straightens between ends that stay exactly on their pins.
- The numbers of a model shape its pipe: rings as far apart and as long as asked, a
  balloon where it is put and as round as asked, and numbers that make no sense still
  leave a pipe that can be drawn.
- The colour of the contents has no step along the pipe, reaches each pigment, is not
  darker between two, and does not notice when labels are shifted.
- A pipe is driven by a signal, by gravity, or by both: head and load of an acceleration,
  pump and head added, a valve letting a share through, an influence of the test's own
  plugged in, and every mechanism doing downhill under gravity what a pump makes it do.
- A path under a load hangs in the parabola a string does, between ends that stay put,
  lower where it is heavier.
- Under gravity, in the view, each pipe stays on its pins, hangs by what a string of its
  weight hangs by, runs downhill as fast as its drop says, and is picked where it hangs
  and not where it ran before.
- At six moments, pushing then pulling, every pipe is a wall and eight bands, no vertex
  of any of them lies outside the walls the view measured, the half widths are the
  model's, and the pointer hovers the wire just inside the wall and not just outside it.
- With fittings, the pipes are painted before the nodes and the fittings after them,
  nothing is painted outside what is picked, the pointer is on the wire on a fitting and
  off it beside one, and a new pipe is pulled from the pin under one fitting to the pin
  under another.
- A pipe is selected and removed where it is drawn.
- The plain look leaves the graph as a viewer with no wire hooks shows it, whatever the
  drives and the ends are set to.

## The wider family

Every kind of tube motion found while choosing these five, grouped by what moves and what
makes it move. Other names for the same thing are in the second column.

| Mechanism | Also called | Here |
| --- | --- | --- |
| **Contents move, wall still** | | |
| Carried contents | advection, convection, plug flow, transport along arc length | Implemented: Carried contents |
| Separate carriers | beads, pulses, packets, particles, dashes, marching ants | Implemented earlier in `examples/interaction`; shown on the left |
| Slugs and fill fronts | slug flow, two-phase flow, Taylor bubbles, meniscus, partial fill | Deferred. Belongs with the Renderlab port, which describes signal-dependent fill and slug speed |
| Faster in the middle | Poiseuille profile, shear, Taylor dispersion | Deferred. Needs contents that vary across the pipe as well as along it |
| Mixing | diffusion, blending of pigments | Deferred. Colours are deliberately carried unmixed |
| Back and forth | oscillatory flow, sloshing, Womersley flow | Partly: a negative command reverses every pipe. No model of its own |
| **Wall moves of itself and drives the contents** | | |
| Travelling contraction | peristalsis, peristaltic or roller pump, occlusion wave | Implemented: Peristalsis |
| Contraction that stays put | segmentation, sphincter, pinch valve, vasomotion | Deferred |
| Wall traction | cilia, metachronal wave | Deferred. Nothing to see at the size of a wire |
| **Contents move the wall** | | |
| Travelling distension | pressure wave, pulse wave, arterial pulse, Moens–Korteweg wave, water hammer in a soft pipe | Implemented: Pressure wave |
| Echoes | wave reflection, standing wave, impedance mismatch | Deferred. The outlet is matched on purpose |
| Steepening | nonlinear pulse wave, shock, bore | Deferred. The model is linearised |
| Whole pipe swells | compliance, Windkessel, capacitance, hose swelling | Partly: the Bourdon pipe rounds out along its whole length. No model of its own |
| Local ballooning | aneurysm, bulge, limit-point or inflation instability, snap-through, compliance with a threshold | Implemented: Ballooning |
| Collapse under suction | collapsible tube, Starling resistor, flow limitation, flutter | Deferred. The strongest candidate for a sixth: it needs the wall and the flow solved together |
| **Contents move the centreline** | | |
| Uncurling under pressure | Bourdon tube, pressure straightening, hose stiffening | Implemented: Bourdon flex |
| Whip | fire-hose instability, garden-hose flutter, pipe conveying fluid | Deferred. Needs a beam with mass coupled to the momentum of the flow |
| Lengthening and shortening | McKibben muscle, axial stretch | Deferred. The change of length in Bourdon flex is a side effect of its geometry, not this |
| Kinking | buckling, crease, Brazier ovalisation | Deferred |
| Twisting | torsion | Deferred |
| **Centreline moves for other reasons** | | |
| Slack and sway | sag, catenary, inertia when a node is dragged | Implemented earlier in `examples/interaction` |
| Hanging under gravity | sag, catenary, taut string under a load | Implemented as an influence, for every pipe: see What drives a pipe |
| **Contents move for other reasons** | | |
| Running downhill | gravity-driven flow, head, siphon | Implemented as an influence, for a full pipe: see What drives a pipe |
| Draining and filling | free surface, open-channel flow, partial fill | Deferred: listed under Influences listed and not built |
| **Wall and contents solved together** | | |
| Fluid–structure interaction | FSI, two-way coupling in two or three dimensions, partitioned or monolithic, moving mesh, immersed boundary | Deferred. Not implemented and not claimed. The pressure wave and ballooning are two-way only in the reduced forms described above |

### Why these five

They are the five that were asked for, and the inventory did not turn up a stronger set.
Each takes a different answer to what moves and what moves it: contents alone; wall moves
contents; contents move the wall as a wave; contents move the wall in one place, past a
threshold; contents move the centreline. Each needs something different from the view,
which is what makes them a test of the hooks: a width that varies along the pipe, a width
that varies in time, a width much larger than the wire in one place, and a centreline that
is not the view's own.

Collapse under suction would be the first to add. It was left out because it is the
ballooning law with the sign reversed as far as the view is concerned, and because done
properly it is the one that needs a real coupled solver.

## Files

| File | What it holds |
| --- | --- |
| `flow.rs` | The five models and their numbers. No drawing. |
| `drive.rs` | What drives a pipe: the signal, gravity, and the place another influence plugs into. |
| `pipe.rs` | The recipe: a model's pipe as geometry for the view and as shapes to paint. |
| `graph.rs` | The graph, and the viewer that gives its wires one of three looks. |
| `main.rs` | The window and its controls. |
| `../interaction/tube.rs` | The beads, used unchanged for the left view. |
