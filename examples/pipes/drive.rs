//! What drives a pipe: a signal, physics, or both.
//!
//! The models in `flow.rs` and the recipe in `pipe.rs` never ask where a push came from.
//! Each step they are handed a [`Forcing`]: everything that acts on one pipe from outside,
//! added up. Whatever acts on pipes implements [`Influence`] and adds its part. So a pipe
//! can be driven by a signal alone, by physics alone, or by any mix, and a new kind of
//! influence is one more implementation of the trait, with no model touched.
//!
//! Two influences are built here: [`Signal`] and [`Gravity`]. The README lists the others
//! that were thought of, what each would write into a [`Forcing`], and which of them need
//! a channel that [`Forcing`] does not have yet.
//!
//! What a [`Forcing`] holds is split the way a pipe splits it:
//!
//! - along the pipe, it moves the contents: [`Forcing::push`], [`Forcing::opening`], and
//!   the part of [`Forcing::body`] that points from one end to the other
//!   ([`Forcing::head`]);
//! - across the pipe, it moves the pipe itself: the part of [`Forcing::body`] at right
//!   angles to that ([`Forcing::load`]), which `pipe.rs` lets the centreline sag under.

use egui::{Pos2, Vec2};
use egui_snarl::ui::WirePins;

/// Acceleration of free fall, in graph units per second squared, taking a graph unit for a
/// millimetre. [`Gravity::strength`] is a multiple of it.
pub const FREE_FALL: f32 = 9810.0;

/// Where one pipe lies, as far as an influence needs to know.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lay {
    /// Which wire the pipe is. An influence that differs from wire to wire, such as the
    /// value each wire carries, tells them apart by this.
    pub wire: Option<WirePins>,

    /// The pin the pipe leaves, in graph coordinates.
    pub from: Pos2,

    /// The pin the pipe enters.
    pub to: Pos2,

    /// Length of the pipe at rest.
    pub length: f32,
}

impl Lay {
    /// A pipe of `length` that runs level, from left to right.
    pub fn level(length: f32) -> Self {
        Self {
            wire: None,
            from: Pos2::ZERO,
            to: Pos2::new(length, 0.0),
            length,
        }
    }
}

/// Everything that acts on one pipe from outside during one step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forcing {
    /// How hard a pump at the inlet pushes, as a share of a full command. Negative pulls.
    pub push: f32,

    /// How far a valve at the inlet is open, from shut at zero to open at one. It lets
    /// through that share of whatever pushes, the pump and the head alike.
    pub opening: f32,

    /// Acceleration of the pipe and everything in it, in graph units per second squared.
    /// Gravity is one; a frame that is itself accelerating would be another.
    pub body: Vec2,
}

impl Forcing {
    /// Nothing acting: no pump, an open valve, no acceleration.
    pub const REST: Self = Self {
        push: 0.0,
        opening: 1.0,
        body: Vec2::ZERO,
    };

    /// What [`Forcing::body`] does along the pipe: the head between its two ends.
    ///
    /// For an acceleration that is the same everywhere this depends on where the ends are
    /// and not on the way the pipe takes between them.
    pub fn head(&self, lay: &Lay) -> f32 {
        let head = self.body.dot(lay.to - lay.from);
        if head.is_finite() { head } else { 0.0 }
    }

    /// What [`Forcing::body`] does across the pipe: the part of it at right angles to the
    /// line between the two ends.
    pub fn load(&self, lay: &Lay) -> Vec2 {
        let chord = lay.to - lay.from;
        let length_sq = chord.length_sq();
        if !(length_sq > 1e-6) || !self.body.is_finite() {
            return Vec2::ZERO;
        }
        self.body - chord * (self.body.dot(chord) / length_sq)
    }

    /// How hard the contents are pushed along the pipe in all, as a share of a full
    /// command and never more than one: the pump and the head, through the valve.
    ///
    /// `full_head` is the head that pushes as hard as a full command.
    pub fn drive(&self, lay: &Lay, full_head: f32) -> f32 {
        let from_head = if full_head > 0.0 {
            self.head(lay) / full_head
        } else {
            0.0
        };

        let drive = self.opening.clamp(0.0, 1.0) * (self.push + from_head);
        if drive.is_finite() {
            drive.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Something that acts on pipes.
pub trait Influence {
    /// What to call it where the influences on a pipe are listed.
    fn name(&self) -> &'static str;

    /// Moves the influence itself on by `seconds`, once a frame, before any pipe is asked
    /// about. Most have nothing to do here.
    fn advance(&mut self, _seconds: f32) {}

    /// Adds what this does to the pipe at `lay` to `forcing`.
    fn act(&self, lay: &Lay, forcing: &mut Forcing);
}

/// What a signal does at the inlet of a pipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// It pushes: the contents move because of it.
    Pump,

    /// It opens a valve: the contents move because of something else, such as gravity,
    /// and the signal says how much of that is let through.
    Valve,
}

/// The signal: one number in `-1.0..=1.0` for every pipe.
///
/// An application would give each wire the value that wire carries, by [`Lay::wire`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Signal {
    pub command: f32,
    pub role: Role,
}

impl Influence for Signal {
    fn name(&self) -> &'static str {
        match self.role {
            Role::Pump => "signal as pump",
            Role::Valve => "signal as valve",
        }
    }

    fn act(&self, _lay: &Lay, forcing: &mut Forcing) {
        if !self.command.is_finite() {
            return;
        }

        match self.role {
            Role::Pump => forcing.push += self.command.clamp(-1.0, 1.0),
            // A valve has no direction: it is as far open for a negative command.
            Role::Valve => forcing.opening *= self.command.abs().min(1.0),
        }
    }
}

/// Gravity: the same acceleration everywhere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gravity {
    /// As a multiple of [`FREE_FALL`]. Zero is no gravity.
    pub strength: f32,

    /// Which way is down, in degrees clockwise from straight down the screen.
    pub angle: f32,
}

impl Gravity {
    /// The acceleration, in graph units per second squared.
    pub fn pull(&self) -> Vec2 {
        if !self.strength.is_finite() || !self.angle.is_finite() {
            return Vec2::ZERO;
        }

        // Down the screen is +y; clockwise from there, as seen on screen, is toward -x.
        let (sin, cos) = self.angle.to_radians().sin_cos();
        self.strength * FREE_FALL * Vec2::new(-sin, cos)
    }
}

impl Influence for Gravity {
    fn name(&self) -> &'static str {
        "gravity"
    }

    fn act(&self, _lay: &Lay, forcing: &mut Forcing) {
        forcing.body += self.pull();
    }
}

/// The influences on the pipes of one graph.
pub struct Drives {
    pub signal: Signal,
    pub gravity: Gravity,

    /// Whatever else acts on the pipes. Nothing, here; this is where another influence
    /// is plugged in.
    pub more: Vec<Box<dyn Influence>>,
}

impl Default for Drives {
    fn default() -> Self {
        Self {
            signal: Signal {
                command: 0.8,
                role: Role::Pump,
            },
            gravity: Gravity {
                strength: 0.0,
                angle: 0.0,
            },
            more: Vec::new(),
        }
    }
}

impl Drives {
    pub fn advance(&mut self, seconds: f32) {
        self.signal.advance(seconds);
        self.gravity.advance(seconds);
        for influence in &mut self.more {
            influence.advance(seconds);
        }
    }

    /// Everything that acts on the pipe at `lay`, added up.
    pub fn forcing(&self, lay: &Lay) -> Forcing {
        let mut forcing = Forcing::REST;
        self.signal.act(lay, &mut forcing);
        self.gravity.act(lay, &mut forcing);
        for influence in &self.more {
            influence.act(lay, &mut forcing);
        }
        forcing
    }

    /// The names of the influences that are doing something, for a readout.
    pub fn active(&self) -> Vec<&'static str> {
        let mut names = vec![self.signal.name()];
        if self.gravity.strength != 0.0 {
            names.push(self.gravity.name());
        }
        names.extend(self.more.iter().map(|influence| influence.name()));
        names
    }
}
