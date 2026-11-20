//! Five ways a pipe can move, each as a small model on one grid along the pipe.
//!
//! Every model answers the same three questions at each point of the pipe: how large the
//! cross-section is, how fast the contents move there, and which contents are there now.
//! How the pipe is drawn is no concern of this file.
//!
//! The models are deliberately small and they are not all the same kind of thing.
//! [`Mechanism::coupling`] says, for each, what acts on what.
//!
//! Every number a model uses is a field of [`Params`]. What pushes the contents comes
//! from outside, as a [`Forcing`]: see `drive.rs`.
//!
//! Lengths are in graph units, times in seconds.

use egui::{Pos2, Vec2};

use super::drive::{FREE_FALL, Forcing, Lay};

/// Segments the pipe is divided into. The models keep a value at each of the points between
/// them, both ends included.
pub const CELLS: usize = 128;

/// Length of one step of every model. A call to [`Pipe::advance`] takes whole steps, so the
/// same forcing always gives the same pipe.
pub const STEP: f32 = 1.0 / 240.0;

/// Most steps one call to [`Pipe::advance`] takes, however long the frame was.
const MAX_STEPS: u32 = 48;

/// The smallest cross-section any model leaves open, as a share of the one at rest.
const MIN_AREA: f32 = 0.2;

/// Labels are kept within a few multiples of this. Whatever is chosen from a label must
/// repeat over this distance.
pub const LABEL_REPEAT: f32 = 16_384.0;

/// Every number the models use. The defaults are chosen to read well in a graph; none is
/// measured from anything.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Speed of the contents under a full push where nothing else decides it.
    pub speed: f32,

    /// The drop from inlet to outlet that, under free fall, pushes as hard as a full
    /// command.
    pub full_drop: f32,

    /// How far the middle of a level pipe 500 long sags under free fall.
    pub sag: f32,

    pub peristalsis: Peristalsis,
    pub wave: Wave,
    pub balloon: Balloon,
    pub bourdon: Bourdon,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            speed: 140.0,
            full_drop: 60.0,
            sag: 20.0,
            peristalsis: Peristalsis::default(),
            wave: Wave::default(),
            balloon: Balloon::default(),
            bourdon: Bourdon::default(),
        }
    }
}

impl Params {
    /// The head that pushes as hard as a full command. See [`Forcing::drive`].
    pub fn full_head(&self) -> f32 {
        FREE_FALL * self.full_drop.max(1.0)
    }

    /// The pull along a pipe that keeps it taut, for each unit of its weight.
    /// A level pipe `length` long sags `acceleration * length^2 / (8 * tension)`.
    pub fn tension(&self) -> f32 {
        FREE_FALL * 500.0 * 500.0 / (8.0 * self.sag.max(0.01))
    }
}

/// Rings of contraction that travel along the wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Peristalsis {
    /// Speed of the rings under a full push.
    pub speed: f32,

    /// Distance from one ring to the next.
    pub spacing: f32,

    /// Length of pipe one ring narrows, from where it starts to close to where it is
    /// open again. The rest of [`Peristalsis::spacing`] is left fully open.
    pub length: f32,

    /// Cross-section left open in the middle of a ring, as a share of the one at rest.
    pub throat: f32,
}

impl Default for Peristalsis {
    fn default() -> Self {
        Self {
            speed: 110.0,
            spacing: 84.0,
            length: 44.0,
            throat: 0.3,
        }
    }
}

impl Peristalsis {
    fn spacing(&self) -> f32 {
        sane(self.spacing, 84.0).max(8.0)
    }

    fn throat(&self) -> f32 {
        sane(self.throat, 0.3).clamp(MIN_AREA, 1.0)
    }

    /// How open the pipe is at a distance along it, measured from a ring that has not
    /// travelled.
    fn area(&self, distance: f32) -> f32 {
        let spacing = self.spacing();
        let length = sane(self.length, 44.0).clamp(2.0, spacing);

        let from_throat = ((distance / spacing).rem_euclid(1.0) - 0.5).abs() * spacing;
        let from_throat = from_throat / (0.5 * length);

        if from_throat >= 1.0 {
            1.0
        } else {
            let closed = 0.5 * (1.0 + (std::f32::consts::PI * from_throat).cos());
            (1.0 - self.throat()).mul_add(-closed, 1.0)
        }
    }
}

/// Beats of pressure that widen the wall and run along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    /// Speed at which a change of pressure runs along the wall.
    pub speed: f32,

    /// Share of the flow lost to friction each second.
    pub friction: f32,

    /// Beats of the source each second.
    pub rate: f32,

    /// Share of a beat during which the source pushes.
    pub share: f32,

    /// Flow between beats and at the height of one, as multiples of [`Params::speed`].
    pub resting: f32,
    pub pushing: f32,
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            speed: 330.0,
            friction: 0.6,
            rate: 0.9,
            share: 0.28,
            resting: 0.25,
            pushing: 1.75,
        }
    }
}

impl Wave {
    fn speed(&self) -> f32 {
        sane(self.speed, 330.0).clamp(20.0, 2000.0)
    }

    /// What the source delivers at a time, as a multiple of [`Params::speed`].
    fn beat(&self, clock: f32) -> f32 {
        let share = sane(self.share, 0.28).clamp(0.02, 1.0);
        let phase = (clock * sane(self.rate, 0.9)).rem_euclid(1.0) / share;

        if phase >= 1.0 {
            self.resting
        } else {
            let push = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * phase).cos());
            (self.pushing - self.resting).mul_add(push, self.resting)
        }
    }
}

/// One soft stretch of wall that gives way past a threshold.
#[derive(Clone, Debug, PartialEq)]
pub struct Balloon {
    /// Where along the pipe the middle of the soft stretch is, as a share of its length.
    pub at: f32,

    /// How far the soft stretch reaches to each side of its middle.
    pub reach: f32,

    /// How the stretch is shaped along the pipe. At 2 it is a bell, pointed in the
    /// middle. Larger is flatter across the middle with rounder shoulders, toward a
    /// sausage.
    pub roundness: f32,

    /// The push above which the soft stretch gives way.
    pub threshold: f32,

    /// How much the cross-section in the middle of the stretch grows for each unit of
    /// [`Pipe::balloon`], as a multiple of the cross-section at rest.
    pub growth: f32,

    /// The fill at which the wall resists most, before it gives way.
    pub gives_way: f32,

    /// The fill at which the wall resists least, once it has. It must fall to here to
    /// give back.
    pub gives_back: f32,
}

impl Default for Balloon {
    fn default() -> Self {
        Self {
            at: 0.42,
            reach: 34.0,
            roundness: 4.0,
            threshold: 0.6,
            growth: 1.2,
            gives_way: 1.0,
            gives_back: 2.2,
        }
    }
}

impl Balloon {
    /// The two fills the pressure turns at, in order and apart.
    pub fn limits(&self) -> (f32, f32) {
        let gives_way = sane(self.gives_way, 1.0).max(0.2);
        (gives_way, sane(self.gives_back, 2.2).max(gives_way + 0.2))
    }

    /// The pressure in the soft stretch at a given fill, as a multiple of the pressure at
    /// which it gives way.
    ///
    /// It rises with the fill up to a limit, falls as the wall gives way, and rises again
    /// once the wall is taut. That is what makes a threshold, and a different way back.
    pub fn pressure(&self, fill: f32) -> f32 {
        let (peak, valley) = self.limits();

        // A cubic through zero that turns at the two limits.
        let curve = |fill: f32| {
            (fill * fill * fill / 3.0) - 0.5 * (peak + valley) * fill * fill + peak * valley * fill
        };
        curve(fill) / curve(peak)
    }

    /// How soft the wall is at a distance from the middle of the stretch: one in the
    /// middle, falling to nothing.
    fn softness(&self, distance: f32) -> f32 {
        let reach = (distance / sane(self.reach, 34.0).max(1.0)).abs();
        (-0.5 * reach.powf(sane(self.roundness, 4.0).clamp(1.0, 12.0))).exp()
    }
}

/// A curled pipe that straightens under pressure.
#[derive(Clone, Debug, PartialEq)]
pub struct Bourdon {
    /// How far the pipe at rest bows out between its ends, as a share of the distance
    /// between them.
    pub bow: f32,

    /// How much of its curl the pipe loses under a full push.
    pub uncurl: f32,

    /// How much curl it gains when it is pulled on as hard instead.
    pub curl: f32,

    /// How quickly the pipe follows the push, in radians per second.
    pub rate: f32,

    /// How strongly its swing is damped. One does not overshoot.
    pub damping: f32,

    /// How much the cross-section rounds out under a full push.
    pub swell: f32,
}

impl Default for Bourdon {
    fn default() -> Self {
        Self {
            bow: 0.2,
            uncurl: 0.75,
            curl: 0.22,
            rate: 6.0,
            damping: 0.45,
            swell: 0.25,
        }
    }
}

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

    /// How hard the contents were pushed in the last step, in all.
    drive: f32,

    /// Peristalsis: how far the rings of contraction have travelled.
    travelled: f32,

    /// Pressure wave: how much wider than at rest each segment is, and the flow at each
    /// point, as the speed it would have at the cross-section at rest.
    swell: Vec<f32>,
    flow: Vec<f32>,

    /// Pressure wave: time on the clock of the source.
    clock: f32,

    /// Ballooning: how full the soft stretch is. Zero is rest, [`Balloon::limits`] are
    /// where it gives way and where it gives back.
    balloon: f32,

    /// Ballooning: how soft the wall is at each point, how much of that lies before each
    /// point, and the pipe and stretch these were worked out for.
    soft: Vec<f32>,
    soft_before: Vec<f32>,
    soft_for: [f32; 4],

    /// Bourdon flex: how far the pipe has straightened, and how fast that is changing.
    /// One is a full push held still; negative is curled tighter than at rest.
    straightening: f32,
    straightening_rate: f32,
}

impl Pipe {
    /// A pipe at rest, filled with contents that have not moved yet.
    pub fn new(mechanism: Mechanism, params: &Params, length: f32) -> Self {
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
            drive: 0.0,
            travelled: 0.0,
            swell: vec![0.0; CELLS],
            flow: vec![0.0; CELLS + 1],
            clock: 0.0,
            balloon: 0.0,
            soft: vec![0.0; CELLS + 1],
            soft_before: vec![0.0; CELLS + 1],
            soft_for: [f32::NAN; 4],
            straightening: 0.0,
            straightening_rate: 0.0,
        };
        pipe.soften(&params.balloon, length);
        pipe.shape(params, 0.0, length);
        pipe
    }

    pub const fn mechanism(&self) -> Mechanism {
        self.mechanism
    }

    /// Moves the pipe on by `seconds` under `forcing`, for the pipe lying at `lay`.
    ///
    /// Whole steps of [`STEP`] are taken, at least one and at most a fifth of a second's
    /// worth, so a stalled frame neither stops the pipe nor makes it leap.
    pub fn advance(&mut self, params: &Params, forcing: &Forcing, lay: &Lay, seconds: f32) {
        if !seconds.is_finite() || seconds <= 0.0 {
            return;
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = ((seconds / STEP).round() as u32).clamp(1, MAX_STEPS);
        for _ in 0..steps {
            self.step(params, forcing, lay);
        }
    }

    /// Moves the pipe on by one [`STEP`].
    ///
    /// Every model takes the same thing from the forcing: how hard the contents are
    /// pushed along the pipe in all, [`Forcing::drive`]. For a pipe that is full, with a
    /// wall that does not give, that is all an even acceleration such as gravity can do
    /// to the flow. What it does across the pipe is for whoever lays the centreline.
    pub fn step(&mut self, params: &Params, forcing: &Forcing, lay: &Lay) {
        let drive = forcing.drive(lay, params.full_head());
        let length = sane_length(lay.length);
        self.drive = drive;

        match self.mechanism {
            Mechanism::Advection => {}
            Mechanism::Peristalsis => {
                self.travelled += drive * params.peristalsis.speed * STEP;
            }
            Mechanism::PressureWave => self.step_wave(params, drive, length),
            Mechanism::Balloon => {
                self.soften(&params.balloon, length);
                self.balloon += self.balloon_rate(params, drive) * STEP;
            }
            Mechanism::Bourdon => {
                let bourdon = &params.bourdon;
                let rate = sane(bourdon.rate, 6.0).clamp(0.1, 60.0);
                let pull = rate * rate * (drive - self.straightening)
                    - 2.0 * sane(bourdon.damping, 0.45).max(0.0) * rate * self.straightening_rate;
                self.straightening_rate += pull * STEP;
                self.straightening += self.straightening_rate * STEP;
            }
        }

        self.shape(params, drive, length);
        self.carry(length);
    }

    /// Works out the cross-section and the speed of the contents at every point from the
    /// state of the model.
    fn shape(&mut self, params: &Params, drive: f32, length: f32) {
        match self.mechanism {
            Mechanism::Advection => {
                self.areas.fill(1.0);
                self.speeds.fill(drive * params.speed);
            }

            Mechanism::Bourdon => {
                // A flattened tube rounds out as it straightens.
                let area = params
                    .bourdon
                    .swell
                    .mul_add(self.straightening, 1.0)
                    .max(MIN_AREA);
                self.areas.fill(area);
                self.speeds.fill(drive * params.speed);
            }

            Mechanism::Peristalsis => {
                let rings = &params.peristalsis;
                let ring_speed = drive * rings.speed;
                for point in 0..=CELLS {
                    let area = rings.area(along(point, length) - self.travelled);
                    self.areas[point] = area;

                    // What is stored under the wall can only change by what flows past:
                    // the flow is the wall's speed times the cross-section, less what
                    // leaks back through the throat. The contents in the throat hold
                    // still; the contents between two throats are pushed along.
                    self.speeds[point] = ring_speed * (1.0 - rings.throat() / area);
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
                let growth = params.balloon.growth;
                let inflow = drive * params.speed;

                // What the stretch takes up each second for each unit of softness.
                let filling = self.balloon_rate(params, drive) * growth;

                for point in 0..=CELLS {
                    let area = (growth * self.balloon)
                        .mul_add(self.soft[point], 1.0)
                        .max(MIN_AREA);
                    self.areas[point] = area;

                    // Past the soft stretch the pipe carries what came in less what the
                    // stretch is taking up.
                    let flow = filling.mul_add(-self.soft_before[point], inflow);
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
    /// prescribed. Together they carry a change along the wall at [`Wave::speed`].
    fn step_wave(&mut self, params: &Params, drive: f32, length: f32) {
        #[allow(clippy::cast_precision_loss)]
        let spacing = length / CELLS as f32;
        let wave = &params.wave;
        let speed = wave.speed();
        let friction = sane(wave.friction, 0.6).max(0.0);

        // A short pipe needs shorter steps for the wave not to outrun the grid.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let parts = ((speed * STEP / (0.9 * spacing)).ceil() as u32).clamp(1, 64);
        #[allow(clippy::cast_precision_loss)]
        let dt = STEP / parts as f32;

        for _ in 0..parts {
            self.clock += dt;

            // The source decides the flow at the start. At the end the pipe runs on into
            // more pipe of the same kind, so a wave leaves without an echo.
            self.flow[0] = drive * params.speed * wave.beat(self.clock);
            self.flow[CELLS] = speed * self.swell[CELLS - 1];

            for segment in 0..CELLS {
                let gained = self.flow[segment] - self.flow[segment + 1];
                self.swell[segment] = (dt / spacing)
                    .mul_add(gained, self.swell[segment])
                    .max(MIN_AREA - 1.0);
            }

            for point in 1..CELLS {
                let push = speed * speed * (self.swell[point - 1] - self.swell[point]) / spacing;
                self.flow[point] += dt * friction.mul_add(-self.flow[point], push);
            }
        }
    }

    /// Works out how soft the wall of a ballooning pipe is at each point, if the pipe or
    /// the stretch has changed since it was last worked out.
    fn soften(&mut self, balloon: &Balloon, length: f32) {
        let soft_for = [length, balloon.at, balloon.reach, balloon.roundness];
        if soft_for.map(f32::to_bits) == self.soft_for.map(f32::to_bits) {
            return;
        }
        self.soft_for = soft_for;

        #[allow(clippy::cast_precision_loss)]
        let spacing = length / CELLS as f32;
        let middle = sane(balloon.at, 0.42).clamp(0.0, 1.0) * length;

        let mut before = 0.0;
        for point in 0..=CELLS {
            let soft = balloon.softness(along(point, length) - middle);
            if point > 0 {
                before += 0.5 * (self.soft[point - 1] + soft) * spacing;
            }
            self.soft[point] = soft;
            self.soft_before[point] = before;
        }
    }

    /// How fast the soft stretch of a ballooning pipe fills under a push of `drive`.
    ///
    /// It takes in what the source delivers and lets out what its own pressure drives on:
    /// so a push just over the threshold is enough to balloon it, and the push must fall
    /// well under the threshold to let it down. See [`Balloon::pressure`].
    pub fn balloon_rate(&self, params: &Params, drive: f32) -> f32 {
        let balloon = &params.balloon;

        // What the stretch holds for each unit of fill, as a length of pipe at rest.
        let holds = (balloon.growth * self.soft_before[CELLS]).max(1e-3);

        params.speed
            * balloon
                .threshold
                .mul_add(-balloon.pressure(self.balloon), drive)
            / holds
    }

    /// How hard the contents were pushed along the pipe in the last step, in all: the
    /// pump and the head, through the valve, as a share of a full command.
    pub const fn drive(&self) -> f32 {
        self.drive
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

/// `value`, or `otherwise` if it is not a number.
const fn sane(value: f32, otherwise: f32) -> f32 {
    if value.is_finite() { value } else { otherwise }
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
/// first point to the last by `straightening` times [`Bourdon::uncurl`]. A negative
/// `straightening` turns it away, by that times [`Bourdon::curl`]: a tube under suction
/// curls, but less than it straightens under pressure.
/// Because straightening a curl moves its far end, the whole is then turned and scaled
/// about the first point until the last is back in place, so every stretch ends up longer
/// or shorter by the same share.
///
/// This is geometry only: nothing here works out the forces that straighten a pressurised
/// tube. A real tube held at both ends could not lose curl without stretching; this one is
/// simply drawn shorter as it straightens and longer as it curls.
pub fn flex(points: &[Pos2], straightening: f32, bourdon: &Bourdon) -> Vec<Pos2> {
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
        bourdon.uncurl.mul_add(-straightening, 1.0)
    } else {
        bourdon.curl.mul_add(-straightening, 1.0)
    };
    let kept = sane(kept, 1.0).clamp(0.0, 2.0);
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
