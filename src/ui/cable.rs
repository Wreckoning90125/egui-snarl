//! One cable geometry shared by painting, hit testing, bounds and anchors.

use egui::{Color32, Mesh, Pos2, Rect, Shape, Stroke, Vec2, emath::TSTransform, vec2};
use smallvec::SmallVec;

/// Upper bound on the segments a sag adds to one cable.
const MAX_SAG_SEGMENTS: usize = 64;

/// Upper bound on the segments one Bezier is flattened into.
const MAX_BEZIER_SEGMENTS: usize = 100;

/// Tolerance used when the caller supplies one that is not a positive finite number.
const FALLBACK_TOLERANCE: f32 = 0.25;

/// Upper bound on the points [`CableGeometry::subdivided`] gives one cable.
const MAX_SUBDIVIDED_POINTS: usize = 1024;

/// How much farther than its half width a wall may reach at a sharp bend.
const MAX_MITRE: f32 = 2.0;

/// The shortest run a cut leaves between itself and the next point of the cable.
const SHORTEST_RUN: f32 = 0.05;

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
///
/// A cable can also carry a half width at each point, given with
/// [`CableGeometry::with_radii`], for a wire that is a tube rather than a line. The walls,
/// the filled ribbon, the bounds and the hit test then all follow that one profile.
#[derive(Clone, Debug, PartialEq)]
pub struct CableGeometry {
    points: Vec<Pos2>,
    lengths: Vec<f32>,
    radii: Option<Vec<f32>>,
    bounds: Rect,
}

impl CableGeometry {
    /// Builds a cable through `points`.
    ///
    /// Non-finite points and repeated consecutive points are dropped.
    /// The result always holds at least two points, so a degenerate input gives a cable of zero length.
    #[must_use]
    pub fn from_points(points: impl IntoIterator<Item = Pos2>) -> Self {
        Self::from_parts(points.into_iter().map(|point| (point, 0.0)), false)
    }

    /// Builds a cable through points that each come with a half width, kept when `tube` is set.
    fn from_parts(parts: impl IntoIterator<Item = (Pos2, f32)>, tube: bool) -> Self {
        let mut parts: Vec<(Pos2, f32)> = parts
            .into_iter()
            .filter(|(point, _)| point.x.is_finite() && point.y.is_finite())
            .collect();
        parts.dedup_by(|next, kept| next.0 == kept.0);

        match parts.len() {
            0 => parts = vec![(Pos2::ZERO, 0.0); 2],
            1 => parts.push(parts[0]),
            _ => {}
        }

        let points: Vec<Pos2> = parts.iter().map(|&(point, _)| point).collect();

        let mut lengths = Vec::with_capacity(points.len());
        let mut total = 0.0;
        lengths.push(total);
        for pair in points.windows(2) {
            total += pair[0].distance(pair[1]);
            lengths.push(total);
        }

        let radii: Option<Vec<f32>> = tube.then(|| {
            parts
                .iter()
                .map(|&(_, radius)| sane_radius(radius))
                .collect()
        });

        let bounds = match &radii {
            None => Rect::from_points(&points),
            Some(radii) => {
                let mut bounds = Rect::NOTHING;
                for (&point, &radius) in points.iter().zip(radii) {
                    bounds |= Rect::from_center_size(point, Vec2::splat(2.0 * radius));
                }
                bounds
            }
        };

        Self {
            points,
            lengths,
            radii,
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
        points.push((self.points[0], self.radius_of(0)));

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
                points.push((
                    from.lerp(to, t) + sag * (4.0 * share * (1.0 - share)),
                    egui::lerp(self.radius_of(index - 1)..=self.radius_of(index), t),
                ));
            }
        }

        Self::from_parts(points, self.radii.is_some())
    }

    /// Returns this cable mapped through `transform`. Half widths scale with it.
    #[must_use]
    pub fn transformed(&self, transform: TSTransform) -> Self {
        Self::from_parts(
            self.points.iter().enumerate().map(|(index, &point)| {
                (
                    transform * point,
                    self.radius_of(index) * transform.scaling.abs(),
                )
            }),
            self.radii.is_some(),
        )
    }

    /// Returns this cable as a tube whose half width at each point is `radius` of the share
    /// of the arc length the point lies at.
    ///
    /// The half width runs straight from one point of the cable to the next, so a profile
    /// finer than the cable's points needs [`CableGeometry::subdivided`] first. Half widths
    /// that are negative or not finite count as zero.
    #[must_use]
    pub fn with_radii(self, mut radius: impl FnMut(f32) -> f32) -> Self {
        let total = self.length();
        let parts: Vec<(Pos2, f32)> = self
            .points
            .iter()
            .zip(&self.lengths)
            .map(|(&point, &length)| {
                let share = if total > 0.0 { length / total } else { 0.0 };
                (point, radius(share))
            })
            .collect();

        Self::from_parts(parts, true)
    }

    /// Returns this cable with points added until none is farther than `spacing` from the
    /// next. Every point it had is kept, so the path is the same.
    ///
    /// A straight wire is two points; this gives it enough for a half width or a displacement
    /// that varies along it.
    #[must_use]
    pub fn subdivided(&self, spacing: f32) -> Self {
        let total = self.length();
        if !spacing.is_finite() || spacing <= 0.0 || total <= 0.0 {
            return self.clone();
        }

        #[allow(clippy::cast_precision_loss)]
        let spacing = spacing.max(total / MAX_SUBDIVIDED_POINTS as f32);

        let mut parts = Vec::with_capacity(self.points.len());
        parts.push((self.points[0], self.radius_of(0)));

        for index in 1..self.points.len() {
            let run = self.lengths[index] - self.lengths[index - 1];

            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pieces = ((run / spacing).ceil() as usize).clamp(1, MAX_SUBDIVIDED_POINTS);

            for piece in 1..=pieces {
                #[allow(clippy::cast_precision_loss)]
                let t = piece as f32 / pieces as f32;
                parts.push((
                    self.points[index - 1].lerp(self.points[index], t),
                    egui::lerp(self.radius_of(index - 1)..=self.radius_of(index), t),
                ));
            }
        }

        Self::from_parts(parts, self.radii.is_some())
    }

    /// The part of this cable between two shares of its arc length, with its half widths.
    ///
    /// Shares are clamped to `0.0..=1.0` and taken in ascending order.
    ///
    /// A point of the cable that lies within a twentieth of a unit of either cut is left
    /// out, and the cut stands in for it. A run that short has no direction worth the
    /// name, and the walls of the part would turn with it.
    #[must_use]
    pub fn section(&self, from: f32, to: f32) -> Self {
        let clamp = |share: f32| {
            if share.is_finite() {
                share.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let (from, to) = (clamp(from).min(clamp(to)), clamp(from).max(clamp(to)));
        let total = self.length();

        let (first, last) = (from * total + SHORTEST_RUN, to * total - SHORTEST_RUN);

        let mut parts = Vec::new();
        parts.push((self.anchor_at(from).pos, self.radius_at(from)));
        for (index, &point) in self.points.iter().enumerate() {
            let length = self.lengths[index];
            if length > first && length < last {
                parts.push((point, self.radius_of(index)));
            }
        }
        parts.push((self.anchor_at(to).pos, self.radius_at(to)));

        Self::from_parts(parts, self.radii.is_some())
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

    /// Smallest rectangle holding every point of the cable, each with its half width around
    /// it when the cable has them.
    #[must_use]
    #[inline]
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The half width at each of [`CableGeometry::points`], if the cable was given them.
    #[must_use]
    #[inline]
    pub fn radii(&self) -> Option<&[f32]> {
        self.radii.as_deref()
    }

    /// The half width `share` of the way along the cable's arc length. Zero for a cable that
    /// was given none.
    #[must_use]
    pub fn radius_at(&self, share: f32) -> f32 {
        if self.radii.is_none() {
            return 0.0;
        }

        let (index, t) = self.locate(share);
        egui::lerp(self.radius_of(index - 1)..=self.radius_of(index), t)
    }

    fn radius_of(&self, index: usize) -> f32 {
        self.radii.as_ref().map_or(0.0, |radii| radii[index])
    }

    /// The segment that holds the point `share` of the way along the cable, as the index of
    /// the point it ends at, and how far along that segment the point is.
    fn locate(&self, share: f32) -> (usize, f32) {
        let share = if share.is_finite() {
            share.clamp(0.0, 1.0)
        } else {
            0.5
        };

        let target = self.length() * share;
        let index = self
            .lengths
            .partition_point(|&length| length < target)
            .clamp(1, self.points.len() - 1);

        let run = self.lengths[index] - self.lengths[index - 1];
        let t = if run > 0.0 {
            ((target - self.lengths[index - 1]) / run).clamp(0.0, 1.0)
        } else {
            0.0
        };

        (index, t)
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

        let (index, t) = self.locate(share);
        let from = self.points[index - 1];
        let to = self.points[index];

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
                best_length = (self.lengths[index] - self.lengths[index - 1])
                    .mul_add(t, self.lengths[index - 1]);
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
    ///
    /// A cable with half widths is also hit anywhere between its walls, however wide it is
    /// there, and the point reported is the one on the stretch `pos` is deepest inside.
    #[must_use]
    pub fn hit(&self, pos: Pos2, tolerance: f32) -> Option<CableHit> {
        if !tolerance.is_finite() || tolerance < 0.0 {
            return None;
        }

        if !self.bounds.expand(tolerance).contains(pos) {
            return None;
        }

        let Some(radii) = &self.radii else {
            let hit = self.closest(pos);
            return (hit.distance <= tolerance).then_some(hit);
        };

        let mut best: Option<(f32, CableHit)> = None;
        let total = self.length();

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
            let distance = nearest.distance(pos);
            let reach = egui::lerp(radii[index - 1]..=radii[index], t).max(tolerance);
            let outside = distance - reach;

            if outside <= 0.0 && best.is_none_or(|(least, _)| outside < least) {
                let length = (self.lengths[index] - self.lengths[index - 1])
                    .mul_add(t, self.lengths[index - 1]);

                best = Some((
                    outside,
                    CableHit {
                        pos: nearest,
                        distance,
                        share: if total > 0.0 {
                            (length / total).clamp(0.0, 1.0)
                        } else {
                            0.0
                        },
                    },
                ));
            }
        }

        best.map(|(_, hit)| hit)
    }

    /// The cable moved sideways: each point displaced along the normal there by `distance`
    /// of the share of the arc length it lies at. Positive is the side
    /// [`CableAnchor::normal`] points to.
    ///
    /// At a bend the displaced point sits on the mitre, so a constant distance gives a
    /// parallel line.
    #[must_use]
    pub fn offset(&self, mut distance: impl FnMut(f32) -> f32) -> Vec<Pos2> {
        let total = self.length();
        let sides = self.sides();

        self.points
            .iter()
            .zip(&self.lengths)
            .zip(sides)
            .map(|((&point, &length), side)| {
                let share = if total > 0.0 { length / total } else { 0.0 };
                let distance = distance(share);
                let distance = if distance.is_finite() { distance } else { 0.0 };

                point + side * distance
            })
            .collect()
    }

    /// For each point of the cable, the step that moves it one unit sideways: along the
    /// normal there, longer at a bend so that the moved point sits on the mitre.
    fn sides(&self) -> Vec<Vec2> {
        let last = self.points.len() - 1;

        (0..=last)
            .map(|index| {
                let before =
                    (index > 0).then(|| unit_or_x(self.points[index] - self.points[index - 1]));
                let after =
                    (index < last).then(|| unit_or_x(self.points[index + 1] - self.points[index]));

                let (tangent, mitre) = match (before, after) {
                    (Some(before), Some(after)) => {
                        let sum = before + after;
                        let length = sum.length();
                        if length > 1e-4 {
                            let tangent = sum / length;
                            // One over the cosine of half the turn.
                            (tangent, (1.0 / tangent.dot(after).max(1e-4)).min(MAX_MITRE))
                        } else {
                            (after, 1.0)
                        }
                    }
                    (Some(tangent), None) | (None, Some(tangent)) => (tangent, 1.0),
                    (None, None) => (Vec2::X, 1.0),
                };

                vec2(-tangent.y, tangent.x) * mitre
            })
            .collect()
    }

    /// The two walls of a cable with half widths: the side [`CableAnchor::normal`] points
    /// away from first, then the side it points to. Both run from the cable's start to its
    /// end and hold one point for each of [`CableGeometry::points`].
    ///
    /// A cable without half widths gives its own path twice.
    #[must_use]
    pub fn walls(&self) -> [Vec<Pos2>; 2] {
        [
            self.offset(|share| -self.radius_at(share)),
            self.offset(|share| self.radius_at(share)),
        ]
    }

    /// The area between the walls of a cable with half widths, as one shape. `fill` gives
    /// the colour at each share of the arc length and is blended from point to point, so
    /// what flows in a tube can be painted as one connected band.
    ///
    /// The edges are not softened; stroke the [`CableGeometry::walls`] over them. A cable
    /// without half widths gives a shape that paints nothing.
    #[must_use]
    pub fn ribbon(&self, mut fill: impl FnMut(f32) -> Color32) -> Shape {
        if self
            .radii
            .as_ref()
            .is_none_or(|radii| radii.iter().all(|&radius| radius <= 0.0))
        {
            return Shape::Noop;
        }

        self.band(|share| {
            let (radius, color) = (self.radius_at(share), fill(share));
            [(-radius, color), (radius, color)]
        })
    }

    /// A band along the cable between two lines beside it, as one shape.
    ///
    /// `edges` gives, at each share of the arc length, the two edges of the band there:
    /// how far each lies from the cable, as [`CableGeometry::offset`] counts it, and its
    /// colour. Colours are blended across the band as well as along it. Several bands
    /// side by side shade a tube across its width; one that is narrower than the tube is
    /// a highlight or a stripe on it.
    ///
    /// The cable needs no half widths for this. The edges are not softened.
    #[must_use]
    pub fn band(&self, mut edges: impl FnMut(f32) -> [(f32, Color32); 2]) -> Shape {
        let total = self.length();
        let sides = self.sides();

        let mut mesh = Mesh::default();
        mesh.reserve_vertices(2 * self.points.len());
        mesh.reserve_triangles(2 * (self.points.len() - 1));

        for (index, (&point, side)) in self.points.iter().zip(sides).enumerate() {
            let share = if total > 0.0 {
                self.lengths[index] / total
            } else {
                0.0
            };

            for (distance, color) in edges(share) {
                let distance = if distance.is_finite() { distance } else { 0.0 };
                mesh.colored_vertex(point + side * distance, color);
            }

            if index > 0 {
                #[allow(clippy::cast_possible_truncation)]
                let corner = 2 * (index as u32 - 1);
                mesh.add_triangle(corner, corner + 1, corner + 2);
                mesh.add_triangle(corner + 1, corner + 3, corner + 2);
            }
        }

        Shape::mesh(mesh)
    }

    /// The cable as a stroked shape.
    #[must_use]
    pub fn stroke(&self, stroke: impl Into<Stroke>) -> Shape {
        match self.points[..] {
            [from, to] => Shape::line_segment([from, to], stroke.into()),
            _ => Shape::line(self.points.clone(), stroke.into()),
        }
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

fn sane_radius(radius: f32) -> f32 {
    if radius.is_finite() && radius > 0.0 {
        radius
    } else {
        0.0
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
    use egui::{Color32, Shape, emath::TSTransform, pos2, vec2};

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

    /// A straight tube, 200 long, that is 2 wide except for a bulge of 14 around its middle.
    fn bulged() -> CableGeometry {
        CableGeometry::line(pos2(0.0, 0.0), pos2(200.0, 0.0))
            .subdivided(2.0)
            .with_radii(|share| {
                let from_middle = (share - 0.5) / 0.1;
                12.0f32.mul_add((-from_middle * from_middle).exp(), 2.0)
            })
    }

    #[test]
    fn subdividing_adds_points_and_keeps_the_path() {
        let cable = elbow();
        let fine = cable.subdivided(7.0);

        assert!(fine.points().len() > 20);
        assert!((fine.length() - cable.length()).abs() < 1e-3);
        for &point in cable.points() {
            assert!(fine.points().contains(&point), "{point:?} is kept");
        }
        for pair in fine.points().windows(2) {
            assert!(pair[0].distance(pair[1]) <= 7.0 + 1e-3);
        }
        for &point in fine.points() {
            assert!(cable.closest(point).distance < 1e-3);
        }

        // Nothing to do, or nothing sensible asked for.
        assert_eq!(cable.subdivided(500.0), cable);
        assert_eq!(cable.subdivided(0.0), cable);
        assert_eq!(cable.subdivided(f32::NAN), cable);

        // However fine the spacing, the number of points is bounded.
        assert!(cable.subdivided(1e-6).points().len() <= 2 * super::MAX_SUBDIVIDED_POINTS);
    }

    #[test]
    fn a_tube_is_hit_between_its_walls_and_nowhere_else() {
        let tube = bulged();
        assert!(tube.radii().is_some());
        assert!((tube.radius_at(0.5) - 14.0).abs() < 0.05);
        assert!((tube.radius_at(0.0) - 2.0).abs() < 0.05);

        // Inside the bulge, far beyond the pick distance of a thin wire.
        let inside = tube.hit(pos2(100.0, 12.0), 3.0).unwrap();
        assert!(inside.pos.distance(pos2(100.0, 0.0)) < 1.0);
        assert!((inside.distance - 12.0).abs() < 0.1);
        assert!((inside.share - 0.5).abs() < 0.01);

        // Just outside its wall.
        assert!(tube.hit(pos2(100.0, 15.0), 0.5).is_none());

        // The same distance from the path where the tube is thin.
        assert!(tube.hit(pos2(30.0, 12.0), 3.0).is_none());
        assert!(tube.hit(pos2(30.0, 2.5), 3.0).is_some());

        // The thin wire this tube is drawn around would not have been hit in the bulge.
        let wire = CableGeometry::line(pos2(0.0, 0.0), pos2(200.0, 0.0));
        assert!(wire.hit(pos2(100.0, 12.0), 3.0).is_none());
        assert_eq!(wire.radii(), None);
        assert_eq!(wire.radius_at(0.5).to_bits(), 0.0f32.to_bits());
    }

    #[test]
    fn bounds_of_a_tube_hold_its_walls() {
        let tube = bulged();
        let bounds = tube.bounds();

        assert!((bounds.top() + 14.0).abs() < 0.05 && (bounds.bottom() - 14.0).abs() < 0.05);
        assert!((bounds.left() + 2.0).abs() < 0.05 && (bounds.right() - 202.0).abs() < 0.05);

        for wall in tube.walls() {
            for point in wall {
                assert!(bounds.expand(1e-3).contains(point));
            }
        }
    }

    #[test]
    fn walls_keep_their_distance_around_a_bend() {
        let tube = elbow().with_radii(|_| 5.0);
        let [near, far] = tube.walls();

        assert_eq!(near.len(), 3);
        assert_eq!(far.len(), 3);

        // The cable runs right, then down. Its normal points down, then left.
        assert!(far[0].distance(pos2(0.0, 5.0)) < 1e-3);
        assert!(near[0].distance(pos2(0.0, -5.0)) < 1e-3);

        // At the corner each wall sits on the mitre, so both of its runs stay 5 away.
        assert!(far[1].distance(pos2(95.0, 5.0)) < 1e-3);
        assert!(near[1].distance(pos2(105.0, -5.0)) < 1e-3);

        assert!(far[2].distance(pos2(95.0, 50.0)) < 1e-3);
        assert!(near[2].distance(pos2(105.0, 50.0)) < 1e-3);

        // A path with no width has its walls on itself.
        let [near, far] = elbow().walls();
        assert_eq!(near, elbow().points());
        assert_eq!(far, elbow().points());
    }

    #[test]
    fn a_ribbon_fills_the_tube_from_wall_to_wall_in_the_colours_given() {
        let tube = bulged();
        let [near, far] = tube.walls();

        let Shape::Mesh(mesh) = tube.ribbon(|share| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Color32::from_gray((share * 255.0) as u8)
        }) else {
            panic!("a tube has an area to fill");
        };

        assert!(mesh.is_valid());
        assert_eq!(mesh.vertices.len(), 2 * tube.points().len());
        assert_eq!(mesh.indices.len(), 6 * (tube.points().len() - 1));

        for (index, pair) in mesh.vertices.chunks(2).enumerate() {
            assert_eq!(pair[0].pos, near[index]);
            assert_eq!(pair[1].pos, far[index]);
            assert_eq!(pair[0].color, pair[1].color);
        }
        assert_eq!(mesh.vertices[0].color, Color32::from_gray(0));
        assert_eq!(mesh.vertices.last().unwrap().color, Color32::from_gray(255));

        // Every triangle has an area: the band is connected and nowhere folded shut.
        for triangle in mesh.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|corner| mesh.vertices[triangle[corner] as usize].pos);
            let area = (b - a).x.mul_add((c - a).y, -((b - a).y * (c - a).x)).abs() / 2.0;
            assert!(area > 0.0);
        }

        assert_eq!(elbow().ribbon(|_| Color32::WHITE), Shape::Noop);
        assert_eq!(
            elbow().with_radii(|_| 0.0).ribbon(|_| Color32::WHITE),
            Shape::Noop
        );
    }

    #[test]
    fn a_band_lies_between_two_lines_beside_the_cable_with_a_colour_at_each() {
        let (inner, outer) = (Color32::from_gray(200), Color32::from_gray(40));

        // A band over half the width of a tube, lighter toward the middle.
        let tube = bulged();
        let Shape::Mesh(mesh) =
            tube.band(|share| [(0.0, inner), (0.5 * tube.radius_at(share), outer)])
        else {
            panic!("a band is a mesh");
        };

        assert!(mesh.is_valid());
        assert_eq!(mesh.vertices.len(), 2 * tube.points().len());
        assert_eq!(mesh.indices.len(), 6 * (tube.points().len() - 1));

        let half_way = tube.offset(|share| 0.5 * tube.radius_at(share));
        for (index, pair) in mesh.vertices.chunks(2).enumerate() {
            assert_eq!(pair[0].pos, tube.points()[index]);
            assert_eq!(pair[1].pos, half_way[index]);
            assert_eq!((pair[0].color, pair[1].color), (inner, outer));
        }

        // A cable needs no half widths to carry a band, and at a bend both edges sit on
        // the mitre.
        let Shape::Mesh(mesh) = elbow().band(|_| [(-2.0, inner), (3.0, outer)]) else {
            panic!("a band is a mesh");
        };
        let (near, far) = (elbow().offset(|_| -2.0), elbow().offset(|_| 3.0));
        for (index, pair) in mesh.vertices.chunks(2).enumerate() {
            assert_eq!((pair[0].pos, pair[1].pos), (near[index], far[index]));
        }

        // A distance that is not a number is none.
        let Shape::Mesh(mesh) = elbow().band(|_| [(f32::NAN, inner), (f32::INFINITY, outer)])
        else {
            panic!("a band is a mesh");
        };
        for (index, pair) in mesh.vertices.chunks(2).enumerate() {
            assert_eq!(pair[0].pos, elbow().points()[index]);
            assert_eq!(pair[1].pos, elbow().points()[index]);
        }
    }

    #[test]
    fn half_widths_follow_a_section_a_sag_and_a_transform() {
        let tube = bulged();

        let section = tube.section(0.75, 0.4);
        assert!(section.start().distance(pos2(80.0, 0.0)) < 1e-3);
        assert!(section.end().distance(pos2(150.0, 0.0)) < 1e-3);
        assert!((section.length() - 70.0).abs() < 1e-3);
        for share in [0.0, 0.3, 0.7, 1.0] {
            let along = egui::lerp(0.4..=0.75, share);
            assert!(
                (section.radius_at(share) - tube.radius_at(along)).abs() < 0.05,
                "share {share}"
            );
        }

        let sagged = tube.sagged(vec2(0.0, 30.0), 0.25);
        assert!((sagged.radius_at(0.5) - 14.0).abs() < 0.2);
        assert!(sagged.hit(pos2(100.0, 42.0), 0.5).is_some());
        assert!(sagged.hit(pos2(100.0, 12.0), 0.5).is_none());

        let moved = tube.transformed(TSTransform::new(vec2(10.0, 20.0), 2.0));
        assert!((moved.radius_at(0.5) - 28.0).abs() < 0.1);
        assert!(moved.hit(pos2(210.0, 44.0), 0.5).is_some());
        assert!(moved.hit(pos2(210.0, 50.0), 0.5).is_none());

        // A path without half widths gains none on the way.
        assert_eq!(elbow().section(0.2, 0.8).radii(), None);
        assert_eq!(elbow().subdivided(5.0).radii(), None);
        assert_eq!(elbow().sagged(vec2(0.0, 9.0), 0.25).radii(), None);

        // Half widths that make no sense count as none.
        let odd = elbow().with_radii(|share| if share < 0.5 { -3.0 } else { f32::NAN });
        assert_eq!(odd.radii(), Some(&[0.0, 0.0, 0.0][..]));
    }

    #[test]
    fn a_section_cut_beside_a_point_keeps_its_walls_beside_the_path() {
        // A straight tube on a slant, far from the origin, with a point every three units.
        let (from, to) = (pos2(412.0, 431.0), pos2(771.0, 566.0));
        let tube = CableGeometry::line(from, to)
            .subdivided(3.0)
            .with_radii(|_| 10.0);
        let total = tube.length();
        let across = {
            let along = (to - from).normalized();
            vec2(-along.y, along.x)
        };

        // Cut it a hair to either side of one of its points, again and again. The walls
        // of each part are ten from the line the tube runs along, at its ends as well.
        for point_index in [7usize, 31, 64, 90] {
            let at = tube.lengths[point_index] / total;
            for hair in [-3e-4, -1e-4, -2e-5, 2e-5, 1e-4, 3e-4, 5e-3, 0.04] {
                let cut = at + hair / total;
                for section in [tube.section(0.0, cut), tube.section(cut, 1.0)] {
                    let [near, far] = section.walls();
                    for (index, &point) in section.points().iter().enumerate() {
                        for (wall, side) in [(&near, -10.0), (&far, 10.0)] {
                            let strays = wall[index].distance(point + across * side);
                            assert!(
                                strays < 0.01,
                                "cut {hair} from point {point_index}: a wall strays {strays}"
                            );
                        }
                    }

                    // No run of the part is too short to have a direction.
                    for pair in section.points().windows(2) {
                        assert!(
                            pair[0].distance(pair[1]) > 0.04,
                            "cut {hair} from point {point_index}"
                        );
                    }
                }
            }
        }

        // The cuts themselves are where they were asked for, and what lies between them
        // is the cable's own points.
        let section = tube.section(0.25, 0.5);
        assert!(section.start().distance(from + 0.25 * (to - from)) < 1e-3);
        assert!(section.end().distance(from + 0.5 * (to - from)) < 1e-3);
        assert!((section.length() - 0.25 * total).abs() < 1e-3);
        for point in &section.points()[1..section.points().len() - 1] {
            assert!(tube.points().contains(point));
        }

        // A part shorter than that is still its two cuts.
        let sliver = tube.section(0.5, 0.5 + 0.01 / total);
        assert_eq!(sliver.points().len(), 2);
        assert!((sliver.length() - 0.01).abs() < 1e-3);
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
