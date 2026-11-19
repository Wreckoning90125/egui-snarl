//! The RenderLab pipe and hose recipe, carried over to this view's wire hooks.
//!
//! Everything here is taken from `renderlab-native` at
//! `265515a1a37ab949bdc6b049636b0cb31957fb44`, `crates/renderlab-dx12/src`:
//!
//! | Here | There |
//! | --- | --- |
//! | [`shown`] | `paint_tubes.rs`, `shown` |
//! | [`Stream`], [`Conduit`], [`Carry`], [`BRASS`], [`RUBBER`], [`STEEL`] | `patchbay.rs` |
//! | [`conduit`], [`PipeView`], [`socket`] | `paint_graph.rs`, `conduit`, `PipeView`, `socket` |
//! | [`Socket`] | `av-native` at `b6665d4`, `control_graph.rs`, how a `GraphPinFace` is drawn |
//! | [`tube`], [`Flow`], [`pipe`], [`hose`], [`arrival`] | `patchbay.rs`, the same names |
//!
//! The bodies are the product's, with two kinds of change. Types of the product's own
//! are replaced by plain ones. And the rack's painters, which there paint straight onto a
//! layer, here return their shapes, so that the view can paint them with the wire.
//!
//! There are two recipes because the product has two. [`conduit`] is what its graph
//! drawer lays on a wire today: strokes and discs, which is what a wire could be given
//! when it was written. [`rack`] is what its rack paints with a painter of its own: a
//! lit cylinder of glass with the paint inside it. With a half width along the wire and
//! shapes painted with it, the second can now be a wire as well. See [`rack`].
//!
//! What is not here is what the product knows and this crate does not: which paint a
//! wire carries and what colour that paint shows under the lamps. Those come in as a
//! [`Carry`]. `light.rs` makes them up for the example.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Rgba, Shape, Stroke, Style, Vec2, epaint};
use egui_snarl::ui::{
    CableGeometry, PinInfo, PinWireInfo, SnarlPin, SnarlStyle, WireLayer, WirePaint, WireRoute,
    WireView,
};

/// Brass for the couplings.
pub const BRASS: [f32; 3] = [0.83, 0.64, 0.33];

/// Rubber for the hoses.
pub const RUBBER: [f32; 3] = [0.085, 0.085, 0.095];

/// Steel for the hose fittings.
pub const STEEL: [f32; 3] = [0.6, 0.62, 0.66];

/// Half width of a glass pipe in the open rack, and of a hose.
pub const PIPE_RADIUS: f32 = 6.5;
pub const HOSE_RADIUS: f32 = 5.0;

/// Distance from one slug of paint to the next.
pub const SLUG_SPACING: f32 = 46.0;

/// Side of the square a pin of the product's canvas is drawn in.
pub const PIN_VISUAL_SIZE: f32 = 8.0;

/// Strips across a tube, from wall to wall.
pub const ACROSS: usize = 14;

/// A linear colour as an sRGB `Color32` at `alpha`, its brightest channel kept at or
/// under one so a glow keeps its hue.
pub fn shown(linear: [f32; 3], alpha: f32) -> Color32 {
    let peak = linear.iter().fold(1.0_f32, |a, c| a.max(*c));
    let [r, g, b] = linear.map(|c| (c / peak).clamp(0.0, 1.0));
    Color32::from(Rgba::from_rgba_unmultiplied(r, g, b, alpha.clamp(0.0, 1.0)))
}

/// What a pipe carries this frame, from the signal its edge carried.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stream {
    /// How far the paint has run, in waves; it runs back toward the source while the
    /// signal is negative.
    pub phase: f32,

    /// How full the pipe runs, from nearly dry (zero) to full (one).
    pub level: f32,

    /// The signal the edge carried last.
    pub signal: f32,
}

impl Stream {
    /// Follow `signal` for `seconds`: the level eases toward its size and the paint runs
    /// faster the stronger it is, backward when it is negative.
    pub fn follow(&mut self, signal: f32, seconds: f32) {
        let size = signal.abs().min(1.0);
        let ease = 1.0 - (-seconds.max(0.0) * 9.0).exp();
        self.level += (size - self.level) * ease;
        self.signal = signal;
        let speed = size.mul_add(1.8, 0.12);
        self.phase = (self.phase + seconds.max(0.0) * speed * signal.signum()).rem_euclid(64.0);
    }
}

/// How a connection is drawn: paint arriving at the brush runs in a glass pipe; a signal
/// into a module runs in a hose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conduit {
    /// Glass, holding the paint it delivers.
    Pipe,

    /// Rubber, with a window onto the paint its signal ends up moving.
    Hose,
}

/// What one connection carries and how it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carry {
    pub conduit: Conduit,

    /// The paint loads it moves, if it moves any.
    pub recipe: Option<[f32; 8]>,

    /// The colour at its source end.
    pub from: [f32; 3],

    /// The colour at its far end: the paint it moves, as the rail shows it.
    pub into: [f32; 3],

    /// The paint's own light, display-linear, and how bright it is.
    pub emitted: [f32; 3],
    pub glow: f32,
}

/// How a wire stands on the canvas this pass.
#[derive(Clone, Copy)]
pub struct PipeView {
    /// Under the pointer or selected.
    pub lit: bool,

    /// Selected, so drawn above the nodes.
    pub raised: bool,

    /// Screen points per graph unit.
    pub scale: f32,

    /// Arc length in graph units.
    pub length: f32,

    /// The canvas's own pick width.
    pub hit: f32,
}

impl PipeView {
    pub fn of(wire: &WireView<'_>, default: &WirePaint) -> Self {
        Self {
            lit: wire.hovered || wire.selected,
            raised: wire.selected,
            scale: wire.scale,
            length: wire.geometry.length(),
            hit: default.hit_width(),
        }
    }
}

/// A connection in strokes laid across its width, back to front, with discs along it.
/// A pipe: a faint halo of the paint, or of the paint's own light where it gives off
/// any, a rim, the glass wall tinted by what feeds it, the paint, its lit core and a
/// narrow streak. A hose: rubber, and a window of the paint its signal moves. Slugs of
/// paint run as far as the signal has carried them, into the brass (pipe) or steel
/// (hose) socket each pin is. Widths are graph units, which the canvas scales with its
/// zoom; a zoomed-out connection keeps about nine points across.
pub fn conduit(carry: Carry, view: PipeView, stream: Stream) -> WirePaint {
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
            let rubber = RUBBER;
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
        // Slugs of brighter paint, one every 46 units, as far along as the signal has
        // carried them and as large as the connection runs full.
        let length = view.length.max(1.0);
        let spacing = SLUG_SPACING / length;
        let waves = (length / SLUG_SPACING).ceil().clamp(1.0, 24.0);
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
        paint = paint.with_layer(WireLayer::AboveNodes);
    }
    paint
}

/// A socket face a pin is drawn as, in place of the canvas's dot: a rim, a face inside
/// it and a bore at its centre, sized to the pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub rim: Color32,
    pub face: Color32,
    pub bore: Color32,
}

/// Each pin as the socket of the fitting that seats on it: brass where a paint pipe
/// ends, steel where a signal hose ends, its bore showing the paint that flows through
/// it, dark where nothing is plugged in.
///
/// `ends` is what each connection on the pin carries.
pub fn socket(ends: &[Carry], wired: bool) -> Face {
    let pipe = ends.iter().any(|carry| carry.conduit == Conduit::Pipe);
    let metal = if pipe { BRASS } else { STEEL };
    let bore = ends
        .iter()
        .find(|carry| carry.recipe.is_some())
        .filter(|_| wired)
        .map_or(Color32::from_gray(10), |carry| shown(carry.into, 1.0));
    Face {
        rim: shown(metal, 1.0),
        face: shown(metal.map(|c| c * 0.45), 1.0),
        bore,
    }
}

/// A pin drawn as a [`Face`], the way the product's canvas draws one.
pub struct Socket(pub Face);

impl Socket {
    /// The three discs of a socket centred on `centre`, largest first, each with its
    /// radius.
    pub fn discs(&self) -> [(f32, Color32); 3] {
        let radius = PIN_VISUAL_SIZE * 0.75;
        [
            (radius, self.0.rim),
            (radius * 0.72, self.0.face),
            (radius * 0.34, self.0.bore),
        ]
    }
}

impl SnarlPin for Socket {
    fn draw(
        self,
        snarl_style: &SnarlStyle,
        style: &Style,
        rect: Rect,
        painter: &Painter,
    ) -> PinWireInfo {
        let visual_rect = Rect::from_center_size(rect.center(), Vec2::splat(PIN_VISUAL_SIZE));
        for (radius, colour) in self.discs() {
            let _ = painter.circle_filled(visual_rect.center(), radius, colour);
        }
        // The canvas's own pin, unseen, still reports the wire's style.
        PinInfo::circle()
            .with_fill(Color32::TRANSPARENT)
            .with_stroke(Stroke::NONE)
            .draw(snarl_style, style, visual_rect, painter)
    }
}

/// What fills a tube.
#[derive(Clone, Copy)]
pub enum Flow {
    /// Paint inside glass: run `phase` waves along, filling `level` of the bore, giving
    /// off `emitted` light `glow` brightly of its own.
    Paint {
        phase: f32,
        level: f32,
        emitted: [f32; 3],
        glow: f32,
    },

    /// Solid metal.
    Metal,
}

/// A shaded cylinder along `points`: a mesh of strips across its width lit from the
/// upper left, so its walls turn from the light, a specular streak runs along it and its
/// rim catches the light. Paint fills the middle of a glass tube and brightens in waves
/// that travel toward its far end; a soft glow of the paint's colour surrounds it.
///
/// Returns the glow, if the tube has one, and the body.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]
pub fn tube(
    points: &[Pos2],
    radius: f32,
    colour: &dyn Fn(f32) -> [f32; 3],
    flow: Flow,
    strength: f32,
) -> (Option<Mesh>, Mesh) {
    let mut glow = Mesh::default();
    let mut body = Mesh::default();
    if points.len() < 2 {
        return (None, body);
    }
    let mut lengths = vec![0.0_f32];
    for pair in points.windows(2) {
        let last = lengths[lengths.len() - 1];
        lengths.push(last + pair[0].distance(pair[1]));
    }
    let total = lengths[lengths.len() - 1].max(1.0e-3);
    // Each point's normal, mitred so the walls stay parallel through a bend.
    let normals: Vec<Vec2> = (0..points.len())
        .map(|index| {
            let before = if index > 0 {
                (points[index] - points[index - 1]).normalized()
            } else {
                Vec2::ZERO
            };
            let after = if index + 1 < points.len() {
                (points[index + 1] - points[index]).normalized()
            } else {
                Vec2::ZERO
            };
            let tangent = (before + after).normalized();
            let normal = Vec2::new(-tangent.y, tangent.x);
            let reference = if after == Vec2::ZERO { before } else { after };
            let side = Vec2::new(-reference.y, reference.x);
            normal / normal.dot(side).abs().max(0.5)
        })
        .collect();
    let light = Vec2::new(-0.45, -0.89).normalized();
    let colour_of = |rgb: [f32; 3], alpha: f32| {
        Color32::from(Rgba::from_rgba_unmultiplied(
            rgb[0].clamp(0.0, 1.0),
            rgb[1].clamp(0.0, 1.0),
            rgb[2].clamp(0.0, 1.0),
            alpha.clamp(0.0, 1.0),
        ))
    };
    for (index, point) in points.iter().enumerate() {
        let t = lengths[index] / total;
        let paint = colour(t);
        let normal = normals[index];
        let unit = normal.normalized();
        let facing = unit.dot(light);
        // The glow: fading from the wall outward.
        if let Flow::Paint {
            level,
            emitted,
            glow: own,
            ..
        } = flow
        {
            // A soft halo of the paint, and where the paint gives off light of its own,
            // that light, as far as the pipe runs full.
            let lit = (own * level.mul_add(0.7, 0.3)).clamp(0.0, 1.5);
            let peak = emitted.iter().fold(1.0e-3_f32, |a, c| a.max(*c));
            let halo: [f32; 3] =
                std::array::from_fn(|c| paint[c] + (emitted[c] / peak - paint[c]) * (lit / 1.5));
            let bright = level.mul_add(0.9, 0.4) * lit.mul_add(2.0, 1.0);
            let spread = lit.mul_add(2.6, 2.4);
            for (reach, alpha) in [(-spread, 0.0), (-1.0, 0.32), (1.0, 0.32), (spread, 0.0)] {
                glow.vertices.push(epaint::Vertex {
                    pos: *point + normal * (radius * reach),
                    uv: epaint::WHITE_UV,
                    color: colour_of(halo, alpha * 0.55 * strength * bright),
                });
            }
        }
        for step in 0..=ACROSS {
            let s = (step as f32 / ACROSS as f32).mul_add(2.0, -1.0);
            let z = (1.0 - s * s).max(0.0).sqrt();
            // Light from the upper left and a little toward the viewer.
            let lambert = (s * facing).mul_add(0.62, z * 0.78).max(0.0);
            let specular = (s * facing).mul_add(0.42, z * 0.91).max(0.0).powf(48.0);
            let rim = s.abs().powf(7.0);
            let rgb: [f32; 3] = match flow {
                Flow::Paint { phase, level, .. } => {
                    // A full pipe runs paint across its bore; a dry one keeps a thread
                    // of it down the middle.
                    let inside = s.abs() < 0.64 * level.mul_add(0.62, 0.38);
                    let wave =
                        (std::f32::consts::TAU * (lengths[index] / SLUG_SPACING - phase)).sin();
                    // Strong signals run as distinct slugs of paint.
                    let slug = (wave * 0.5 + 0.5).powf(level.mul_add(3.0, 1.0));
                    if inside {
                        // The paint as the rail shows it where the bore faces the
                        // light, brighter where a slug passes.
                        let lift = level.mul_add(0.15, 0.8) + slug * level.mul_add(0.45, 0.1);
                        std::array::from_fn(|c| paint[c] * lift * lambert.mul_add(0.55, 0.62))
                    } else {
                        // The glass wall: dark, tinted by what it holds.
                        std::array::from_fn(|c| {
                            paint[c].mul_add(0.25, 0.06) * lambert.mul_add(0.9, 0.2)
                        })
                    }
                }
                Flow::Metal => std::array::from_fn(|c| paint[c] * lambert.mul_add(0.85, 0.18)),
            };
            let shine = match flow {
                Flow::Paint { .. } => specular.mul_add(0.95, rim * 0.35),
                Flow::Metal => specular.mul_add(1.2, rim * 0.15),
            };
            let rgb: [f32; 3] = std::array::from_fn(|c| rgb[c] + shine);
            body.vertices.push(epaint::Vertex {
                pos: *point + normal * (radius * s),
                uv: epaint::WHITE_UV,
                color: colour_of(rgb, strength.mul_add(0.08, 0.9)),
            });
        }
    }
    let ring = (ACROSS + 1) as u32;
    for index in 0..points.len() as u32 - 1 {
        for step in 0..ACROSS as u32 {
            let a = index * ring + step;
            let b = a + ring;
            body.indices
                .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
        if !glow.vertices.is_empty() {
            for step in 0..3_u32 {
                let a = index * 4 + step;
                let b = a + 4;
                glow.indices
                    .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    ((!glow.vertices.is_empty()).then_some(glow), body)
}

/// The shapes of one connection, in the order they are painted.
#[derive(Default)]
pub struct Parts {
    /// The halo round a pipe or round the window of a hose.
    pub glow: Vec<Mesh>,

    /// The glass or the rubber, and what shows inside it.
    pub body: Vec<Mesh>,

    /// Couplings and nuts, over the body.
    pub metal: Vec<Mesh>,
}

impl Parts {
    fn add(&mut self, (glow, body): (Option<Mesh>, Mesh), metal: bool) {
        self.glow.extend(glow);
        if metal {
            self.metal.push(body);
        } else {
            self.body.push(body);
        }
    }
}

/// One pipe of glass along `points`, its paint flowing toward the brush, with a brass
/// coupling a little way in from each end.
///
/// The rack lays its pipes on square paths and rounds each bend, with a coupling to
/// either side of it. A wire has no corners, so the rounding has nothing to do here and
/// the couplings are the two at the ends.
pub fn pipe(points: &[Pos2], radius: f32, carry: Carry, stream: Stream) -> Parts {
    let (from, into) = (carry.from, carry.into);
    let mut parts = Parts::default();
    if points.len() < 2 {
        return parts;
    }
    let colour =
        move |t: f32| std::array::from_fn::<f32, 3, _>(|c| from[c] + (into[c] - from[c]) * t);
    parts.add(
        tube(
            points,
            radius,
            &colour,
            Flow::Paint {
                phase: stream.phase,
                level: stream.level,
                emitted: carry.emitted,
                glow: carry.glow,
            },
            1.0,
        ),
        false,
    );
    for [near, far] in collars(points, COUPLING) {
        parts.add(
            tube(&[near, far], radius * 1.32, &|_| BRASS, Flow::Metal, 1.0),
            true,
        );
    }
    parts
}

/// A signal hose into a module: dark rubber, a clear window down its back showing the
/// paint the signal ends up moving, running as the signal runs, and a steel fitting at
/// each end. A hose whose signal moves no paint shows an empty window.
pub fn hose(points: &[Pos2], radius: f32, carry: Carry, stream: Stream) -> Parts {
    let mut parts = Parts::default();
    if points.len() < 2 {
        return parts;
    }
    parts.add(tube(points, radius, &|_| RUBBER, Flow::Metal, 1.0), false);
    if carry.recipe.is_some() {
        let window = carry.into;
        parts.add(
            tube(
                points,
                radius * 0.42,
                &|_| window,
                Flow::Paint {
                    phase: stream.phase,
                    level: stream.level,
                    emitted: carry.emitted,
                    glow: carry.glow * 0.5,
                },
                0.8,
            ),
            false,
        );
    }
    // Fittings: a steel nut on each end.
    for [near, far] in collars(points, NUT) {
        parts.add(
            tube(&[near, far], radius * 1.45, &|_| STEEL, Flow::Metal, 1.0),
            true,
        );
    }
    parts
}

/// Where a collar sits, as distances from the end of the connection it is on: a brass
/// coupling 7 long centred 4 in, and a steel nut from 1 to 8.
pub const COUPLING: [f32; 2] = [0.5, 7.5];
pub const NUT: [f32; 2] = [1.0, 8.0];

/// The two ends of each collar on a path, one collar at each end of the path, as far in
/// as `span` says. None on a path too short for them.
fn collars(points: &[Pos2], span: [f32; 2]) -> Vec<[Pos2; 2]> {
    let path = CableGeometry::from_points(points.iter().copied());
    let length = path.length();
    if length < 4.0 * span[1] {
        return Vec::new();
    }
    [0.0, 1.0]
        .map(|end: f32| span.map(|distance| path.anchor_at((end - distance / length).abs()).pos))
        .to_vec()
}

/// The bloom where a pipe's paint reaches a brush control: rings of its colour as wide
/// as the pipe runs full, and a bead that swells as each slug arrives.
pub fn arrival(at: Pos2, carry: Carry, stream: Stream) -> Vec<Shape> {
    if stream.level < 0.02 {
        return Vec::new();
    }
    // The bloom is the paint's own light where it gives off any, else the paint.
    let peak = carry.emitted.iter().fold(1.0e-3_f32, |a, c| a.max(*c));
    let share = (carry.glow / 1.5).clamp(0.0, 1.0);
    let colour: [f32; 3] =
        std::array::from_fn(|c| carry.into[c] + (carry.emitted[c] / peak - carry.into[c]) * share);
    let bright = stream.level * carry.glow.mul_add(1.5, 1.0);
    let beat = (std::f32::consts::TAU * stream.phase)
        .sin()
        .mul_add(0.5, 0.5);
    let reach = stream.level.mul_add(14.0, 8.0) * beat.mul_add(0.3, 0.85);
    let mut shapes: Vec<Shape> = [(1.0, 0.07), (0.7, 0.12), (0.45, 0.2)]
        .into_iter()
        .map(|(scale, alpha)| {
            Shape::circle_filled(at, reach * scale, shown(colour, alpha * bright.min(2.5)))
        })
        .collect();
    shapes.push(Shape::circle_filled(
        at,
        stream.level.mul_add(2.5, 1.5) * beat.mul_add(0.5, 0.75),
        shown(colour.map(|c| c.mul_add(0.5, 0.5)), 0.9),
    ));
    shapes
}

/// Distance between the points a rack connection is drawn through.
const SPACING: f32 = 3.0;

/// The path of a wire as a rack connection: the view's own route, with the half width of
/// the pipe or hose that runs along it and of the collar at each end.
///
/// Like [`conduit`], a connection keeps about nine points across however far the view is
/// zoomed out. Return this from `wire_route`. The view then picks the wire between
/// these walls and hands the same path to [`rack`].
pub fn rack_route(route: &WireRoute, kind: Conduit) -> CableGeometry {
    let (base, collar, span) = match kind {
        Conduit::Pipe => (PIPE_RADIUS, 1.32, COUPLING),
        Conduit::Hose => (HOSE_RADIUS, 1.45, NUT),
    };
    let radius = base.max(base * (4.5 / 6.5) / route.scale.max(0.05));

    let path = route.default_route().subdivided(SPACING);
    let length = path.length();
    path.with_radii(|share| {
        let from_end = (share * length).min((1.0 - share) * length);
        // Wide from the pin to the first point past the collar, so that the collar lies
        // inside the walls wherever the points of the path fall.
        if length >= 4.0 * span[1] && from_end <= span[1] + SPACING {
            radius * collar
        } else {
            radius
        }
    })
}

/// A wire as the rack draws its connection: the lit cylinder of [`pipe`] or [`hose`]
/// along the path [`rack_route`] gave it, and the bloom of [`arrival`] where a pipe
/// meets its pin.
///
/// The rack paints these over everything, with a painter of its own. Here they are the
/// wire: the glass and the collars are painted in the wire's layer, behind the nodes
/// until the wire is selected, and the bloom in front of the node the pipe runs into.
/// What the pointer picks is the tube that is painted.
///
/// One thing is added that the rack has no need of: a wire under the pointer or selected
/// is outlined, as [`conduit`] brightens its rim.
pub fn rack(wire: &WireView<'_>, carry: Carry, stream: Stream) -> WirePaint {
    let tube = wire.geometry;
    let radius = tube.radius_at(0.5);
    let parts = match carry.conduit {
        Conduit::Pipe => pipe(tube.points(), radius, carry, stream),
        Conduit::Hose => hose(tube.points(), radius, carry, stream),
    };

    let mut paint = WirePaint::hidden();
    for mesh in parts.glow.into_iter().chain(parts.body).chain(parts.metal) {
        paint = paint.over_shape(Shape::mesh(mesh));
    }

    if wire.hovered || wire.selected {
        let outline = Stroke::new(
            if wire.selected { 1.6 } else { 1.0 },
            Color32::from_white_alpha(if wire.selected { 230 } else { 150 }),
        );
        for wall in tube.walls() {
            paint = paint.over_shape(Shape::line(wall, outline));
        }
    }

    if carry.conduit == Conduit::Pipe {
        for shape in arrival(tube.end(), carry, stream) {
            paint = paint.front_shape(shape);
        }
    }

    if wire.selected {
        paint = paint.with_layer(WireLayer::AboveNodes);
    }
    paint
}
