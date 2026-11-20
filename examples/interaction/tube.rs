//! A glowing tube with pulses travelling along it, given to the view through its wire hooks.
//!
//! A product drew this look in a painter of its own, laid over the canvas: it worked out a
//! second curve between the measured pins, stroked it four times and moved three discs along
//! it. Here the same numbers go through `wire_route` and `wire_paint` instead, so the curve
//! that is painted is the curve the pointer picks, and no second painter is left.
//!
//! This file fixes the shape, the widths, the opacities and the motion. Which wires get the
//! look, and in what colour, is for the caller to say. [`shown`] stands in for the product's
//! own way of turning a colour and an opacity into something to paint with.

use egui::{Color32, Stroke, epaint::CubicBezierShape, vec2};
use egui_snarl::ui::{CableGeometry, WireLayer, WirePaint, WireRoute, WireView};

/// Width of the tube at natural size, in screen points.
pub const WIDTH: f32 = 8.0;

/// The tube's width follows the zoom only between these two zoom levels.
pub const ZOOM_RANGE: (f32, f32) = (0.6, 1.5);

/// The strokes of the tube, back to front: width as a multiple of the tube's, and opacity.
pub const STROKES: [(f32, f32); 4] = [(3.4, 0.08), (2.2, 0.18), (1.3, 0.45), (0.7, 0.95)];

/// Discs travelling along the tube, evenly spaced.
pub const PULSES: usize = 3;

/// Trips from one end of the tube to the other that a pulse makes in a second.
pub const PULSE_RATE: f32 = 0.6;

/// Radius of a pulse as a multiple of the tube's width.
pub const PULSE_RADIUS: f32 = 0.9;

pub const PULSE_OPACITY: f32 = 0.95;

/// Shortest horizontal run of the curve out of and into a pin, in screen points.
pub const MIN_REACH: f32 = 24.0;

/// Largest distance between the curve and the path laid along it, in screen points.
pub const FLATNESS: f32 = 0.5;

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

/// The path of the tube: a curve that leaves the output pin and enters the input pin
/// horizontally.
///
/// [`MIN_REACH`] and [`FLATNESS`] are lengths on screen, so both are divided by the zoom the
/// route request carries.
pub fn route(route: &WireRoute) -> CableGeometry {
    let (from, to) = (route.from, route.to);
    let reach = ((to.x - from.x).abs() * 0.5).max(MIN_REACH / route.scale);

    let curve = CubicBezierShape::from_points_stroke(
        [from, from + vec2(reach, 0.0), to - vec2(reach, 0.0), to],
        false,
        Color32::TRANSPARENT,
        Stroke::NONE,
    );

    CableGeometry::from_points(curve.flatten(Some(FLATNESS / route.scale)))
}

/// The tube laid along the wire's path, above the nodes, with its pulses where they are at
/// `time` seconds. Pass no time to leave the pulses out.
pub fn paint(wire: &WireView<'_>, colour: [f32; 3], time: Option<f32>) -> WirePaint {
    // Strokes are given in graph units and the view multiplies them by the zoom. Dividing
    // by it here leaves a width on screen that follows the zoom only inside `ZOOM_RANGE`.
    let width = WIDTH * wire.scale.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1) / wire.scale;

    let mut paint = WirePaint::hidden()
        .with_layer(WireLayer::AboveNodes)
        .with_hit_width(width);

    for (spread, opacity) in STROKES {
        paint = paint.over(Stroke::new(width * spread, shown(colour, opacity)));
    }

    let Some(time) = time else {
        return paint;
    };

    // A pulse hops from one point of the path to the next, as it did in the product's painter.
    // `geometry.closest` turns that point back into a share of the path's length; giving
    // `at` itself as the share would move the pulse at an even speed instead.
    let points = wire.geometry.points();
    #[allow(clippy::cast_precision_loss)]
    let count = points.len().max(1) as f32;

    for pulse in 0..PULSES {
        // Written the way the product wrote it, so both round the same way.
        #[allow(clippy::cast_precision_loss, clippy::suboptimal_flops)]
        let at = (time * PULSE_RATE + pulse as f32 / PULSES as f32).fract();

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let point = points[((at * count) as usize).min(points.len() - 1)];

        paint = paint.with_bead(
            wire.geometry.closest(point).share,
            width * PULSE_RADIUS,
            shown(colour, PULSE_OPACITY),
        );
    }

    paint
}
