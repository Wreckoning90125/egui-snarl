//! One cable geometry shared by painting, hit testing, bounds and anchors.

use egui::{Pos2, Rect, Shape, Stroke, Vec2, emath::TSTransform, vec2};
use smallvec::SmallVec;

/// Upper bound on the segments a sag adds to one cable.
const MAX_SAG_SEGMENTS: usize = 64;

/// Upper bound on the segments one Bezier is flattened into.
const MAX_BEZIER_SEGMENTS: usize = 100;

/// Tolerance used when the caller supplies one that is not a positive finite number.
const FALLBACK_TOLERANCE: f32 = 0.25;

/// A point on a cable together with the cable's direction there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CableAnchor {
    /// Position on the cable.
    pub pos: Pos2,

    /// Unit direction of travel from the cable's start to its end.
    pub tangent: Vec2,

    /// Unit vector a quarter turn clockwise on screen from [`CableAnchor::tangent`].
    pub normal: Vec2,

    /// Fraction of the cable's arc length at which this point lies, in `0.0..=1.0`.
    pub share: f32,
}

/// The point of a cable nearest to some position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CableHit {
    /// Nearest point on the cable.
    pub pos: Pos2,

    /// Distance from the tested position to [`CableHit::pos`].
    pub distance: f32,

    /// Fraction of the cable's arc length at which [`CableHit::pos`] lies, in `0.0..=1.0`.
    pub share: f32,
}

/// The path of one wire, as a polyline.
///
/// The same points are stroked, hit tested, bounded and measured, so a wire can never be
/// painted along one curve and picked along another.
/// Coordinates are whatever space the points were given in; the graph view uses graph space.
#[derive(Clone, Debug, PartialEq)]
pub struct CableGeometry {
    points: Vec<Pos2>,
    lengths: Vec<f32>,
    bounds: Rect,
}

impl CableGeometry {
    /// Builds a cable through `points`.
    ///
    /// Non-finite points and repeated consecutive points are dropped.
    /// The result always holds at least two points, so a degenerate input gives a cable of zero length.
    #[must_use]
    pub fn from_points(points: impl IntoIterator<Item = Pos2>) -> Self {
        let mut points: Vec<Pos2> = points
            .into_iter()
            .filter(|point| point.x.is_finite() && point.y.is_finite())
            .collect();
        points.dedup();

        match points.len() {
            0 => points = vec![Pos2::ZERO; 2],
            1 => points.push(points[0]),
            _ => {}
        }

        let mut lengths = Vec::with_capacity(points.len());
        let mut total = 0.0;
        lengths.push(total);
        for pair in points.windows(2) {
            total += pair[0].distance(pair[1]);
            lengths.push(total);
        }

        let bounds = Rect::from_points(&points);

        Self {
            points,
            lengths,
            bounds,
        }
    }

    /// Builds a straight cable.
    #[must_use]
    pub fn line(from: Pos2, to: Pos2) -> Self {
        Self::from_points([from, to])
    }

    /// Flattens a Bezier curve of any degree into a cable.
    ///
    /// `tolerance` is the largest allowed distance between the curve and the cable.
    #[must_use]
    pub fn bezier(control: &[Pos2], tolerance: f32) -> Self {
        if control.len() < 3 {
            return Self::from_points(control.iter().copied());
        }

        let tolerance = sane_tolerance(tolerance);

        let mut bend = 0.0f32;
        for triple in control.windows(3) {
            let second_difference =
                triple[2].to_vec2() - 2.0 * triple[1].to_vec2() + triple[0].to_vec2();
            bend = bend.max(second_difference.length());
        }

        #[allow(clippy::cast_precision_loss)]
        let degree = (control.len() - 1) as f32;

        // A Bezier flattened into `n` equal parameter steps strays from its chords by at most
        // `degree * (degree - 1) / 8 * bend / n^2`.
        let segments = (degree * (degree - 1.0) / 8.0 * bend / tolerance)
            .sqrt()
            .ceil();

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let segments = if segments.is_finite() {
            (segments as usize).clamp(1, MAX_BEZIER_SEGMENTS)
        } else {
            MAX_BEZIER_SEGMENTS
        };

        Self::from_points((0..=segments).map(|step| {
            #[allow(clippy::cast_precision_loss)]
            let t = step as f32 / segments as f32;
            de_casteljau(control, t)
        }))
    }

    /// Returns this cable with its middle displaced by `sag`, leaving both ends where they are.
    ///
    /// The displacement follows a parabola along the cable's arc length.
    /// Straight runs are subdivided so the sagged cable stays within `tolerance` of that parabola.
    #[must_use]
    pub fn sagged(&self, sag: Vec2, tolerance: f32) -> Self {
        let total = self.length();
        let depth = sag.length();

        if !depth.is_finite() || depth <= 0.0 || total <= 0.0 {
            return self.clone();
        }

        let segments = (depth / sane_tolerance(tolerance)).sqrt().ceil();

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let segments = if segments.is_finite() {
            (segments as usize).clamp(2, MAX_SAG_SEGMENTS)
        } else {
            MAX_SAG_SEGMENTS
        };

        #[allow(clippy::cast_precision_loss)]
        let step = total / segments as f32;

        let mut points = Vec::with_capacity(self.points.len() + segments);
        points.push(self.points[0]);

        for index in 1..self.points.len() {
            let from = self.points[index - 1];
            let to = self.points[index];
            let from_length = self.lengths[index - 1];
            let run = self.lengths[index] - from_length;

            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pieces = ((run / step).ceil() as usize).clamp(1, MAX_SAG_SEGMENTS);

            for piece in 1..=pieces {
                #[allow(clippy::cast_precision_loss)]
                let t = piece as f32 / pieces as f32;
                let share = (run.mul_add(t, from_length) / total).clamp(0.0, 1.0);
                points.push(from.lerp(to, t) + sag * (4.0 * share * (1.0 - share)));
            }
        }

        Self::from_points(points)
    }

    /// Returns this cable mapped through `transform`.
    #[must_use]
    pub fn transformed(&self, transform: TSTransform) -> Self {
        Self::from_points(self.points.iter().map(|&point| transform * point))
    }

    /// The points of the cable, from its start to its end.
    #[must_use]
    #[inline]
    pub fn points(&self) -> &[Pos2] {
        &self.points
    }

    /// First point of the cable.
    #[must_use]
    #[inline]
    pub fn start(&self) -> Pos2 {
        self.points[0]
    }

    /// Last point of the cable.
    #[must_use]
    #[inline]
    pub fn end(&self) -> Pos2 {
        self.points[self.points.len() - 1]
    }

    /// Smallest rectangle holding every point of the cable.
    #[must_use]
    #[inline]
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Arc length of the cable.
    #[must_use]
    #[inline]
    pub fn length(&self) -> f32 {
        self.lengths[self.lengths.len() - 1]
    }

    /// The point `share` of the way along the cable's arc length, with the direction there.
    ///
    /// `share` is clamped to `0.0..=1.0`.
    #[must_use]
    pub fn anchor_at(&self, share: f32) -> CableAnchor {
        let share = if share.is_finite() {
            share.clamp(0.0, 1.0)
        } else {
            0.5
        };

        let total = self.length();
        if total <= 0.0 {
            return CableAnchor {
                pos: self.points[0],
                tangent: Vec2::X,
                normal: Vec2::Y,
                share,
            };
        }

        let target = total * share;
        let index = self
            .lengths
            .partition_point(|&length| length < target)
            .clamp(1, self.points.len() - 1);

        let from = self.points[index - 1];
        let to = self.points[index];
        let run = self.lengths[index] - self.lengths[index - 1];
        let t = if run > 0.0 {
            ((target - self.lengths[index - 1]) / run).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let tangent = unit_or_x(to - from);

        CableAnchor {
            pos: from.lerp(to, t),
            tangent,
            normal: vec2(-tangent.y, tangent.x),
            share,
        }
    }

    /// The point halfway along the cable's arc length.
    #[must_use]
    #[inline]
    pub fn midpoint(&self) -> CableAnchor {
        self.anchor_at(0.5)
    }

    /// The point of the cable nearest to `pos`.
    #[must_use]
    pub fn closest(&self, pos: Pos2) -> CableHit {
        let mut best_pos = self.points[0];
        let mut best_distance_sq = f32::INFINITY;
        let mut best_length = 0.0;

        for index in 1..self.points.len() {
            let from = self.points[index - 1];
            let segment = self.points[index] - from;
            let length_sq = segment.length_sq();

            let t = if length_sq > 0.0 {
                ((pos - from).dot(segment) / length_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let nearest = from + segment * t;
            let distance_sq = nearest.distance_sq(pos);

            if distance_sq < best_distance_sq {
                best_distance_sq = distance_sq;
                best_pos = nearest;
                best_length =
                    (self.lengths[index] - self.lengths[index - 1]).mul_add(t, self.lengths[index - 1]);
            }
        }

        let total = self.length();

        CableHit {
            pos: best_pos,
            distance: best_distance_sq.sqrt(),
            share: if total > 0.0 {
                (best_length / total).clamp(0.0, 1.0)
            } else {
                0.0
            },
        }
    }

    /// The point of the cable nearest to `pos`, if it lies within `tolerance` of it.
    #[must_use]
    pub fn hit(&self, pos: Pos2, tolerance: f32) -> Option<CableHit> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return None;
        }

        if !self.bounds.expand(tolerance).contains(pos) {
            return None;
        }

        let hit = self.closest(pos);
        (hit.distance <= tolerance).then_some(hit)
    }

    /// The cable as a stroked shape.
    #[must_use]
    pub fn stroke(&self, stroke: impl Into<Stroke>) -> Shape {
        Shape::line(self.points.clone(), stroke.into())
    }
}

/// Bounded spring that lets the middle of a cable lag behind its moving ends.
///
/// It only displaces the painted and hit-tested path. It never changes which pins a wire joins.
/// The same sequence of [`CableSway::advance`] calls always yields the same offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CableSway {
    offset: Vec2,
    velocity: Vec2,
}

impl CableSway {
    /// Length of one integration step, in seconds.
    pub const STEP: f32 = 1.0 / 120.0;

    /// Most integration steps taken by one [`CableSway::advance`] call.
    pub const MAX_STEPS: u32 = 8;

    const STIFFNESS: f32 = 170.0;
    const DAMPING: f32 = 14.0;
    const REST_OFFSET: f32 = 0.05;
    const REST_VELOCITY: f32 = 0.5;

    /// Current displacement of the cable's middle.
    #[must_use]
    #[inline]
    pub const fn offset(&self) -> Vec2 {
        self.offset
    }

    /// Whether the cable hangs still.
    #[must_use]
    #[inline]
    pub fn is_resting(&self) -> bool {
        self.offset == Vec2::ZERO && self.velocity == Vec2::ZERO
    }

    /// Advances the spring by `dt` seconds after the cable's resting middle moved by `motion`.
    ///
    /// `amount` in `0.0..=1.0` is how much of `motion` the middle lags behind by.
    /// The displacement never exceeds `limit`.
    /// Returns whether the cable is still moving and so needs another frame.
    pub fn advance(&mut self, motion: Vec2, amount: f32, limit: f32, dt: f32) -> bool {
        if !(amount > 0.0 && limit > 0.0) || !limit.is_finite() {
            *self = Self::default();
            return false;
        }

        if motion.x.is_finite() && motion.y.is_finite() {
            self.offset -= motion * amount.min(1.0);
        }
        self.offset = clamp_length(self.offset, limit);

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = if dt.is_finite() && dt > 0.0 {
            ((dt / Self::STEP).round() as u32).clamp(1, Self::MAX_STEPS)
        } else {
            0
        };

        for _ in 0..steps {
            let acceleration = -Self::STIFFNESS * self.offset - Self::DAMPING * self.velocity;
            self.velocity += acceleration * Self::STEP;
            self.offset += self.velocity * Self::STEP;
        }
        self.offset = clamp_length(self.offset, limit);

        if self.offset.length() < Self::REST_OFFSET && self.velocity.length() < Self::REST_VELOCITY
        {
            *self = Self::default();
            false
        } else {
            true
        }
    }
}

fn sane_tolerance(tolerance: f32) -> f32 {
    if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        FALLBACK_TOLERANCE
    }
}

fn unit_or_x(vector: Vec2) -> Vec2 {
    let length = vector.length();
    if length.is_finite() && length > 0.0 {
        vector / length
    } else {
        Vec2::X
    }
}

fn clamp_length(vector: Vec2, limit: f32) -> Vec2 {
    let length = vector.length();
    if !length.is_finite() {
        Vec2::ZERO
    } else if length > limit {
        vector * (limit / length)
    } else {
        vector
    }
}

fn de_casteljau(control: &[Pos2], t: f32) -> Pos2 {
    let mut points: SmallVec<[Pos2; 8]> = SmallVec::from_slice(control);

    for level in (1..points.len()).rev() {
        for index in 0..level {
            points[index] = points[index].lerp(points[index + 1], t);
        }
    }

    points[0]
}

#[cfg(test)]
mod tests {
    use egui::{emath::TSTransform, pos2, vec2};

    use super::{CableGeometry, CableSway};

    fn elbow() -> CableGeometry {
        CableGeometry::from_points([pos2(0.0, 0.0), pos2(100.0, 0.0), pos2(100.0, 50.0)])
    }

    #[test]
    fn degenerate_input_still_gives_a_usable_cable() {
        for cable in [
            CableGeometry::from_points([]),
            CableGeometry::from_points([pos2(3.0, 4.0)]),
            CableGeometry::from_points([pos2(3.0, 4.0), pos2(f32::NAN, 1.0), pos2(3.0, 4.0)]),
        ] {
            assert_eq!(cable.points().len(), 2);
            assert_eq!(cable.length().to_bits(), 0.0f32.to_bits());
            assert_eq!(cable.midpoint().pos, cable.start());
            assert_eq!(cable.midpoint().tangent, vec2(1.0, 0.0));
            assert!(cable.hit(cable.start(), 1.0).is_some());
        }
    }

    #[test]
    fn midpoint_and_tangent_follow_arc_length() {
        let cable = elbow();
        assert!((cable.length() - 150.0).abs() < 1e-4);

        let middle = cable.midpoint();
        assert!(middle.pos.distance(pos2(75.0, 0.0)) < 1e-4);
        assert_eq!(middle.tangent, vec2(1.0, 0.0));
        assert_eq!(middle.normal, vec2(0.0, 1.0));

        let late = cable.anchor_at(0.9);
        assert!(late.pos.distance(pos2(100.0, 35.0)) < 1e-4);
        assert_eq!(late.tangent, vec2(0.0, 1.0));

        assert_eq!(cable.anchor_at(0.0).pos, cable.start());
        assert_eq!(cable.anchor_at(1.0).pos, cable.end());
        assert_eq!(cable.anchor_at(7.0).pos, cable.end());
    }

    #[test]
    fn hit_accepts_exactly_the_stroked_path_within_tolerance() {
        let cable = elbow();

        let on_corner = cable.hit(pos2(103.0, -4.0), 5.0).unwrap();
        assert_eq!(on_corner.pos, pos2(100.0, 0.0));
        assert!((on_corner.distance - 5.0).abs() < 1e-4);
        assert!((on_corner.share - 100.0 / 150.0).abs() < 1e-4);

        assert!(cable.hit(pos2(50.0, 4.9), 5.0).is_some());
        assert!(cable.hit(pos2(50.0, 5.1), 5.0).is_none());

        // Inside the bounds of the cable, but far from either run.
        assert!(cable.hit(pos2(40.0, 40.0), 5.0).is_none());

        assert!(cable.hit(pos2(50.0, 0.0), -1.0).is_none());
        assert!(cable.hit(pos2(50.0, 0.0), f32::NAN).is_none());
    }

    #[test]
    fn hit_tolerance_in_screen_points_holds_under_pan_and_zoom() {
        let graph = CableGeometry::bezier(
            &[
                pos2(0.0, 0.0),
                pos2(120.0, 0.0),
                pos2(80.0, 160.0),
                pos2(200.0, 160.0),
            ],
            0.1,
        );
        let screen_tolerance = 6.0;

        for scaling in [0.25, 0.5, 1.0, 1.75, 3.0] {
            let to_screen = TSTransform::new(vec2(310.0, -45.0), scaling);
            let screen = graph.transformed(to_screen);

            for share in [0.0, 0.2, 0.5, 0.8, 1.0] {
                let graph_anchor = graph.anchor_at(share);
                let screen_anchor = screen.anchor_at(share);

                // Anchors commute with the view transform.
                assert!(
                    screen_anchor.pos.distance(to_screen * graph_anchor.pos) < 1e-2,
                    "scaling {scaling}, share {share}"
                );
                assert!((screen_anchor.tangent - graph_anchor.tangent).length() < 1e-3);

                for (reach, expected) in [(0.8, true), (1.25, false)] {
                    let screen_pos =
                        screen_anchor.pos + screen_anchor.normal * (screen_tolerance * reach);
                    let graph_pos = to_screen.inverse() * screen_pos;

                    // Bends can bring another part of the cable closer than the anchor, so only
                    // the near probe has a known answer everywhere; the far probe is checked
                    // for agreement between the two spaces.
                    let in_screen = screen.hit(screen_pos, screen_tolerance).is_some();
                    let in_graph = graph.hit(graph_pos, screen_tolerance / scaling).is_some();

                    assert_eq!(
                        in_screen, in_graph,
                        "scaling {scaling}, share {share}, reach {reach}"
                    );
                    if expected {
                        assert!(in_graph, "scaling {scaling}, share {share}");
                    }
                }
            }
        }
    }

    #[test]
    fn bounds_hold_every_point_and_follow_the_transform() {
        let cable = elbow();
        for &point in cable.points() {
            assert!(cable.bounds().contains(point));
        }

        let moved = cable.transformed(TSTransform::new(vec2(10.0, 20.0), 2.0));
        assert_eq!(moved.bounds().min, pos2(10.0, 20.0));
        assert_eq!(moved.bounds().max, pos2(210.0, 120.0));
        assert!((moved.length() - 300.0).abs() < 1e-3);
    }

    #[test]
    fn bezier_stays_within_tolerance_of_the_curve() {
        let control = [
            pos2(0.0, 0.0),
            pos2(150.0, 0.0),
            pos2(-50.0, 200.0),
            pos2(100.0, 200.0),
        ];

        for tolerance in [2.0, 0.5, 0.05] {
            let cable = CableGeometry::bezier(&control, tolerance);
            assert_eq!(cable.start(), control[0]);
            assert_eq!(cable.end(), control[3]);

            for step in 0..=400 {
                #[allow(clippy::cast_precision_loss)]
                let t = step as f32 / 400.0;
                let on_curve = super::de_casteljau(&control, t);
                assert!(
                    cable.closest(on_curve).distance <= tolerance,
                    "tolerance {tolerance}, t {t}"
                );
            }
        }

        let coarse = CableGeometry::bezier(&control, 2.0).points().len();
        let fine = CableGeometry::bezier(&control, 0.05).points().len();
        assert!(fine > coarse);
        assert!(fine <= super::MAX_BEZIER_SEGMENTS + 1);
    }

    #[test]
    fn sag_keeps_both_ends_and_lowers_the_middle() {
        let straight = CableGeometry::line(pos2(0.0, 0.0), pos2(200.0, 0.0));
        let sagged = straight.sagged(vec2(0.0, 40.0), 0.25);

        assert_eq!(sagged.start(), straight.start());
        assert_eq!(sagged.end(), straight.end());
        assert!(sagged.points().len() > 2);
        assert!(sagged.points().len() <= super::MAX_SAG_SEGMENTS + 1);

        let lowest = sagged
            .points()
            .iter()
            .map(|point| point.y)
            .fold(0.0, f32::max);
        assert!((lowest - 40.0).abs() < 0.5);
        assert!(sagged.hit(pos2(100.0, 40.0), 1.0).is_some());
        assert!(sagged.hit(pos2(100.0, 0.0), 1.0).is_none());

        assert_eq!(straight.sagged(vec2(0.0, 0.0), 0.25), straight);
        assert_eq!(straight.sagged(vec2(f32::NAN, 0.0), 0.25), straight);
    }

    #[test]
    fn sway_is_bounded_deterministic_and_comes_to_rest() {
        let run = || {
            let mut sway = CableSway::default();
            let mut trace = Vec::new();

            for frame in 0..600 {
                let motion = if frame < 30 {
                    vec2(12.0, -7.0)
                } else {
                    vec2(0.0, 0.0)
                };
                let moving = sway.advance(motion, 0.6, 48.0, 1.0 / 60.0);
                assert!(sway.offset().length() <= 48.0 + 1e-3);
                trace.push((sway.offset(), moving));
            }

            (sway, trace)
        };

        let (first, first_trace) = run();
        let (second, second_trace) = run();
        assert_eq!(first_trace, second_trace);
        assert_eq!(first, second);

        assert!(first_trace[10].1, "the cable lags while its ends move");
        assert!(first.is_resting(), "the cable settles once its ends stop");
        assert!(!first_trace.last().unwrap().1);
    }

    #[test]
    fn sway_stays_still_without_motion_or_when_switched_off() {
        let mut sway = CableSway::default();
        assert!(!sway.advance(vec2(0.0, 0.0), 0.6, 48.0, 1.0 / 60.0));
        assert!(sway.is_resting());

        // Reduced motion: no amount, so no displacement however far the ends move.
        assert!(!sway.advance(vec2(500.0, 500.0), 0.0, 48.0, 1.0 / 60.0));
        assert!(sway.is_resting());

        // A stalled frame takes a bounded number of steps.
        assert!(sway.advance(vec2(30.0, 0.0), 1.0, 48.0, 10.0));
        assert!(sway.offset().length() <= 48.0);
        assert!(!sway.advance(vec2(f32::NAN, 0.0), 1.0, f32::INFINITY, 1.0));
        assert!(sway.is_resting());
    }
}
