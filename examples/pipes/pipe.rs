//! A pipe drawn from a model: a wall whose width follows the model's cross-section, and the
//! contents inside it as one band of colour that moves with them.
//!
//! Everything goes through the view's two wire hooks. [`route`] gives the view the pipe as
//! it is at this moment: its centreline, and its half width at every point. The view picks
//! the wire between those walls and hands the same pipe to [`paint`], which fills it. So
//! there is one deformed pipe each pass, and it is both what is drawn and what is picked.
//!
//! The colours here are stand-ins that tell five pipes apart. They mean nothing.

use egui::{Color32, Shape, Stroke};
use egui_snarl::ui::{CableGeometry, WirePaint, WireRoute, WireView};

use super::flow::{self, LABEL_REPEAT, Mechanism, Pipe};

/// Half width of a pipe at rest.
pub const RADIUS: f32 = 9.0;

/// Thickness of the wall.
pub const WALL: f32 = 1.5;

/// Distance between the points a pipe is drawn through.
pub const SPACING: f32 = 3.0;

/// How far a Bourdon pipe at rest bows out between its ends, as a share of the distance
/// between them.
pub const BOW: f32 = 0.2;

/// Length of pipe that contents of one colour fill at rest.
pub const BAND: f32 = 128.0;

/// Distance from one fine streak in the contents to the next.
pub const STREAK: f32 = 32.0;

/// The colours the contents of one pipe run through, in turn.
pub type Pigments = [[f32; 3]; 4];

// Labels are shifted by whole repeats now and then; the colours must not notice.
const _: () = assert!(LABEL_REPEAT % (BAND * 4.0) == 0.0 && LABEL_REPEAT % STREAK == 0.0);

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
/// Contents keep their label as they move, so the colour moves with them: bands a
/// [`BAND`] long that run into each other, with fine streaks that show the contents
/// stretching where the pipe is narrow.
pub fn colour(pigments: &Pigments, label: f32) -> [f32; 3] {
    let band = (label / BAND).rem_euclid(4.0);

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (band.floor() as usize).min(3);
    let from = pigments[index];
    let to = pigments[(index + 1) % 4];

    // Hold the colour for most of the band, then run into the next.
    let run = ((band.fract() - 0.55) / 0.45).clamp(0.0, 1.0);
    let run = run * run * 2.0f32.mul_add(-run, 3.0);

    let streak = 0.08f32.mul_add((std::f32::consts::TAU * label / STREAK).sin(), 1.0);

    [0, 1, 2].map(|channel| (to[channel] - from[channel]).mul_add(run, from[channel]) * streak)
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

/// The centreline of a pipe that nothing is acting on.
///
/// It is the view's own route between the two pins, with enough points for a width that
/// varies along it. A Bourdon pipe is bowed out to one side, so that it has a curl to lose.
pub fn rest(route: &WireRoute, mechanism: Mechanism) -> CableGeometry {
    let path = route.default_route().subdivided(SPACING);

    if mechanism == Mechanism::Bourdon {
        let bow = BOW * route.from.distance(route.to);
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

/// The pipe as it is now: where it runs and how wide it is at each point.
pub fn route(route: &WireRoute, pipe: &Pipe) -> CableGeometry {
    let rest = rest(route, pipe.mechanism());

    let path = if pipe.mechanism() == Mechanism::Bourdon {
        CableGeometry::from_points(flow::flex(rest.points(), pipe.straightening()))
    } else {
        rest
    };

    // The model keeps cross-sections; a round pipe twice as large in section is the square
    // root of two as wide.
    path.with_radii(|share| RADIUS * pipe.area_at(share).sqrt())
}

/// The pipe filled with its contents.
///
/// `wire.geometry` is what [`route`] returned for this pass, so every shape below is
/// built from the walls the view picks between.
pub fn paint(wire: &WireView<'_>, pipe: &Pipe, pigments: &Pigments) -> WirePaint {
    let tube = wire.geometry;
    let bore = |share: f32| (tube.radius_at(share) - WALL).max(0.0);

    let contents = tube.clone().with_radii(bore);

    // A lighter band off centre, as a round pipe is lit.
    let sheen = CableGeometry::from_points(tube.offset(|share| -0.3 * bore(share)))
        .with_radii(|share| 0.38 * bore(share));

    let [near, far] = tube.walls();
    let glint = tube.offset(|share| -0.62 * tube.radius_at(share));

    let wall = if wire.selected {
        Stroke::new(2.2, Color32::from_rgb(255, 255, 255))
    } else if wire.hovered {
        Stroke::new(1.8, Color32::from_rgb(235, 245, 250))
    } else {
        Stroke::new(1.2, Color32::from_rgb(150, 172, 184))
    };

    WirePaint::hidden()
        .over_shape(tube.ribbon(|_| Color32::from_rgba_unmultiplied(18, 24, 30, 235)))
        .over_shape(contents.ribbon(|share| shown(colour(pigments, pipe.label_at(share)), 1.0)))
        .over_shape(sheen.ribbon(|share| {
            let lit =
                colour(pigments, pipe.label_at(share)).map(|channel| channel.mul_add(0.5, 0.5));
            shown(lit, 0.55)
        }))
        .over_shape(Shape::line(near, wall))
        .over_shape(Shape::line(far, wall))
        .over_shape(Shape::line(
            glint,
            Stroke::new(0.8, Color32::from_rgba_unmultiplied(255, 255, 255, 110)),
        ))
}
