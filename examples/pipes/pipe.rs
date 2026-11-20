//! A pipe drawn from a model: a wall whose width follows the model's cross-section, and the
//! contents inside it as one band of colour that moves with them.
//!
//! Everything goes through the view's two wire hooks. [`route`] gives the view the pipe as
//! it is at this moment: its centreline, and its half width at every point. The view picks
//! the wire between those walls and hands the same pipe to [`paint`], which fills it. So
//! there is one deformed pipe each pass, and it is both what is drawn and what is picked.
//!
//! How the pipe looks is set by a [`Style`]. The colours are stand-ins that tell five
//! pipes apart. They mean nothing.

use egui::{Color32, Pos2, Shape, Stroke, Vec2};
use egui_snarl::ui::{CableGeometry, WirePaint, WireRoute, WireView};

use super::{
    drive::{Forcing, Lay},
    flow::{self, LABEL_REPEAT, Mechanism, Params, Pipe},
};

/// Distance between the points a pipe is drawn through.
pub const SPACING: f32 = 3.0;

/// Distance between the points a fitting is drawn through. Its steps are short.
const FITTING_SPACING: f32 = 0.75;

/// Length of a fitting along the pipe, for a pipe at the width [`Style::default`] gives.
const FITTING_LENGTH: f32 = 19.0;

/// The outline of a fitting: distance from the pin, and half width there as a multiple of
/// the pipe's at rest. A flange against the node, a nut, and a collar around the pipe.
const FITTING: [(f32, f32); 7] = [
    (0.0, 1.3),
    (1.2, 1.62),
    (4.6, 1.62),
    (5.4, 1.4),
    (14.0, 1.4),
    (15.0, 1.2),
    (FITTING_LENGTH, 1.2),
];

/// Where the outline of a fitting steps, and a dark ring is drawn across it.
const FITTING_RINGS: [f32; 2] = [5.0, 14.5];

/// Share of a pipe's length that both its fittings together never take more than.
const FITTING_SHARE: f32 = 0.5;

/// How much a length of empty pipe weighs against the same length full of contents.
const WALL_WEIGHT: f32 = 0.35;

/// Lines along which a pipe is shaded across its width, from one wall to the other.
const ACROSS: [f32; 9] = [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0];

/// How many bands side by side fill a pipe from wall to wall, or a fitting.
pub const BANDS: usize = ACROSS.len() - 1;

/// Where the light comes from: above and to the left of the screen, and in front of it.
const LIGHT: [f32; 3] = [-0.30, -0.52, 0.80];

/// The colours the contents of one pipe run through, in turn.
pub type Pigments = [[f32; 3]; 4];

/// How the two ends of a pipe meet the nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    /// The pipe runs in behind the node and nothing is added.
    Behind,

    /// A fitting sits on the front of the node at each end, over the pin, and the pipe
    /// runs into it.
    Fittings,
}

/// How a pipe is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// Half width of a pipe at rest.
    pub radius: f32,

    /// Thickness of the wall.
    pub wall: f32,

    /// Length of pipe that contents of one colour fill at rest.
    pub band: f32,

    /// How much of that length a colour takes to run into the next. At one the colours
    /// change evenly all along; toward zero they sit side by side as blocks.
    pub blend: f32,

    /// How round the pipe is lit: zero is flat colour from wall to wall, one is a lit
    /// side, a shaded side and a highlight.
    pub depth: f32,

    pub ends: Ends,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            radius: 9.0,
            wall: 1.5,
            band: 128.0,
            blend: 1.0,
            depth: 1.0,
            ends: Ends::Behind,
        }
    }
}

impl Style {
    /// Half width of a pipe at rest, kept within what can be drawn.
    pub fn radius(&self) -> f32 {
        if self.radius.is_finite() {
            self.radius.clamp(1.0, 64.0)
        } else {
            9.0
        }
    }

    /// The length of pipe the colours repeat over. Labels are shifted by whole multiples
    /// of [`LABEL_REPEAT`] now and then, so it must divide that; the nearest length that
    /// does is used.
    fn repeat(&self) -> f32 {
        let band = if self.band.is_finite() {
            self.band.clamp(8.0, LABEL_REPEAT / 4.0)
        } else {
            128.0
        };
        LABEL_REPEAT / (LABEL_REPEAT / (4.0 * band)).round().max(1.0)
    }

    /// How long a fitting is on a pipe of `length`, and how much larger or smaller than
    /// [`FITTING`] it is drawn. Nothing for a pipe that has no fittings.
    pub fn fitting(&self, length: f32) -> Option<(f32, f32)> {
        if self.ends != Ends::Fittings || !(length > 1.0) {
            return None;
        }

        let scale = self.radius() / Self::default().radius;
        let reach = (FITTING_LENGTH * scale).min(0.5 * FITTING_SHARE * length);
        Some((reach, reach / FITTING_LENGTH))
    }
}

pub const fn pigments(mechanism: Mechanism) -> Pigments {
    match mechanism {
        Mechanism::Advection => [
            [0.10, 0.62, 0.95],
            [0.92, 0.95, 1.00],
            [0.16, 0.30, 0.85],
            [0.35, 0.90, 0.88],
        ],
        Mechanism::Peristalsis => [
            [0.98, 0.56, 0.12],
            [1.00, 0.88, 0.40],
            [0.86, 0.26, 0.14],
            [0.99, 0.74, 0.55],
        ],
        Mechanism::PressureWave => [
            [0.93, 0.18, 0.32],
            [1.00, 0.72, 0.76],
            [0.62, 0.10, 0.36],
            [0.98, 0.42, 0.30],
        ],
        Mechanism::Balloon => [
            [0.30, 0.82, 0.36],
            [0.86, 0.96, 0.52],
            [0.10, 0.55, 0.42],
            [0.60, 0.92, 0.74],
        ],
        Mechanism::Bourdon => [
            [0.66, 0.42, 0.96],
            [0.92, 0.80, 1.00],
            [0.40, 0.24, 0.78],
            [0.86, 0.52, 0.92],
        ],
    }
}

/// The colour of the contents with a given label.
///
/// Contents keep their label as they move, so the colour moves with them. The four
/// pigments follow each other along the pipe and run into each other as light does, not
/// as numbers on a screen do, so the colour between two of them is not darker than either.
pub fn colour(pigments: &Pigments, label: f32, style: &Style) -> [f32; 3] {
    let band = (4.0 * label / style.repeat()).rem_euclid(4.0);

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (band.floor() as usize).min(3);
    let from = pigments[index];
    let to = pigments[(index + 1) % 4];

    // Hold the colour for the part of the band that is not blended, then ease into the
    // next. Easing leaves no crease where one band ends and the next begins.
    let blend = if style.blend.is_finite() {
        style.blend.clamp(0.02, 1.0)
    } else {
        1.0
    };
    let run = ((band.fract() - (1.0 - blend)) / blend).clamp(0.0, 1.0);
    let run = run * run * 2.0f32.mul_add(-run, 3.0);

    [0, 1, 2].map(|channel| {
        let (from, to) = (from[channel].powf(2.2), to[channel].powf(2.2));
        (to - from).mul_add(run, from).powf(1.0 / 2.2)
    })
}

/// A colour at an opacity, ready to paint with.
pub fn shown(colour: [f32; 3], opacity: f32) -> Color32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;

    Color32::from_rgba_unmultiplied(
        byte(colour[0]),
        byte(colour[1]),
        byte(colour[2]),
        byte(opacity),
    )
}

/// How something round is lit.
#[derive(Clone, Copy)]
struct Finish {
    /// Share of its colour it keeps where no light reaches it.
    ambient: f32,

    /// Strength and tightness of the highlight.
    shine: f32,
    gloss: f32,
}

const CONTENTS: Finish = Finish {
    ambient: 0.6,
    shine: 0.26,
    gloss: 9.0,
};

const METAL: Finish = Finish {
    ambient: 0.30,
    shine: 0.85,
    gloss: 22.0,
};

/// The colour of a round pipe of colour `base`, seen from the front, at `across` of the
/// way from its centreline to the wall on the side `normal` points to.
fn lit(base: [f32; 3], across: f32, normal: Vec2, finish: Finish, depth: f32) -> [f32; 3] {
    let depth = if depth.is_finite() {
        depth.clamp(0.0, 1.0)
    } else {
        1.0
    };

    // The surface there faces partly sideways and partly out of the screen.
    let out = across.mul_add(-across, 1.0).max(0.0).sqrt();
    let facing = |toward: [f32; 3]| {
        (across * normal.x.mul_add(toward[0], normal.y * toward[1]) + out * toward[2]).max(0.0)
    };

    // Half way between the light and the eye, which looks straight at the screen.
    let half = {
        let sum = [LIGHT[0], LIGHT[1], LIGHT[2] + 1.0];
        let length = sum.iter().map(|part| part * part).sum::<f32>().sqrt();
        sum.map(|part| part / length)
    };

    let shade = (1.0 - finish.ambient).mul_add(facing(LIGHT), finish.ambient);
    let shade = depth.mul_add(shade - 1.0, 1.0);
    let highlight = depth * finish.shine * facing(half).powf(finish.gloss);

    base.map(|channel| channel.mul_add(shade, highlight))
}

/// The centreline of a pipe that nothing is acting on.
///
/// It is the view's own route between the two pins, with enough points for a width that
/// varies along it. A Bourdon pipe is bowed out to one side, so that it has a curl to lose.
pub fn rest(route: &WireRoute, mechanism: Mechanism, params: &Params) -> CableGeometry {
    let path = route.default_route().subdivided(SPACING);

    if mechanism == Mechanism::Bourdon {
        let bow = params.bourdon.bow * route.from.distance(route.to);
        // To the left of the way the pipe runs: upward, for a pipe that runs to the right.
        let mut bowed = path.offset(|share| -bow * (std::f32::consts::PI * share).sin());

        // The bow is nothing at either end, but only as nearly as a sine is. The ends are
        // the pins, exactly.
        if let Some(first) = bowed.first_mut() {
            *first = route.from;
        }
        if let Some(last) = bowed.last_mut() {
            *last = route.to;
        }

        CableGeometry::from_points(bowed).subdivided(SPACING)
    } else {
        path
    }
}

/// A path hanging under a load across it, between ends that stay where they are.
///
/// `weight` is how heavy the path is at each share of its length, `load` the acceleration
/// across it and `tension` the pull along it for each unit of weight. This is a taut string
/// under a load that varies along it: its curvature at each point is the load there over
/// the tension. It holds while the sag is small against the length, and it is worked out
/// afresh each pass with nothing swinging: a string at rest, not one in motion.
pub fn hang(
    path: &CableGeometry,
    mut weight: impl FnMut(f32) -> f32,
    load: Vec2,
    tension: f32,
) -> Vec<Pos2> {
    let points = path.points();
    let length = path.length();
    if points.len() < 3 || !(length > 0.0) || !(tension > 0.0) || !load.is_finite() {
        return points.to_vec();
    }

    // Distance along the path to each point, and the weight up to it added up twice: the
    // two sums a curvature takes to become a displacement.
    let mut along = 0.0;
    let mut weight_before = 0.0;
    let mut summed = 0.0;
    let mut last_weight = weight(0.0);

    let mut sums = Vec::with_capacity(points.len());
    sums.push((0.0, 0.0));

    for pair in points.windows(2) {
        let run = pair[0].distance(pair[1]);
        along += run;

        let here = weight((along / length).min(1.0));
        let before = weight_before;
        weight_before += 0.5 * (last_weight + here) * run;
        summed += 0.5 * (before + weight_before) * run;
        last_weight = here;

        sums.push((along, summed));
    }

    let (_, total) = sums[sums.len() - 1];
    let last = points.len() - 1;

    points
        .iter()
        .zip(sums)
        .enumerate()
        .map(|(index, (&point, (along, summed)))| {
            if index == 0 || index == last {
                // Exactly, not merely closely: these are the pins.
                point
            } else {
                point + load * ((along / length).mul_add(total, -summed) / tension)
            }
        })
        .collect()
}

/// The half width of a fitting at a distance from its pin, for a fitting drawn `scale`
/// times the size of [`FITTING`] on a pipe of half width `radius`.
fn fitting_radius(distance: f32, scale: f32, radius: f32) -> f32 {
    let distance = distance / scale;

    let after = FITTING
        .iter()
        .position(|&(at, _)| at >= distance)
        .unwrap_or(FITTING.len() - 1)
        .max(1);
    let ((from, narrow), (to, wide)) = (FITTING[after - 1], FITTING[after]);

    let t = ((distance - from) / (to - from)).clamp(0.0, 1.0);
    radius * (wide - narrow).mul_add(t, narrow)
}

/// The pipe as it is now: where it runs and how wide it is at each point.
///
/// `forcing` is what acts on the pipe from outside. What it does along the pipe the model
/// has already taken; here the pipe hangs under what it does across.
pub fn route(
    route: &WireRoute,
    pipe: &Pipe,
    params: &Params,
    forcing: &Forcing,
    style: &Style,
) -> CableGeometry {
    let rest = rest(route, pipe.mechanism(), params);
    let lay = Lay {
        wire: route.pins,
        from: route.from,
        to: route.to,
        length: rest.length(),
    };

    let path = if pipe.mechanism() == Mechanism::Bourdon {
        CableGeometry::from_points(flow::flex(
            rest.points(),
            pipe.straightening(),
            &params.bourdon,
        ))
    } else {
        rest
    };

    // A pipe is heavier where it holds more.
    let load = forcing.load(&lay);
    let path = if load == Vec2::ZERO {
        path
    } else {
        CableGeometry::from_points(hang(
            &path,
            |share| WALL_WEIGHT + pipe.area_at(share),
            load,
            params.tension(),
        ))
    };

    // The model keeps cross-sections; a round pipe twice as large in section is the square
    // root of two as wide.
    let radius = style.radius();
    let model = |share: f32| radius * pipe.area_at(share).sqrt();

    let length = path.length();
    let Some((reach, scale)) = style.fitting(length) else {
        return path.with_radii(model);
    };

    // A fitting has steps shorter than the pipe's points are apart: give each end more.
    let share = reach / length;
    let mut points = path
        .section(0.0, share)
        .subdivided(FITTING_SPACING * scale)
        .points()
        .to_vec();
    points.extend_from_slice(&path.section(share, 1.0 - share).points()[1..]);
    points.extend_from_slice(
        &path
            .section(1.0 - share, 1.0)
            .subdivided(FITTING_SPACING * scale)
            .points()[1..],
    );

    let path = CableGeometry::from_points(points);
    let length = path.length();

    // A fitting does not give: inside it the pipe is as wide as the fitting is.
    path.with_radii(|share| {
        let from_end = (share * length).min((1.0 - share) * length);
        // The point where the pipe leaves the fitting is the fitting's, give or take
        // what adding up the length of the path loses.
        if from_end <= reach + 1e-3 {
            fitting_radius(from_end, scale, radius)
        } else {
            model(share)
        }
    })
}

/// `tube` shaded across its width: bands side by side from wall to wall of a pipe whose
/// half width is `half_width` and whose colour is `base`, at each share of its length.
fn round(
    tube: &CableGeometry,
    half_width: impl Fn(f32) -> f32,
    base: impl Fn(f32) -> [f32; 3],
    finish: Finish,
    depth: f32,
) -> Vec<Shape> {
    let mut bands = Vec::with_capacity(BANDS);

    for pair in ACROSS.windows(2) {
        bands.push(tube.band(|share| {
            let (half_width, base) = (half_width(share), base(share));
            let normal = tube.anchor_at(share).normal;

            [pair[0], pair[1]].map(|across| {
                (
                    across * half_width,
                    shown(lit(base, across, normal, finish, depth), 1.0),
                )
            })
        }));
    }

    bands
}

/// A fitting over the stretch of `tube` from the share of its length its pin is at to the
/// share the pipe leaves it at.
fn fitting(tube: &CableGeometry, pin: f32, leaves: f32, scale: f32, style: &Style) -> Vec<Shape> {
    const STEEL: [f32; 3] = [0.66, 0.69, 0.73];
    const EDGE: Color32 = Color32::from_rgb(20, 24, 28);

    let barrel = tube.section(pin, leaves);
    let length = barrel.length();
    let mut shapes = round(
        &barrel,
        |share| barrel.radius_at(share),
        |_| STEEL,
        METAL,
        style.depth.max(0.6),
    );

    let across = |from_pin: f32| {
        // The section runs from the lower share to the higher, whichever the pin is at.
        let share = if pin <= leaves {
            from_pin / length
        } else {
            1.0 - from_pin / length
        };
        let anchor = barrel.anchor_at(share);
        let reach = anchor.normal * barrel.radius_at(share);
        [anchor.pos - reach, anchor.pos + reach]
    };

    for ring in FITTING_RINGS {
        shapes.push(Shape::line_segment(
            across(ring * scale),
            Stroke::new(0.9, EDGE.gamma_multiply(0.7)),
        ));
    }

    // Edges all round, which also soften the outline of the bands.
    let edge = Stroke::new(1.0, EDGE);
    let [near, far] = barrel.walls();
    shapes.push(Shape::line(near, edge));
    shapes.push(Shape::line(far, edge));
    shapes.push(Shape::line_segment(across(0.0), edge));
    shapes.push(Shape::line_segment(across(length), edge));

    shapes
}

/// The pipe filled with its contents.
///
/// `wire.geometry` is what [`route`] returned for this pass, so every shape below is
/// built from the walls the view picks between.
pub fn paint(wire: &WireView<'_>, pipe: &Pipe, pigments: &Pigments, style: &Style) -> WirePaint {
    let tube = wire.geometry;
    let length = tube.length();
    let radius = style.radius();
    let fitted = style.fitting(length);

    // Inside a fitting the walls are the fitting's; the pipe in it is as wide as the model
    // says, and never wider than what is picked.
    let bore = |share: f32| {
        let modelled = radius * pipe.area_at(share).sqrt();
        (modelled.min(tube.radius_at(share)) - style.wall.max(0.0)).max(0.0)
    };

    // The wall is a line as thick as the wall, with its outer edge where the pipe is
    // picked. It runs between the fittings of the pipe, not around them.
    let glass = match fitted {
        Some((reach, _)) => tube.section(reach / length, 1.0 - reach / length),
        None => tube.clone(),
    };
    let wall = if wire.selected {
        Stroke::new(style.wall + 0.9, Color32::from_rgb(255, 255, 255))
    } else if wire.hovered {
        Stroke::new(style.wall + 0.4, Color32::from_rgb(226, 238, 246))
    } else {
        Stroke::new(style.wall, Color32::from_rgb(132, 154, 168))
    };
    let wall = Stroke::new(wall.width.clamp(0.5, radius), wall.color);
    let [near, far] = [-1.0, 1.0].map(|side: f32| {
        glass.offset(|share| side * (glass.radius_at(share) - 0.5 * wall.width).max(0.0))
    });

    let mut paint = WirePaint::hidden()
        .over_shape(tube.ribbon(|_| Color32::from_rgba_unmultiplied(18, 24, 30, 235)));

    for band in round(
        tube,
        bore,
        |share| colour(pigments, pipe.label_at(share), style),
        CONTENTS,
        style.depth,
    ) {
        paint = paint.over_shape(band);
    }

    paint = paint
        .over_shape(Shape::line(near, wall))
        .over_shape(Shape::line(far, wall));

    if let Some((reach, scale)) = fitted {
        let share = reach / length;
        for (pin, leaves) in [(0.0, share), (1.0, 1.0 - share)] {
            for shape in fitting(tube, pin, leaves, scale, style) {
                paint = paint.front_shape(shape);
            }
        }
    }

    paint
}
