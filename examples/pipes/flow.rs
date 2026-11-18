//! Five ways a pipe can move, each as a small model on one grid along the pipe.
//!
//! Every model answers the same three questions at each point of the pipe: how large the
//! cross-section is, how fast the contents move there, and which contents are there now.
//! How the pipe is drawn is no concern of this file.
//!
//! The models are deliberately small and they are not all the same kind of thing.
//! [`Mechanism::coupling`] says, for each, what acts on what.
//!
//! Lengths are in graph units, times in seconds. The command is a number in `-1.0..=1.0`:
//! how hard the source pushes, and with a negative sign, pulls.

use egui::{Pos2, Vec2};

/// Segments the pipe is divided into. The models keep a value at each of the points between
/// them, both ends included.
pub const CELLS: usize = 128;

/// Length of one step of every model. A call to [`Pipe::advance`] takes whole steps, so the
/// same commands always give the same pipe.
pub const STEP: f32 = 1.0 / 240.0;

/// Most steps one call to [`Pipe::advance`] takes, however long the frame was.
const MAX_STEPS: u32 = 48;

/// Speed of the contents under a full command where nothing else decides it.
pub const SPEED: f32 = 140.0;

/// Peristalsis: speed of the ring of contraction under a full command.
pub const SQUEEZE_SPEED: f32 = 110.0;

/// Peristalsis: distance from one ring of contraction to the next.
pub const SQUEEZE_SPACING: f32 = 150.0;

/// Peristalsis: share of that distance one contraction takes up.
pub const SQUEEZE_SHARE: f32 = 0.45;

/// Peristalsis: cross-section left open in the middle of a contraction, as a share of the
/// cross-section at rest.
pub const SQUEEZE_THROAT: f32 = 0.28;

/// Pressure wave: speed at which a change of pressure runs along the wall.
pub const WAVE_SPEED: f32 = 330.0;

/// Pressure wave: share of the flow lost to friction each second.
pub const WAVE_FRICTION: f32 = 0.6;

/// Pressure wave: beats of the source each second.
pub const BEAT_RATE: f32 = 0.9;

/// Pressure wave: share of a beat during which the source pushes.
pub const BEAT_SHARE: f32 = 0.28;

/// Pressure wave: flow between beats and at the height of one, as multiples of [`SPEED`].
pub const BEAT_FLOW: (f32, f32) = (0.25, 1.75);

/// Ballooning: where along the pipe the soft stretch is, as a share of its length.
pub const BALLOON_AT: f32 = 0.55;

/// Ballooning: how far the soft stretch reaches to each side of its middle, as the standard
/// deviation of a bell curve.
pub const BALLOON_REACH: f32 = 22.0;

/// Ballooning: the command above which the soft stretch gives way.
pub const BALLOON_THRESHOLD: f32 = 0.6;

/// Ballooning: how much the cross-section in the middle of the soft stretch grows for each
/// unit of [`Pipe::balloon`], as a multiple of the cross-section at rest.
pub const BALLOON_GROWTH: f32 = 1.2;

/// Ballooning: fill at which the wall resists most before giving way, and fill at which it
/// resists least once it has.
pub const BALLOON_LIMITS: (f32, f32) = (1.0, 2.2);

/// Bourdon flex: how much of its curl the pipe loses under a full command.
pub const FLEX: f32 = 0.75;

/// Bourdon flex: how much curl the pipe gains when it is pulled on as hard instead.
pub const FLEX_IN: f32 = 0.22;

/// Bourdon flex: how quickly the pipe follows the command, in radians per second, and how
/// strongly its swing is damped.
pub const FLEX_SPRING: (f32, f32) = (6.0, 0.45);

/// Bourdon flex: how much the cross-section rounds out under a full command.
pub const FLEX_SWELL: f32 = 0.25;

/// The smallest cross-section any model leaves open, as a share of the one at rest.
const MIN_AREA: f32 = 0.2;

/// Labels are kept within a few multiples of this. Whatever is chosen from a label must
/// repeat over this distance.
pub const LABEL_REPEAT: f32 = 16_384.0;

/// What moves a pipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mechanism {
    /// The wall holds still and the contents are carried along it.
    Advection,

    /// The wall contracts in rings that travel, and pushes the contents ahead of them.
    Peristalsis,

    /// The source pushes in beats; each beat widens the wall where it is and runs ahead of
    /// the contents.
    PressureWave,

    /// One soft stretch of the wall holds until the pressure passes a threshold, then
    /// balloons, and stays ballooned until the pressure has fallen well below it.
    Balloon,

    /// The pipe is curled, and straightens between its fixed ends as the pressure rises.
    Bourdon,
}

impl Mechanism {
    pub const ALL: [Self; 5] = [
        Self::Advection,
        Self::Peristalsis,
        Self::PressureWave,
        Self::Balloon,
        Self::Bourdon,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Advection => "Carried contents",
            Self::Peristalsis => "Peristalsis",
            Self::PressureWave => "Pressure wave",
            Self::Balloon => "Ballooning",
            Self::Bourdon => "Bourdon flex",
        }
    }

    /// The kind of model, in a word or two.
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Advection | Self::Peristalsis | Self::Bourdon => "kinematic",
            Self::PressureWave => "coupled, 1-D",
            Self::Balloon => "lumped",
        }
    }

    /// What acts on what in the model, in as few words as stay true.
    pub const fn coupling(self) -> &'static str {
        match self {
            Self::Advection => "kinematic: fixed wall, contents carried",
            Self::Peristalsis => "kinematic: wall prescribed, moves contents",
            Self::PressureWave => "coupled in 1-D: flow and wall act on each other",
            Self::Balloon => "lumped: one pressure, one volume, threshold",
            Self::Bourdon => "kinematic: pressure bends centreline",
        }
    }
}

/// One pipe and what is in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipe {
    mechanism: Mechanism,

    /// Which contents are at each point: the distance, at the cross-section at rest, that
    /// has gone by since they passed the start of the pipe. It falls along the pipe.
    labels: Vec<f32>,

    /// Cross-section at each point, as a multiple of the one at rest.
    areas: Vec<f32>,

    /// Speed of the contents at each point.
    speeds: Vec<f32>,

    /// Peristalsis: how far the rings of contraction have travelled.
    travelled: f32,

    /// Pressure wave: how much wider than at rest each segment is, and the flow at each
    /// point, as the speed it would have at the cross-section at rest.
    swell: Vec<f32>,
    flow: Vec<f32>,

    /// Pressure wave: time on the clock of the source.
    clock: f32,

    /// Ballooning: how full the soft stretch is. Zero is rest, [`BALLOON_LIMITS`] are where
    /// it gives way and where it gives back.
    balloon: f32,

    /// Bourdon flex: how far the pipe has straightened, and how fast that is changing.
    /// One is the full command held still; negative is curled tighter than at rest.
    straightening: f32,
    straightening_rate: f32,
}

impl Pipe {
    /// A pipe at rest, filled with contents that have not moved yet.
    pub fn new(mechanism: Mechanism, length: f32) -> Self {
        let length = sane_length(length);

        #[allow(clippy::cast_precision_loss)]
        let labels = (0..=CELLS)
            .map(|point| -length * point as f32 / CELLS as f32)
            .collect();

        let mut pipe = Self {
            mechanism,
            labels,
            areas: vec![1.0; CELLS + 1],
            speeds: vec![0.0; CELLS + 1],
            travelled: 0.0,
            swell: vec![0.0; CELLS],
            flow: vec![0.0; CELLS + 1],
            clock: 0.0,
            balloon: 0.0,
            straightening: 0.0,
            straightening_rate: 0.0,
        };
        pipe.shape(0.0, length);
        pipe
    }

    pub const fn mechanism(&self) -> Mechanism {
        self.mechanism
    }

    /// Moves the pipe on by `seconds` under `command`, for a pipe `length` long.
    ///
    /// Whole steps of [`STEP`] are taken, at least one and at most a fifth of a second's
    /// worth, so a stalled frame neither stops the pipe nor makes it leap.
    pub fn advance(&mut self, command: f32, length: f32, seconds: f32) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = ((seconds / STEP).round() as u32).clamp(1, MAX_STEPS);
        for _ in 0..steps {
            self.step(command, length);
        }
    }

    /// Moves the pipe on by one [`STEP`].
    pub fn step(&mut self, command: f32, length: f32) {
        let command = if command.is_finite() {
            command.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let length = sane_length(length);

        match self.mechanism {
            Mechanism::Advection | Mechanism::Bourdon => {}
            Mechanism::Peristalsis => self.travelled += command * SQUEEZE_SPEED * STEP,
            Mechanism::PressureWave => self.step_wave(command, length),
            Mechanism::Balloon => self.balloon += self.balloon_rate(command) * STEP,
        }

        if self.mechanism == Mechanism::Bourdon {
            let (spring, damping) = FLEX_SPRING;
            let pull = spring * spring * (command - self.straightening)
                - 2.0 * damping * spring * self.straightening_rate;
            self.straightening_rate += pull * STEP;
            self.straightening += self.straightening_rate * STEP;
        }

        self.shape(command, length);
        self.carry(length);
    }

    /// Works out the cross-section and the speed of the contents at every point from the
    /// state of the model.
    fn shape(&mut self, command: f32, length: f32) {
        match self.mechanism {
            Mechanism::Advection => {
                self.areas.fill(1.0);
                self.speeds.fill(command * SPEED);
            }

            Mechanism::Bourdon => {
                // A flattened tube rounds out as it straightens.
                let area = FLEX_SWELL.mul_add(self.straightening, 1.0).max(MIN_AREA);
                self.areas.fill(area);
                self.speeds.fill(command * SPEED);
            }

            Mechanism::Peristalsis => {
                let wave_speed = command * SQUEEZE_SPEED;
                for point in 0..=CELLS {
                    let area = squeeze_area(along(point, length) - self.travelled);
                    self.areas[point] = area;

                    // What is stored under the wall can only change by what flows past:
                    // the flow is the wall's speed times the cross-section, less what
                    // leaks back through the throat. The contents in the throat hold
                    // still; the contents between two throats are pushed along.
                    self.speeds[point] = wave_speed * (1.0 - SQUEEZE_THROAT / area);
                }
            }

            Mechanism::PressureWave => {
                for point in 0..=CELLS {
                    let before = self.swell[point.saturating_sub(1)];
                    let after = self.swell[point.min(CELLS - 1)];
                    let area = (1.0 + 0.5 * (before + after)).max(MIN_AREA);
                    self.areas[point] = area;
                    self.speeds[point] = self.flow[point] / area;
                }
            }

            Mechanism::Balloon => {
                let inflow = command * SPEED;
                let filling = self.balloon_rate(command) * balloon_volume();

                for point in 0..=CELLS {
                    let from_middle = along(point, length) - BALLOON_AT * length;
                    let area = (BALLOON_GROWTH * self.balloon)
                        .mul_add(bell(from_middle), 1.0)
                        .max(MIN_AREA);
                    self.areas[point] = area;

                    // Past the soft stretch the pipe carries what came in less what the
                    // stretch is taking up.
                    let flow = filling.mul_add(-bell_before(from_middle), inflow);
                    self.speeds[point] = flow / area;
                }
            }
        }
    }

    /// Carries the contents along at the speed found for each point.
    fn carry(&mut self, length: f32) {
        #[allow(clippy::cast_precision_loss)]
        let spacing = length / CELLS as f32;

        // Beyond either end the contents go on as they do just inside it.
        let before_start = 2.0f32.mul_add(self.labels[0], -self.labels[1]);
        let after_end = 2.0f32.mul_add(self.labels[CELLS], -self.labels[CELLS - 1]);

        let mut upstream = before_start;
        for point in 0..=CELLS {
            let here = self.labels[point];
            let downstream = if point < CELLS {
                self.labels[point + 1]
            } else {
                after_end
            };

            // No more than one segment a step, whatever the model asked for.
            let courant = (self.speeds[point] * STEP / spacing).clamp(-1.0, 1.0);
            self.labels[point] = if courant >= 0.0 {
                courant.mul_add(upstream - here, here)
            } else {
                courant.mul_add(here - downstream, here)
            };

            upstream = here;
        }

        // Only differences between labels matter, and what is chosen from them repeats.
        // Keep the numbers small enough to stay exact.
        if self.labels[0].abs() > 2.0 * LABEL_REPEAT {
            let shift = LABEL_REPEAT * (self.labels[0] / LABEL_REPEAT).trunc();
            for label in &mut self.labels {
                *label -= shift;
            }
        }
    }

    /// One step of the pressure wave.
    ///
    /// Two things are kept: how much wider than at rest each segment is, and the flow
    /// through each point between segments. A segment widens by what flows in less what
    /// flows out. The flow through a point grows where the segment behind it is wider, and
    /// so under more pressure, than the one ahead. Each acts on the other; neither is
    /// prescribed. Together they carry a change along the wall at [`WAVE_SPEED`].
    fn step_wave(&mut self, command: f32, length: f32) {
        #[allow(clippy::cast_precision_loss)]
        let spacing = length / CELLS as f32;

        // A short pipe needs shorter steps for the wave not to outrun the grid.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let parts = ((WAVE_SPEED * STEP / (0.9 * spacing)).ceil() as u32).clamp(1, 64);
        #[allow(clippy::cast_precision_loss)]
        let dt = STEP / parts as f32;

        for _ in 0..parts {
            self.clock += dt;

            // The source decides the flow at the start. At the end the pipe runs on into
            // more pipe of the same kind, so a wave leaves without an echo.
            self.flow[0] = command * SPEED * beat(self.clock);
            self.flow[CELLS] = WAVE_SPEED * self.swell[CELLS - 1];

            for segment in 0..CELLS {
                let gained = self.flow[segment] - self.flow[segment + 1];
                self.swell[segment] = (dt / spacing)
                    .mul_add(gained, self.swell[segment])
                    .max(MIN_AREA - 1.0);
            }

            for point in 1..CELLS {
                let push =
                    WAVE_SPEED * WAVE_SPEED * (self.swell[point - 1] - self.swell[point]) / spacing;
                self.flow[point] += dt * WAVE_FRICTION.mul_add(-self.flow[point], push);
            }
        }
    }

    /// How fast the soft stretch fills under `command`.
    ///
    /// It takes in what the source delivers and lets out what its own pressure drives on.
    /// Its pressure rises with its fill up to a limit, falls as the wall gives way, and
    /// rises again once the wall is taut: so a command just over the threshold is enough
    /// to balloon it, and the command must fall well under the threshold to let it down.
    pub fn balloon_rate(&self, command: f32) -> f32 {
        SPEED * BALLOON_THRESHOLD.mul_add(-balloon_pressure(self.balloon), command)
            / balloon_volume()
    }

    /// The cross-section `share` of the way along the pipe, as a multiple of the one at rest.
    pub fn area_at(&self, share: f32) -> f32 {
        sample(&self.areas, share)
    }

    /// The speed of the contents `share` of the way along the pipe.
    pub fn speed_at(&self, share: f32) -> f32 {
        sample(&self.speeds, share)
    }

    /// Which contents are `share` of the way along the pipe. Contents keep their label as
    /// they move, so a colour chosen from it travels with them.
    pub fn label_at(&self, share: f32) -> f32 {
        sample(&self.labels, share)
    }

    /// How full the soft stretch of a ballooning pipe is.
    pub const fn balloon(&self) -> f32 {
        self.balloon
    }

    /// How far a Bourdon pipe has straightened.
    pub const fn straightening(&self) -> f32 {
        self.straightening
    }

    /// The contents the pipe holds, as the length they would fill at the cross-section at
    /// rest. For a pipe of length one this is a multiple of what it holds at rest.
    pub fn volume(&self, length: f32) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        let spacing = sane_length(length) / CELLS as f32;
        let ends = 0.5 * (self.areas[0] + self.areas[CELLS]);
        (self.areas[1..CELLS].iter().sum::<f32>() + ends) * spacing
    }
}

/// The pressure in the soft stretch of a ballooning pipe at a given fill, as a multiple of
/// the pressure at which it gives way.
pub fn balloon_pressure(fill: f32) -> f32 {
    let (peak, valley) = BALLOON_LIMITS;

    // A cubic through zero that turns at the two limits.
    let curve = |fill: f32| {
        (fill * fill * fill / 3.0) - 0.5 * (peak + valley) * fill * fill + peak * valley * fill
    };
    curve(fill) / curve(peak)
}

/// What the soft stretch holds for each unit of fill, as a length of pipe at rest.
fn balloon_volume() -> f32 {
    BALLOON_GROWTH * BALLOON_REACH * (2.0 * std::f32::consts::PI).sqrt()
}

/// How open a peristaltic pipe is at a distance along it, measured from a ring of
/// contraction that has not travelled.
fn squeeze_area(distance: f32) -> f32 {
    let phase = (distance / SQUEEZE_SPACING).rem_euclid(1.0);
    let from_throat = (phase - 0.5).abs() / (0.5 * SQUEEZE_SHARE);

    if from_throat >= 1.0 {
        1.0
    } else {
        let closed = 0.5 * (1.0 + (std::f32::consts::PI * from_throat).cos());
        (1.0 - SQUEEZE_THROAT).mul_add(-closed, 1.0)
    }
}

/// What the source of a pressure wave delivers at a time, as a multiple of [`SPEED`].
fn beat(clock: f32) -> f32 {
    let (resting, pushing) = BEAT_FLOW;
    let phase = (clock * BEAT_RATE).rem_euclid(1.0) / BEAT_SHARE;

    if phase >= 1.0 {
        resting
    } else {
        let push = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * phase).cos());
        (pushing - resting).mul_add(push, resting)
    }
}

fn bell(distance: f32) -> f32 {
    let reach = distance / BALLOON_REACH;
    (-0.5 * reach * reach).exp()
}

/// The share of a bell curve that lies before `distance` from its middle.
fn bell_before(distance: f32) -> f32 {
    // A logistic curve of matching spread. It is within a hundredth of the true share.
    1.0 / (1.0 + (-1.702 * distance / BALLOON_REACH).exp())
}

#[allow(clippy::cast_precision_loss)]
fn along(point: usize, length: f32) -> f32 {
    length * point as f32 / CELLS as f32
}

fn sane_length(length: f32) -> f32 {
    if length.is_finite() && length > 1.0 {
        length
    } else {
        1.0
    }
}

/// The value `share` of the way along values kept at evenly spaced points.
fn sample(values: &[f32], share: f32) -> f32 {
    let share = if share.is_finite() {
        share.clamp(0.0, 1.0)
    } else {
        0.0
    };

    #[allow(clippy::cast_precision_loss)]
    let at = share * (values.len() - 1) as f32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let before = (at.floor() as usize).min(values.len() - 2);

    #[allow(clippy::cast_precision_loss)]
    let t = at - before as f32;
    (values[before + 1] - values[before]).mul_add(t, values[before])
}

/// A path with part of its curl taken out, or put in, between ends that stay where they are.
///
/// Each stretch of the path keeps its length and turns toward the straight line from the
/// first point to the last by `straightening` times [`FLEX`]. A negative `straightening`
/// turns it away, by that times [`FLEX_IN`]: a tube under suction curls, but less than it
/// straightens under pressure.
/// Because straightening a curl moves its far end, the whole is then turned and scaled
/// about the first point until the last is back in place, so every stretch ends up longer
/// or shorter by the same share.
///
/// This is geometry only: nothing here works out the forces that straighten a pressurised
/// tube. A real tube held at both ends could not lose curl without stretching; this one is
/// simply drawn shorter as it straightens and longer as it curls.
pub fn flex(points: &[Pos2], straightening: f32) -> Vec<Pos2> {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return Vec::new();
    };

    let chord = last - first;
    if points.len() < 3 || chord.length() < 1e-3 || !straightening.is_finite() {
        return points.to_vec();
    }

    let chord_angle = chord.angle();
    let mut flexed = Vec::with_capacity(points.len());
    flexed.push(first);

    let kept = if straightening >= 0.0 {
        FLEX.mul_add(-straightening, 1.0)
    } else {
        FLEX_IN.mul_add(-straightening, 1.0)
    };
    let mut last_turn = 0.0f32;
    let mut at = first;

    for pair in points.windows(2) {
        let run = pair[1] - pair[0];

        // The direction of this stretch against the chord, followed around without a
        // jump where it passes half a turn.
        let mut turn = run.angle() - chord_angle;
        turn -= std::f32::consts::TAU * ((turn - last_turn) / std::f32::consts::TAU).round();
        last_turn = turn;

        at += run.length() * Vec2::angled(turn.mul_add(kept, chord_angle));
        flexed.push(at);
    }

    let reached = at - first;
    if reached.length() < 1e-3 {
        return points.to_vec();
    }

    // Turn and scale about the first point so that the last comes back to where it was.
    let scale = chord.length() / reached.length();
    let rotation = chord_angle - reached.angle();
    let (sin, cos) = rotation.sin_cos();

    for point in &mut flexed {
        let offset = *point - first;
        *point = first
            + scale
                * Vec2::new(
                    cos.mul_add(offset.x, -sin * offset.y),
                    sin.mul_add(offset.x, cos * offset.y),
                );
    }

    // Exactly, not merely closely: this is the pin.
    if let Some(end) = flexed.last_mut() {
        *end = last;
    }

    flexed
}
