use std::{collections::hash_map::Entry, sync::Arc};

use ahash::HashMap;
use egui::{Color32, Context, Id, Pos2, Rect, Shape, Stroke, Vec2, cache::CacheTrait, pos2};
use smallvec::SmallVec;

use crate::{InPinId, OutPinId};

use super::cable::CableGeometry;

const MAX_CURVE_SAMPLES: usize = 100;

/// Layer where wires are rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "egui-probe", derive(egui_probe::EguiProbe))]
#[derive(Default)]
pub enum WireLayer {
    /// Wires are rendered behind nodes.
    /// This is default.
    #[default]
    BehindNodes,

    /// Wires are rendered above nodes.
    AboveNodes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WireId {
    Connected {
        snarl_id: Id,
        out_pin: OutPinId,
        in_pin: InPinId,
    },
    NewInput {
        snarl_id: Id,
        in_pin: InPinId,
    },
    NewOutput {
        snarl_id: Id,
        out_pin: OutPinId,
    },
}

/// Controls style in which wire is rendered.
///
/// Variants are given in order of precedence when two pins require different styles.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "egui-probe", derive(egui_probe::EguiProbe))]
#[derive(Default)]
pub enum WireStyle {
    /// Straight line from one endpoint to another.
    Line,

    /// Draw wire as straight lines with 90 degree turns.
    /// Corners has radius of `corner_radius`.
    AxisAligned {
        /// Radius of corners in wire.
        corner_radius: f32,
    },

    /// Draw wire as 3rd degree Bezier curve.
    Bezier3,

    /// Draw wire as 5th degree Bezier curve.
    #[default]
    Bezier5,
}

pub const fn pick_wire_style(left: WireStyle, right: WireStyle) -> WireStyle {
    match (left, right) {
        (WireStyle::Line, _) | (_, WireStyle::Line) => WireStyle::Line,
        (
            WireStyle::AxisAligned { corner_radius: a },
            WireStyle::AxisAligned { corner_radius: b },
        ) => WireStyle::AxisAligned {
            corner_radius: f32::max(a, b),
        },
        (WireStyle::AxisAligned { corner_radius }, _)
        | (_, WireStyle::AxisAligned { corner_radius }) => WireStyle::AxisAligned { corner_radius },
        (WireStyle::Bezier3, _) | (_, WireStyle::Bezier3) => WireStyle::Bezier3,
        (WireStyle::Bezier5, WireStyle::Bezier5) => WireStyle::Bezier5,
    }
}

pub fn adjust_frame_size(
    mut frame_size: f32,
    upscale: bool,
    downscale: bool,
    from: Pos2,
    to: Pos2,
) -> f32 {
    let length = (from - to).length();
    if upscale {
        frame_size = frame_size.max(length / 6.0);
    }
    if downscale {
        frame_size = frame_size.min(length / 6.0);
    }
    frame_size
}

/// Returns 5th degree bezier curve control points for the wire
fn wire_bezier_5(frame_size: f32, from: Pos2, to: Pos2) -> [Pos2; 6] {
    let from_norm_x = frame_size;
    let from_2 = pos2(from.x + from_norm_x, from.y);
    let to_norm_x = -from_norm_x;
    let to_2 = pos2(to.x + to_norm_x, to.y);

    let between = (from_2 - to_2).length();

    if from_2.x <= to_2.x && between >= frame_size * 2.0 {
        let middle_1 = from_2 + (to_2 - from_2).normalized() * frame_size;
        let middle_2 = to_2 + (from_2 - to_2).normalized() * frame_size;

        [from, from_2, middle_1, middle_2, to_2, to]
    } else if from_2.x <= to_2.x {
        let t = (between - (to_2.y - from_2.y).abs())
            / frame_size.mul_add(2.0, -(to_2.y - from_2.y).abs());

        let mut middle_1 = from_2 + (to_2 - from_2).normalized() * frame_size;
        let mut middle_2 = to_2 + (from_2 - to_2).normalized() * frame_size;

        if from_2.y >= to_2.y + frame_size {
            let u = (from_2.y - to_2.y - frame_size) / frame_size;

            let t0_middle_1 = pos2(
                (1.0 - u).mul_add(frame_size, from_2.x),
                frame_size.mul_add(-u, from_2.y),
            );
            let t0_middle_2 = pos2(to_2.x, to_2.y + frame_size);

            middle_1 = t0_middle_1.lerp(middle_1, t);
            middle_2 = t0_middle_2.lerp(middle_2, t);
        } else if from_2.y >= to_2.y {
            let u = (from_2.y - to_2.y) / frame_size;

            let t0_middle_1 = pos2(
                u.mul_add(frame_size, from_2.x),
                frame_size.mul_add(1.0 - u, from_2.y),
            );
            let t0_middle_2 = pos2(to_2.x, to_2.y + frame_size);

            middle_1 = t0_middle_1.lerp(middle_1, t);
            middle_2 = t0_middle_2.lerp(middle_2, t);
        } else if to_2.y >= from_2.y + frame_size {
            let u = (to_2.y - from_2.y - frame_size) / frame_size;

            let t0_middle_1 = pos2(from_2.x, from_2.y + frame_size);
            let t0_middle_2 = pos2(
                (1.0 - u).mul_add(-frame_size, to_2.x),
                frame_size.mul_add(-u, to_2.y),
            );

            middle_1 = t0_middle_1.lerp(middle_1, t);
            middle_2 = t0_middle_2.lerp(middle_2, t);
        } else if to_2.y >= from_2.y {
            let u = (to_2.y - from_2.y) / frame_size;

            let t0_middle_1 = pos2(from_2.x, from_2.y + frame_size);
            let t0_middle_2 = pos2(
                u.mul_add(-frame_size, to_2.x),
                frame_size.mul_add(1.0 - u, to_2.y),
            );

            middle_1 = t0_middle_1.lerp(middle_1, t);
            middle_2 = t0_middle_2.lerp(middle_2, t);
        } else {
            unreachable!();
        }

        [from, from_2, middle_1, middle_2, to_2, to]
    } else if from_2.y >= frame_size.mul_add(2.0, to_2.y) {
        let middle_1 = pos2(from_2.x, from_2.y - frame_size);
        let middle_2 = pos2(to_2.x, to_2.y + frame_size);

        [from, from_2, middle_1, middle_2, to_2, to]
    } else if from_2.y >= to_2.y + frame_size {
        let t = (from_2.y - to_2.y - frame_size) / frame_size;

        let middle_1 = pos2(
            (1.0 - t).mul_add(frame_size, from_2.x),
            frame_size.mul_add(-t, from_2.y),
        );
        let middle_2 = pos2(to_2.x, to_2.y + frame_size);

        [from, from_2, middle_1, middle_2, to_2, to]
    } else if from_2.y >= to_2.y {
        let t = (from_2.y - to_2.y) / frame_size;

        let middle_1 = pos2(
            t.mul_add(frame_size, from_2.x),
            frame_size.mul_add(1.0 - t, from_2.y),
        );
        let middle_2 = pos2(to_2.x, to_2.y + frame_size);

        [from, from_2, middle_1, middle_2, to_2, to]
    } else if to_2.y >= frame_size.mul_add(2.0, from_2.y) {
        let middle_1 = pos2(from_2.x, from_2.y + frame_size);
        let middle_2 = pos2(to_2.x, to_2.y - frame_size);

        [from, from_2, middle_1, middle_2, to_2, to]
    } else if to_2.y >= from_2.y + frame_size {
        let t = (to_2.y - from_2.y - frame_size) / frame_size;

        let middle_1 = pos2(from_2.x, from_2.y + frame_size);
        let middle_2 = pos2(
            (1.0 - t).mul_add(-frame_size, to_2.x),
            frame_size.mul_add(-t, to_2.y),
        );

        [from, from_2, middle_1, middle_2, to_2, to]
    } else if to_2.y >= from_2.y {
        let t = (to_2.y - from_2.y) / frame_size;

        let middle_1 = pos2(from_2.x, from_2.y + frame_size);
        let middle_2 = pos2(
            t.mul_add(-frame_size, to_2.x),
            frame_size.mul_add(1.0 - t, to_2.y),
        );

        [from, from_2, middle_1, middle_2, to_2, to]
    } else {
        unreachable!();
    }
}

/// Returns 3rd degree bezier curve control points for the wire
fn wire_bezier_3(frame_size: f32, from: Pos2, to: Pos2) -> [Pos2; 4] {
    let [a, b, _, _, c, d] = wire_bezier_5(frame_size, from, to);
    [a, b, c, d]
}

fn bezier_derivative_3(points: &[Pos2; 4]) -> [Pos2; 3] {
    let [p0, p1, p2, p3] = *points;

    let factor = 3.0;

    [
        (factor * (p1 - p0)).to_pos2(),
        (factor * (p2 - p1)).to_pos2(),
        (factor * (p3 - p2)).to_pos2(),
    ]
}

fn bezier_derivative_5(points: &[Pos2; 6]) -> [Pos2; 5] {
    let [p0, p1, p2, p3, p4, p5] = *points;

    let factor = 5.0;

    [
        (factor * (p1 - p0)).to_pos2(),
        (factor * (p2 - p1)).to_pos2(),
        (factor * (p3 - p2)).to_pos2(),
        (factor * (p4 - p3)).to_pos2(),
        (factor * (p5 - p4)).to_pos2(),
    ]
}

fn bezier_draw_samples_number_3(points: &[Pos2; 4], threshold: f32) -> usize {
    #![allow(clippy::similar_names)]
    #![allow(clippy::cast_precision_loss)]

    let d = bezier_derivative_3(points);

    lower_bound(2, MAX_CURVE_SAMPLES, |n| {
        let mut prev = points[0];
        for i in 1..n {
            let t = i as f32 / (n - 1) as f32;
            let next = sample_bezier(points, t);

            let m = t - 0.5 / (n - 1) as f32;

            // Compare absolute error of mid point
            let mid_line = ((prev.to_vec2() + next.to_vec2()) * 0.5).to_pos2();
            let mid_curve = sample_bezier(points, m);

            let error_sq = (mid_curve - mid_line).length_sq();
            if error_sq > threshold * threshold {
                return false;
            }

            // Compare angular error of mid point
            let mid_line_dx = next.x - prev.x;
            let mid_line_dy = next.y - prev.y;

            let line_w = f32::hypot(mid_line_dx, mid_line_dy);

            let d_curve = sample_bezier(&d, m);
            let mid_curve_dx = d_curve.x;
            let mid_curve_dy = d_curve.y;

            let curve_w = f32::hypot(mid_curve_dx, mid_curve_dy);

            let error = f32::max(
                (mid_curve_dx / curve_w).mul_add(line_w, -mid_line_dx).abs(),
                (mid_curve_dy / curve_w).mul_add(line_w, -mid_line_dy).abs(),
            );
            if error > threshold * 2.0 {
                return false;
            }

            prev = next;
        }

        true
    })
}

fn bezier_draw_samples_number_5(points: &[Pos2; 6], threshold: f32) -> usize {
    #![allow(clippy::similar_names)]
    #![allow(clippy::cast_precision_loss)]

    let d = bezier_derivative_5(points);

    lower_bound(2, MAX_CURVE_SAMPLES, |n| {
        let mut prev = points[0];
        for i in 1..n {
            let t = i as f32 / (n - 1) as f32;
            let next = sample_bezier(points, t);

            let m = t - 0.5 / (n - 1) as f32;

            // Compare absolute error of mid point
            let mid_line = ((prev.to_vec2() + next.to_vec2()) * 0.5).to_pos2();
            let mid_curve = sample_bezier(points, m);

            let error_sq = (mid_curve - mid_line).length_sq();
            if error_sq > threshold * threshold {
                return false;
            }

            // Compare angular error of mid point
            let mid_line_dx = next.x - prev.x;
            let mid_line_dy = next.y - prev.y;

            let line_w = f32::hypot(mid_line_dx, mid_line_dy);

            let d_curve = sample_bezier(&d, m);
            let mid_curve_dx = d_curve.x;
            let mid_curve_dy = d_curve.y;

            let curve_w = f32::hypot(mid_curve_dx, mid_curve_dy);

            let error = f32::max(
                (mid_curve_dx / curve_w).mul_add(line_w, -mid_line_dx).abs(),
                (mid_curve_dy / curve_w).mul_add(line_w, -mid_line_dy).abs(),
            );
            if error > threshold * 2.0 {
                return false;
            }

            prev = next;
        }

        true
    })
}

// #[allow(clippy::let_and_return)]
fn sample_bezier(points: &[Pos2], t: f32) -> Pos2 {
    match *points {
        [] => unimplemented!(),
        [p0] => p0,
        [p0, p1] => p0.lerp(p1, t),
        [p0, p1, p2] => {
            let p0_0 = p0;
            let p1_0 = p1;
            let p2_0 = p2;

            let p0_1 = p0_0.lerp(p1_0, t);
            let p1_1 = p1_0.lerp(p2_0, t);

            p0_1.lerp(p1_1, t)
        }
        [p0, p1, p2, p3] => {
            let p0_0 = p0;
            let p1_0 = p1;
            let p2_0 = p2;
            let p3_0 = p3;

            let p0_1 = p0_0.lerp(p1_0, t);
            let p1_1 = p1_0.lerp(p2_0, t);
            let p2_1 = p2_0.lerp(p3_0, t);

            sample_bezier(&[p0_1, p1_1, p2_1], t)
        }
        [p0, p1, p2, p3, p4] => {
            let p0_0 = p0;
            let p1_0 = p1;
            let p2_0 = p2;
            let p3_0 = p3;
            let p4_0 = p4;

            let p0_1 = p0_0.lerp(p1_0, t);
            let p1_1 = p1_0.lerp(p2_0, t);
            let p2_1 = p2_0.lerp(p3_0, t);
            let p3_1 = p3_0.lerp(p4_0, t);

            sample_bezier(&[p0_1, p1_1, p2_1, p3_1], t)
        }
        [p0, p1, p2, p3, p4, p5] => {
            let p0_0 = p0;
            let p1_0 = p1;
            let p2_0 = p2;
            let p3_0 = p3;
            let p4_0 = p4;
            let p5_0 = p5;

            let p0_1 = p0_0.lerp(p1_0, t);
            let p1_1 = p1_0.lerp(p2_0, t);
            let p2_1 = p2_0.lerp(p3_0, t);
            let p3_1 = p3_0.lerp(p4_0, t);
            let p4_1 = p4_0.lerp(p5_0, t);

            sample_bezier(&[p0_1, p1_1, p2_1, p3_1, p4_1], t)
        }
        _ => unimplemented!(),
    }
}

#[derive(Clone, Copy, PartialEq)]
struct AxisAlignedWire {
    aabb: Rect,
    turns: usize,
    segments: [(Pos2, Pos2); 5],
    turn_centers: [Pos2; 4],
    turn_radii: [f32; 4],
}

impl Default for AxisAlignedWire {
    #[inline]
    fn default() -> Self {
        Self {
            aabb: Rect::NOTHING,
            turns: 0,
            segments: [(Pos2::ZERO, Pos2::ZERO); 5],
            turn_centers: [Pos2::ZERO; 4],
            turn_radii: [0.0; 4],
        }
    }
}

#[allow(clippy::too_many_lines)]
fn wire_axis_aligned(corner_radius: f32, frame_size: f32, from: Pos2, to: Pos2) -> AxisAlignedWire {
    let corner_radius = corner_radius.max(0.0);

    let half_height = f32::abs(from.y - to.y) / 2.0;
    let max_radius = (half_height / 2.0).min(corner_radius);

    let frame_size = frame_size.max(max_radius * 2.0);

    let zero_segment = (Pos2::ZERO, Pos2::ZERO);

    if from.x + frame_size <= to.x - frame_size {
        if f32::abs(from.y - to.y) < 1.0 {
            // Single segment case.
            AxisAlignedWire {
                aabb: Rect::from_two_pos(from, to),
                segments: [
                    (from, to),
                    zero_segment,
                    zero_segment,
                    zero_segment,
                    zero_segment,
                ],
                turns: 0,
                turn_centers: [Pos2::ZERO; 4],
                turn_radii: [f32::NAN; 4],
            }
        } else {
            // Two turns case.
            let mid_x = f32::midpoint(from.x, to.x);
            let half_width = (to.x - from.x) / 2.0;

            let turn_radius = max_radius.min(half_width);

            let turn_vert_len = if from.y < to.y {
                turn_radius
            } else {
                -turn_radius
            };

            let segments = [
                (from, pos2(mid_x - turn_radius, from.y)),
                (
                    pos2(mid_x, from.y + turn_vert_len),
                    pos2(mid_x, to.y - turn_vert_len),
                ),
                (pos2(mid_x + turn_radius, to.y), to),
                zero_segment,
                zero_segment,
            ];

            let turn_centers = [
                pos2(mid_x - turn_radius, from.y + turn_vert_len),
                pos2(mid_x + turn_radius, to.y - turn_vert_len),
                Pos2::ZERO,
                Pos2::ZERO,
            ];

            let turn_radii = [turn_radius, turn_radius, f32::NAN, f32::NAN];

            AxisAlignedWire {
                aabb: Rect::from_two_pos(from, to),
                turns: 2,
                segments,
                turn_centers,
                turn_radii,
            }
        }
    } else {
        // Four turns case.
        let mid = f32::midpoint(from.y, to.y);

        let right = from.x + frame_size;
        let left = to.x - frame_size;

        let half_width = f32::abs(right - left) / 2.0;

        let ends_turn_radius = max_radius;
        let middle_turn_radius = max_radius.min(half_width);

        let ends_turn_vert_len = if from.y < to.y {
            ends_turn_radius
        } else {
            -ends_turn_radius
        };

        let middle_turn_vert_len = if from.y < to.y {
            middle_turn_radius
        } else {
            -middle_turn_radius
        };

        let segments = [
            (from, pos2(right - ends_turn_radius, from.y)),
            (
                pos2(right, from.y + ends_turn_vert_len),
                pos2(right, mid - middle_turn_vert_len),
            ),
            (
                pos2(right - middle_turn_radius, mid),
                pos2(left + middle_turn_radius, mid),
            ),
            (
                pos2(left, mid + middle_turn_vert_len),
                pos2(left, to.y - ends_turn_vert_len),
            ),
            (pos2(left + ends_turn_radius, to.y), to),
        ];

        let turn_centers = [
            pos2(right - ends_turn_radius, from.y + ends_turn_vert_len),
            pos2(right - middle_turn_radius, mid - middle_turn_vert_len),
            pos2(left + middle_turn_radius, mid + middle_turn_vert_len),
            pos2(left + ends_turn_radius, to.y - ends_turn_vert_len),
        ];

        let turn_radii = [
            ends_turn_radius,
            middle_turn_radius,
            middle_turn_radius,
            ends_turn_radius,
        ];

        AxisAlignedWire {
            aabb: Rect::from_min_max(
                pos2(f32::min(left, from.x), f32::min(from.y, to.y)),
                pos2(f32::max(right, to.x), f32::max(from.y, to.y)),
            ),
            turns: 4,
            segments,
            turn_centers,
            turn_radii,
        }
    }
}

/// Very basic lower-bound algorithm
/// Finds the smallest number in range [min, max) that satisfies the predicate
/// If no such number exists, returns max
///
/// For the algorithm to work, the predicate must be monotonic
/// i.e. if f(i) is true, then f(j) is true for all j within (i, max)
/// and if f(i) is false, then f(j) is false for all j within [min, i)
fn lower_bound(min: usize, max: usize, f: impl Fn(usize) -> bool) -> usize {
    #![allow(clippy::similar_names)]

    let mut min = min;
    let mut max = max;

    while min < max {
        let mid = usize::midpoint(min, max);
        if f(mid) {
            max = mid;
        } else {
            min = mid + 1;
        }
    }

    max

    // for i in min..max {
    //     if f(i) {
    //         return i;
    //     }
    // }
    // max
}

/// The two pins a wire joins.
///
/// This is the identity of a wire: it does not change while the wire is moved, restyled,
/// rerouted, or shown in a different view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WirePins {
    /// Output pin the wire starts at.
    pub out_pin: OutPinId,

    /// Input pin the wire ends at.
    pub in_pin: InPinId,
}

impl WirePins {
    /// The wire from `out_pin` to `in_pin`.
    #[must_use]
    #[inline]
    pub const fn new(out_pin: OutPinId, in_pin: InPinId) -> Self {
        Self { out_pin, in_pin }
    }
}

/// Everything needed to lay out the path of one wire, in graph coordinates.
///
/// Passed to [`SnarlViewer::wire_route`](super::SnarlViewer::wire_route).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireRoute {
    /// Pins the wire joins, or `None` for a wire that is still being dragged.
    pub pins: Option<WirePins>,

    /// Measured centre of the output pin, or the pointer for a wire dragged from an input.
    pub from: Pos2,

    /// Measured centre of the input pin, or the pointer for a wire dragged from an output.
    pub to: Pos2,

    /// Style picked from the two pins.
    pub style: WireStyle,

    /// Length of the straight lead out of each pin, already adjusted to the distance between them.
    pub frame_size: f32,

    /// Largest distance the path may stray from the ideal curve.
    pub tolerance: f32,

    /// Screen points per graph unit.
    ///
    /// A route that keeps some length the same on screen at every zoom divides that length
    /// by this.
    pub scale: f32,

    /// Displacement asked of the middle of the wire by [`SnarlStyle::wire_slack`](super::SnarlStyle::wire_slack)
    /// and [`SnarlStyle::wire_sway`](super::SnarlStyle::wire_sway). Zero when both are off.
    pub sag: Vec2,
}

impl WireRoute {
    /// The path the graph view uses when the viewer supplies none.
    #[must_use]
    pub fn default_route(&self) -> CableGeometry {
        let Self {
            from,
            to,
            frame_size,
            tolerance,
            ..
        } = *self;

        let base = match self.style {
            WireStyle::Line => CableGeometry::line(from, to),
            WireStyle::Bezier3 => {
                let points = wire_bezier_3(frame_size, from, to);
                let samples = bezier_draw_samples_number_3(&points, tolerance);
                CableGeometry::from_points(sampled_bezier(&points, samples))
            }
            WireStyle::Bezier5 => {
                let points = wire_bezier_5(frame_size, from, to);
                let samples = bezier_draw_samples_number_5(&points, tolerance);
                CableGeometry::from_points(sampled_bezier(&points, samples))
            }
            WireStyle::AxisAligned { corner_radius } => {
                let wire = wire_axis_aligned(corner_radius, frame_size, from, to);
                CableGeometry::from_points(axis_aligned_line(&wire))
            }
        };

        if self.sag == Vec2::ZERO {
            base
        } else {
            base.sagged(self.sag, tolerance)
        }
    }
}

fn sampled_bezier(points: &[Pos2], samples: usize) -> impl Iterator<Item = Pos2> + '_ {
    let samples = samples.max(2);
    (0..samples).map(move |i| {
        #[allow(clippy::cast_precision_loss)]
        let t = i as f32 / (samples - 1) as f32;
        sample_bezier(points, t)
    })
}

fn axis_aligned_line(wire: &AxisAlignedWire) -> Vec<Pos2> {
    let mut line = Vec::new();

    for i in 0..wire.turns {
        line.push(wire.segments[i].0);
        line.push(wire.segments[i].1);

        if wire.turn_radii[i] > 0.0 {
            let turn = wire.turn_centers[i];
            // Corners have always been drawn this finely, whatever smoothness was asked for.
            let samples = MAX_CURVE_SAMPLES;

            let start = wire.segments[i].1;
            let end = wire.segments[i + 1].0;

            let sin_x = end.x - turn.x;
            let cos_x = start.x - turn.x;

            let sin_y = end.y - turn.y;
            let cos_y = start.y - turn.y;

            for j in 1..samples {
                #[allow(clippy::cast_precision_loss)]
                let a = std::f32::consts::FRAC_PI_2 * (j as f32 / samples as f32);

                let (sin_a, cos_a) = a.sin_cos();

                line.push(pos2(
                    cos_x.mul_add(cos_a, sin_x.mul_add(sin_a, turn.x)),
                    cos_y.mul_add(cos_a, sin_y.mul_add(sin_a, turn.y)),
                ));
            }
        }
    }

    line.push(wire.segments[wire.turns].0);
    line.push(wire.segments[wire.turns].1);

    line
}

struct CachedRoute {
    generation: u32,
    route: WireRoute,
    geometry: Arc<CableGeometry>,
}

#[derive(Default)]
struct RouteCache {
    generation: u32,
    routes: HashMap<WireId, CachedRoute>,
}

impl CacheTrait for RouteCache {
    fn update(&mut self) {
        self.routes
            .retain(|_, cached| cached.generation == self.generation);

        self.generation = self.generation.wrapping_add(1);
    }

    fn len(&self) -> usize {
        self.routes.len()
    }
}

/// Returns the default path for `route`, rebuilding it only when the request changed.
pub fn cached_default_route(ctx: &Context, wire: WireId, route: &WireRoute) -> Arc<CableGeometry> {
    ctx.memory_mut(|memory| {
        let cache = memory.caches.cache::<RouteCache>();
        let generation = cache.generation;

        match cache.routes.entry(wire) {
            Entry::Occupied(mut entry) => {
                let cached = entry.get_mut();
                cached.generation = generation;
                if cached.route != *route {
                    cached.route = *route;
                    cached.geometry = Arc::new(route.default_route());
                }
                Arc::clone(&cached.geometry)
            }
            Entry::Vacant(entry) => Arc::clone(
                &entry
                    .insert(CachedRoute {
                        generation,
                        route: *route,
                        geometry: Arc::new(route.default_route()),
                    })
                    .geometry,
            ),
        }
    })
}

/// A filled disc that rides on a wire.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireBead {
    /// Fraction of the wire's arc length the disc is centred at, in `0.0..=1.0`.
    pub share: f32,

    /// Radius of the disc, in graph coordinates.
    pub radius: f32,

    /// Colour the disc is filled with.
    pub fill: Color32,
}

/// How one wire is painted: strokes laid back to front along the wire's one path, then discs
/// placed on it.
///
/// The path is the same [`CableGeometry`] the wire is hit tested with, so a wire drawn as a
/// wide glowing tube is still picked along exactly the curve the eye sees.
/// Colours and widths carry no meaning to the graph view; the viewer decides what they stand for.
///
/// A viewer that needs more than strokes and discs adds shapes of its own with
/// [`WirePaint::under_shape`] and [`WirePaint::over_shape`]. They are painted with the wire:
/// in its layer, inside the clip of the view, in graph coordinates, behind and in front of
/// its strokes and discs.
#[derive(Clone, Debug, PartialEq)]
pub struct WirePaint {
    shapes_under: Vec<Shape>,
    strokes: SmallVec<[Stroke; 4]>,
    beads: SmallVec<[WireBead; 4]>,
    shapes_over: Vec<Shape>,
    hit_width: f32,
    layer: Option<WireLayer>,
}

impl WirePaint {
    /// A wire painted as a single stroke.
    #[must_use]
    pub fn solid(stroke: impl Into<Stroke>) -> Self {
        let stroke = stroke.into();
        Self {
            shapes_under: Vec::new(),
            strokes: smallvec::smallvec![stroke],
            beads: SmallVec::new(),
            shapes_over: Vec::new(),
            hit_width: stroke.width,
            layer: None,
        }
    }

    /// A wire that paints nothing.
    ///
    /// It keeps its path, hover, selection and actions, so a viewer can draw the wire itself
    /// from the measured [`CableGeometry`].
    #[must_use]
    pub fn hidden() -> Self {
        Self {
            shapes_under: Vec::new(),
            strokes: SmallVec::new(),
            beads: SmallVec::new(),
            shapes_over: Vec::new(),
            hit_width: 0.0,
            layer: None,
        }
    }

    /// Adds a stroke behind everything painted so far.
    #[must_use]
    pub fn under(mut self, stroke: impl Into<Stroke>) -> Self {
        self.strokes.insert(0, stroke.into());
        self
    }

    /// Adds a stroke in front of everything painted so far.
    #[must_use]
    pub fn over(mut self, stroke: impl Into<Stroke>) -> Self {
        self.strokes.push(stroke.into());
        self
    }

    /// Adds a soft halo of `color` behind the wire, reaching `spread` beyond the widest stroke
    /// on each side.
    #[must_use]
    pub fn with_glow(mut self, color: Color32, spread: f32) -> Self {
        const FALLOFF: [(f32, f32); 3] = [(1.0, 0.10), (0.6, 0.16), (0.25, 0.28)];

        if !(spread > 0.0) || !spread.is_finite() {
            return self;
        }

        let widest = self.widest();
        let halo = FALLOFF.map(|(reach, strength)| {
            Stroke::new(
                (2.0 * spread).mul_add(reach, widest),
                color.gamma_multiply(strength),
            )
        });

        self.strokes.insert_many(0, halo);
        self
    }

    /// Adds a filled disc of `radius` centred on the wire, `share` of the way along its length.
    ///
    /// Discs are painted in front of the wire's strokes, in the order they were added. The
    /// view places each one on the path it paints and hit tests, whatever route that is, so a
    /// viewer animates a disc by giving a different `share` on each pass.
    #[must_use]
    pub fn with_bead(mut self, share: f32, radius: f32, fill: Color32) -> Self {
        self.beads.push(WireBead {
            share,
            radius,
            fill,
        });
        self
    }

    /// Adds a shape behind the wire's strokes and discs, and behind the shapes added this
    /// way before it.
    ///
    /// The shape is in graph coordinates, as the wire's path is. Build it from the path the
    /// view hands to [`SnarlViewer::wire_paint`](super::SnarlViewer::wire_paint), for example
    /// with [`CableGeometry::ribbon`] or [`CableGeometry::walls`], and it is painted where
    /// the wire is picked.
    #[must_use]
    pub fn under_shape(mut self, shape: impl Into<Shape>) -> Self {
        self.shapes_under.insert(0, shape.into());
        self
    }

    /// Adds a shape in front of the wire's strokes and discs, and in front of the shapes
    /// added this way before it. See [`WirePaint::under_shape`].
    #[must_use]
    pub fn over_shape(mut self, shape: impl Into<Shape>) -> Self {
        self.shapes_over.push(shape.into());
        self
    }

    /// Sets the width of the band in which the pointer counts as being on the wire.
    ///
    /// Defaults to the width of the stroke given to [`WirePaint::solid`]. A glow does not widen it,
    /// and neither does a bead or a shape. A path with half widths is picked between its
    /// walls whatever this width is.
    /// The view never picks more narrowly than [`SnarlStyle::wire_hit_tolerance`](super::SnarlStyle::wire_hit_tolerance).
    #[must_use]
    pub const fn with_hit_width(mut self, width: f32) -> Self {
        self.hit_width = width;
        self
    }

    /// Paints this wire behind or above the nodes, whatever
    /// [`SnarlStyle::wire_layer`](super::SnarlStyle::wire_layer) says for the rest.
    ///
    /// Only the painting moves. A node still takes the pointer where it covers the wire.
    #[must_use]
    pub const fn with_layer(mut self, layer: WireLayer) -> Self {
        self.layer = Some(layer);
        self
    }

    /// Strokes painted along the wire, back to front.
    #[must_use]
    #[inline]
    pub fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }

    /// Discs placed on the wire, back to front.
    #[must_use]
    #[inline]
    pub fn beads(&self) -> &[WireBead] {
        &self.beads
    }

    /// Shapes painted behind the wire's strokes, back to front.
    #[must_use]
    #[inline]
    pub fn shapes_under(&self) -> &[Shape] {
        &self.shapes_under
    }

    /// Shapes painted in front of the wire's discs, back to front.
    #[must_use]
    #[inline]
    pub fn shapes_over(&self) -> &[Shape] {
        &self.shapes_over
    }

    /// Width of the band in which the pointer counts as being on the wire.
    #[must_use]
    #[inline]
    pub const fn hit_width(&self) -> f32 {
        self.hit_width
    }

    /// The layer asked for with [`WirePaint::with_layer`], or `None` to follow the style.
    #[must_use]
    #[inline]
    pub const fn layer(&self) -> Option<WireLayer> {
        self.layer
    }

    fn widest(&self) -> f32 {
        self.strokes
            .iter()
            .map(|stroke| stroke.width)
            .fold(0.0, f32::max)
    }

    /// Appends the shapes that paint `geometry` this way, unless it lies outside `clip_rect`.
    pub fn paint(&self, geometry: &CableGeometry, clip_rect: Rect, shapes: &mut Vec<Shape>) {
        let strokes = self
            .strokes
            .iter()
            .filter(|stroke| stroke.width > 0.0 && stroke.color != Color32::TRANSPARENT);

        let beads = self
            .beads
            .iter()
            .filter(|bead| bead.radius > 0.0 && bead.fill != Color32::TRANSPARENT);

        let reach = beads
            .clone()
            .map(|bead| bead.radius)
            .fold(self.widest() * 0.5, f32::max);

        let mut covered = geometry.bounds().expand(reach);
        for shape in self.shapes_under.iter().chain(&self.shapes_over) {
            covered |= shape.visual_bounding_rect();
        }

        if !covered.intersects(clip_rect) {
            return;
        }

        shapes.extend(self.shapes_under.iter().cloned());
        shapes.extend(strokes.map(|&stroke| geometry.stroke(stroke)));
        shapes.extend(beads.map(|bead| {
            Shape::circle_filled(geometry.anchor_at(bead.share).pos, bead.radius, bead.fill)
        }));
        shapes.extend(self.shapes_over.iter().cloned());
    }
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Rect, Shape, Stroke, pos2, vec2};

    use crate::{InPinId, NodeId, OutPinId};

    use super::{WirePaint, WirePins, WireRoute, WireStyle};

    fn route(style: WireStyle) -> WireRoute {
        WireRoute {
            pins: Some(WirePins::new(
                OutPinId {
                    node: NodeId(0),
                    output: 0,
                },
                InPinId {
                    node: NodeId(1),
                    input: 0,
                },
            )),
            from: pos2(10.0, 20.0),
            to: pos2(260.0, 140.0),
            style,
            frame_size: 30.0,
            tolerance: 1.0,
            scale: 1.0,
            sag: vec2(0.0, 0.0),
        }
    }

    const STYLES: [WireStyle; 4] = [
        WireStyle::Line,
        WireStyle::AxisAligned { corner_radius: 8.0 },
        WireStyle::Bezier3,
        WireStyle::Bezier5,
    ];

    #[test]
    fn every_style_routes_between_the_measured_pin_centres() {
        for style in STYLES {
            let request = route(style);
            let cable = request.default_route();

            assert_eq!(cable.start(), request.from, "{style:?}");
            assert_eq!(cable.end(), request.to, "{style:?}");
            assert!(cable.length() >= request.from.distance(request.to) - 1e-3);

            // Pins that sit behind each other still get a finite path.
            let backwards = WireRoute {
                from: request.to,
                to: request.from,
                ..request
            };
            let cable = backwards.default_route();
            assert_eq!(cable.start(), backwards.from, "{style:?}");
            assert_eq!(cable.end(), backwards.to, "{style:?}");
            assert!(cable.length().is_finite());
        }
    }

    #[test]
    fn slack_moves_the_middle_and_keeps_the_ends() {
        for style in STYLES {
            let taut = route(style);
            let slack = WireRoute {
                sag: vec2(0.0, 30.0),
                ..taut
            };

            let taut_cable = taut.default_route();
            let slack_cable = slack.default_route();

            assert_eq!(slack_cable.start(), taut.from, "{style:?}");
            assert_eq!(slack_cable.end(), taut.to, "{style:?}");

            let lowered = slack_cable.midpoint().pos.y - taut_cable.midpoint().pos.y;
            assert!(lowered > 20.0 && lowered < 40.0, "{style:?}: {lowered}");
        }
    }

    #[test]
    fn painted_strokes_follow_the_hit_tested_path() {
        let cable = route(WireStyle::Bezier5).default_route();
        let paint = WirePaint::solid((3.0, Color32::WHITE))
            .with_glow(Color32::from_rgb(255, 120, 40), 6.0)
            .over((1.0, Color32::BLACK));

        assert_eq!(paint.strokes().len(), 5);
        assert_eq!(paint.hit_width().to_bits(), 3.0f32.to_bits());

        // Back to front: widest halo first, highlight last.
        let widths: Vec<f32> = paint.strokes().iter().map(|stroke| stroke.width).collect();
        assert!(widths[0] > widths[1] && widths[1] > widths[2] && widths[2] > widths[3]);
        assert_eq!(widths[0].to_bits(), 15.0f32.to_bits());
        assert_eq!(paint.strokes()[4], Stroke::new(1.0, Color32::BLACK));

        let mut shapes = Vec::new();
        paint.paint(&cable, Rect::EVERYTHING, &mut shapes);
        assert_eq!(shapes.len(), 5);

        for shape in &shapes {
            let Shape::Path(path) = shape else {
                panic!("a wire is painted as open paths, got {shape:?}");
            };
            assert!(!path.closed);
            assert_eq!(path.points, cable.points());
        }

        // Every painted vertex is on the path the pointer is tested against.
        for &point in cable.points() {
            assert!(cable.hit(point, 0.0).is_some());
        }
    }

    #[test]
    fn shapes_of_the_viewer_are_painted_around_the_strokes_and_count_for_culling() {
        let cable = route(WireStyle::Line).default_route();
        let mark = |x: f32, color| Shape::circle_filled(pos2(x, 500.0), 4.0, color);

        let paint = WirePaint::solid((3.0, Color32::WHITE))
            .with_bead(0.5, 4.0, Color32::GREEN)
            .under_shape(mark(1.0, Color32::RED))
            .over_shape(mark(2.0, Color32::RED))
            .under_shape(mark(3.0, Color32::RED))
            .over_shape(mark(4.0, Color32::RED));

        assert_eq!(paint.shapes_under().len(), 2);
        assert_eq!(paint.shapes_over().len(), 2);
        assert_eq!(paint.hit_width().to_bits(), 3.0f32.to_bits());

        let mut shapes = Vec::new();
        paint.paint(&cable, Rect::EVERYTHING, &mut shapes);

        let order: Vec<f32> = shapes
            .iter()
            .map(|shape| match shape {
                Shape::Circle(circle) if circle.fill == Color32::RED => circle.center.x,
                Shape::Circle(_) => -1.0,
                _ => 0.0,
            })
            .collect();
        // The shape put under last, the one before it, the stroke, the bead, then the
        // shapes put over in the order they were given.
        assert_eq!(order, [3.0, 1.0, 0.0, -1.0, 2.0, 4.0]);

        // A view that shows only where the shapes are still gets the wire painted.
        let around_marks = Rect::from_center_size(pos2(2.0, 500.0), vec2(4.0, 4.0));
        assert!(!cable.bounds().expand(10.0).intersects(around_marks));
        shapes.clear();
        paint.paint(&cable, around_marks, &mut shapes);
        assert_eq!(shapes.len(), 6);

        // A view that shows neither gets nothing.
        shapes.clear();
        paint.paint(
            &cable,
            Rect::from_center_size(pos2(-900.0, -900.0), vec2(10.0, 10.0)),
            &mut shapes,
        );
        assert!(shapes.is_empty());

        // Nothing is painted for a wire that was given no shape.
        assert!(WirePaint::hidden().shapes_under().is_empty());
        assert!(WirePaint::hidden().shapes_over().is_empty());
    }

    #[test]
    fn beads_are_placed_on_the_painted_path() {
        let cable = route(WireStyle::Bezier5).default_route();
        let paint = WirePaint::solid((3.0, Color32::WHITE))
            .with_bead(0.0, 4.0, Color32::RED)
            .with_bead(0.4, 5.0, Color32::GREEN)
            .with_bead(2.0, 6.0, Color32::BLUE)
            .with_bead(0.5, 0.0, Color32::RED)
            .with_bead(0.5, 3.0, Color32::TRANSPARENT);

        assert_eq!(paint.beads().len(), 5);
        assert_eq!(paint.hit_width().to_bits(), 3.0f32.to_bits());

        let mut shapes = Vec::new();
        paint.paint(&cable, Rect::EVERYTHING, &mut shapes);

        // The stroke first, then the three discs that have a size and a colour.
        let [Shape::Path(_), discs @ ..] = shapes.as_slice() else {
            panic!("the stroke is painted first, got {shapes:?}");
        };
        assert_eq!(discs.len(), 3);

        for (disc, (share, radius, fill)) in discs.iter().zip([
            (0.0, 4.0f32, Color32::RED),
            (0.4, 5.0, Color32::GREEN),
            (1.0, 6.0, Color32::BLUE),
        ]) {
            let Shape::Circle(disc) = disc else {
                panic!("a bead is a disc, got {disc:?}");
            };
            assert_eq!(disc.center, cable.anchor_at(share).pos);
            assert!(cable.hit(disc.center, 1e-3).is_some());
            assert_eq!(disc.radius.to_bits(), radius.to_bits());
            assert_eq!(disc.fill, fill);
        }

        // A wire may be nothing but beads.
        let mut shapes = Vec::new();
        WirePaint::hidden().with_bead(0.5, 4.0, Color32::RED).paint(
            &cable,
            Rect::EVERYTHING,
            &mut shapes,
        );
        assert!(matches!(shapes.as_slice(), [Shape::Circle(_)]));

        // A bead that reaches into the clip rectangle is painted even when the path stops
        // short of it.
        let past_the_end = Rect::from_min_size(cable.end() + vec2(3.0, -40.0), vec2(50.0, 80.0));
        let mut shapes = Vec::new();
        WirePaint::solid((3.0, Color32::WHITE)).paint(&cable, past_the_end, &mut shapes);
        assert!(shapes.is_empty());
        paint.paint(&cable, past_the_end, &mut shapes);
        assert_eq!(shapes.len(), 4);
    }

    #[test]
    fn hidden_and_offscreen_wires_paint_nothing() {
        let cable = route(WireStyle::Bezier3).default_route();
        let mut shapes = Vec::new();

        WirePaint::hidden().paint(&cable, Rect::EVERYTHING, &mut shapes);
        assert!(shapes.is_empty());

        let far_away = Rect::from_min_size(pos2(5000.0, 5000.0), vec2(100.0, 100.0));
        WirePaint::solid((2.0, Color32::WHITE)).paint(&cable, far_away, &mut shapes);
        assert!(shapes.is_empty());

        WirePaint::solid((2.0, Color32::TRANSPARENT)).paint(&cable, Rect::EVERYTHING, &mut shapes);
        assert!(shapes.is_empty());

        // A stroke thinner than one graph unit keeps the width it was given: it may be several
        // points wide on screen in a magnified view. egui fades what is thinner than a pixel.
        WirePaint::solid((0.5, Color32::WHITE)).paint(&cable, Rect::EVERYTHING, &mut shapes);
        let [Shape::Path(path)] = shapes.as_slice() else {
            panic!("expected one path");
        };
        assert_eq!(path.stroke.width.to_bits(), 0.5f32.to_bits());
        assert_eq!(
            path.stroke.color,
            egui::epaint::ColorMode::Solid(Color32::WHITE)
        );
    }
}
