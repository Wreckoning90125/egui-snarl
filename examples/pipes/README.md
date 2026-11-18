# Pipes

Five kinds of tube motion, beside the travelling beads they are an alternative to.

```sh
cargo run --example pipes
cargo run --example pipes -- --size 1440x900 --sweep
```

The window shows one graph twice. On the left (A) its wires are beads travelling along a
line, the look of `examples/interaction`. On the right (B) the same wires are pipes that
move. Both views run from the same command, so each wire can be compared with itself.

- `Pipes:` gives each pipe the mechanism its sink is named after, or all five pipes the
  same one.
- `Compare with:` switches the left view between beads and the plain graph.
- `command` is how hard the sources push, from -1 to 1. Negative pulls. `Sweep` swings it
  from full push to full pull and back every 14 seconds.
- `Pause` and `Reduced motion` hold every pipe and every bead still.
- Point at a pipe and the line at the bottom reads out where the pointer is against the
  wall as it is at that moment.

Nothing here is the default. The pipes exist only because this example's viewer overrides
`wire_route` and `wire_paint`. With `Plain` the viewer overrides neither and the style is
`SnarlStyle::new()`; a test checks that the view then paints what a viewer with no wire
hooks paints.

The colours are stand-ins that tell five pipes apart. This is not the Renderlab pipe look
and does not claim to be: that is a separate piece of work, waiting on its source.

## The five mechanisms

Each is a small model on a grid of 128 segments along the pipe, stepped 240 times a second
(`flow.rs`). Each answers the same three questions at every point: how large the
cross-section is, how fast the contents move, and which contents are there. `pipe.rs`
draws the answers.

| Pipe | What moves | What acts on what | Kind |
| --- | --- | --- | --- |
| Carried contents | Contents only. The wall holds still. | The command sets the speed of the contents. | Kinematic |
| Peristalsis | Rings of contraction travel along the wall and push the contents between them. | The wall is prescribed and moves the contents. The contents do not act on the wall. | Kinematic, one-way |
| Pressure wave | The source pushes in beats. Each beat widens the wall where it is and runs ahead of the contents. | Flow widens the wall; a wider wall is under more pressure and drives the flow. Neither is prescribed. | Two-way coupled, in one dimension, linearised |
| Ballooning | One soft stretch holds until the command passes 0.6, then balloons, and stays ballooned until the command has fallen well below that. | One pressure and one volume for the stretch. Its pressure resists the inflow and the pipe past it carries less while it fills. | Lumped, with a threshold and hysteresis |
| Bourdon flex | A curled pipe straightens between its two pins as the command rises, and curls a little tighter when it pulls. | The command bends the centreline through a damped spring. Nothing acts back. | Kinematic |

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

## One pipe for drawing and picking

Each pass, `wire_route` returns the pipe as it is at that moment: a centreline, and a half
width at every point (`CableGeometry::with_radii`). The view picks the wire between those
walls, takes its bounds for culling from them, and hands the same geometry back to
`wire_paint`, which builds every shape from it (`ribbon`, `walls`, `offset`). The shapes go
to the view as `WirePaint::over_shape`, so they are painted in the wire's layer, inside the
view's clip, moved and zoomed with the graph.

So a pipe that has ballooned is picked across the balloon, a pipe that is squeezed is not
picked beside the throat, and a pipe that has straightened is not picked where it used to
bow.

The contents are one mesh along the pipe with a colour at each vertex. There are no discs.
Where the contents are along the pipe is still kept by distance along the path.

## What is checked

`tests/pipes_demo.rs`, with no window:

- Carried contents move at the commanded speed and come back when it reverses.
- A travelling contraction pushes the contents and obeys continuity.
- A beat of pressure runs along the wall ahead of the contents, at the wave speed, and
  what the pipe gains is what went in less what came out.
- In a pressure wave the wall and the flow drive each other.
- A soft stretch holds below the threshold, balloons above it and stays until the command
  has fallen well below it.
- A curled pipe straightens between ends that stay exactly on their pins.
- At six moments, pushing then pulling, every pipe is filled by one mesh that lies between
  the walls the view measured, the half widths are the model's, and the pointer hovers the
  wire just inside the wall and not just outside it.
- A pipe is selected and removed where it is drawn.
- The plain look leaves the graph as a viewer with no wire hooks shows it.

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
| `flow.rs` | The five models. No drawing. |
| `pipe.rs` | The recipe: a model's pipe as geometry for the view and as shapes to paint. |
| `graph.rs` | The graph, and the viewer that gives its wires one of three looks. |
| `main.rs` | The window and its controls. |
| `../interaction/tube.rs` | The beads, used unchanged for the left view. |
