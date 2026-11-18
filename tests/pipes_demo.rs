//! Checks the five models of the `pipes` example, and runs its graph through headless egui
//! passes to check that each pipe is drawn between the walls it is picked between.

#[allow(dead_code)]
#[path = "../examples/pipes/flow.rs"]
mod flow;
#[allow(dead_code)]
#[path = "../examples/pipes/graph.rs"]
mod graph;
#[allow(dead_code)]
#[path = "../examples/pipes/pipe.rs"]
mod pipe;
#[allow(dead_code)]
#[path = "../examples/interaction/tube.rs"]
mod tube;

use std::sync::Arc;

use egui::{Context, Event, FullOutput, Id, Mesh, Pos2, RawInput, Rect, Shape, Ui, pos2, vec2};
use egui_snarl::{
    InPin, OutPin, Snarl,
    ui::{
        CableGeometry, SnarlAnatomy, SnarlPin, SnarlViewer, SnarlWidget, WirePins,
        get_selected_wire, get_snarl_anatomy,
    },
};
use flow::{CELLS, Mechanism, Pipe, STEP};
use graph::{Assign, Look, Node, Viewer};

const LENGTH: f32 = 640.0;

/// Runs a pipe for `seconds` under one command.
fn run(pipe: &mut Pipe, command: f32, seconds: f32) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (seconds / STEP).round() as usize;
    for _ in 0..steps {
        pipe.step(command, LENGTH);
    }
}

#[allow(clippy::cast_precision_loss)]
fn share(point: usize) -> f32 {
    point as f32 / CELLS as f32
}

/// The flow through each point of the grid: cross-section times speed.
fn flows(pipe: &Pipe) -> Vec<f32> {
    (0..=CELLS)
        .map(|point| pipe.area_at(share(point)) * pipe.speed_at(share(point)))
        .collect()
}

fn areas(pipe: &Pipe) -> Vec<f32> {
    (0..=CELLS)
        .map(|point| pipe.area_at(share(point)))
        .collect()
}

/// Where along the pipe the contents with `label` are, as a share of its length.
fn where_is(pipe: &Pipe, label: f32) -> Option<f32> {
    (0..CELLS).find_map(|point| {
        let (here, next) = (pipe.label_at(share(point)), pipe.label_at(share(point + 1)));
        (here >= label && label > next)
            .then(|| share(point) + (here - label) / (here - next) / CELLS as f32)
    })
}

/// Over one step and over each stretch of sixteen segments: the largest mismatch between
/// what the stretch gained in contents and what flowed in at one end less what flowed out
/// at the other; and the largest gain, to judge it against.
fn continuity(pipe: &mut Pipe, command: f32) -> (f32, f32) {
    let spacing = LENGTH / CELLS as f32;
    let held = |areas: &[f32], from: usize, to: usize| {
        let ends = 0.5 * (areas[from] + areas[to]);
        (areas[from + 1..to].iter().sum::<f32>() + ends) * spacing
    };

    let (before, flow_before) = (areas(pipe), flows(pipe));
    pipe.step(command, LENGTH);
    let (after, flow_after) = (areas(pipe), flows(pipe));

    let mut worst = 0.0f32;
    let mut largest = 0.0f32;
    for from in (0..CELLS).step_by(8) {
        let to = (from + 16).min(CELLS);
        let gained = (held(&after, from, to) - held(&before, from, to)) / STEP;
        let flowed_in =
            0.5 * ((flow_before[from] - flow_before[to]) + (flow_after[from] - flow_after[to]));

        worst = worst.max((gained - flowed_in).abs());
        largest = largest.max(gained.abs());
    }
    (worst, largest)
}

#[test]
fn carried_contents_move_at_the_commanded_speed_and_come_back_when_it_reverses() {
    let mut pipe = Pipe::new(Mechanism::Advection, LENGTH);

    // At rest the pipe is filled evenly and nothing moves.
    let marked = pipe.label_at(0.25);
    assert!((where_is(&pipe, marked).unwrap() - 0.25).abs() < 1e-3);
    run(&mut pipe, 0.0, 1.0);
    assert!((where_is(&pipe, marked).unwrap() - 0.25).abs() < 1e-3);

    // A full command carries the marked contents on by the speed times the time.
    run(&mut pipe, 1.0, 1.5);
    let carried = (where_is(&pipe, marked).unwrap() - 0.25) * LENGTH;
    assert!(
        (carried - 1.5 * flow::SPEED).abs() < 1.0,
        "carried {carried}"
    );

    // Half the command, half the speed.
    run(&mut pipe, 0.5, 1.0);
    let carried = (where_is(&pipe, marked).unwrap() - 0.25) * LENGTH;
    assert!(
        (carried - 2.0 * flow::SPEED).abs() < 1.0,
        "carried {carried}"
    );

    // A negative command brings them back the way they came.
    run(&mut pipe, -1.0, 2.0);
    assert!((where_is(&pipe, marked).unwrap() - 0.25).abs() < 2e-3);

    // The wall never moved, and the contents stayed one connected run: labels fall all
    // the way along, with no gap and no fold.
    assert!(
        areas(&pipe)
            .iter()
            .all(|&area| area.to_bits() == 1.0f32.to_bits())
    );
    for point in 0..CELLS {
        assert!(pipe.label_at(share(point)) > pipe.label_at(share(point + 1)));
    }
}

#[test]
fn a_travelling_contraction_pushes_the_contents_and_obeys_continuity() {
    let mut pipe = Pipe::new(Mechanism::Peristalsis, LENGTH);

    // The wall is where the model put it: closed down to the throat at each contraction,
    // fully open between them.
    let (narrowest, widest) = areas(&pipe)
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), &area| {
            (low.min(area), high.max(area))
        });
    assert!(
        (narrowest - flow::SQUEEZE_THROAT).abs() < 0.02,
        "{narrowest}"
    );
    assert!((widest - 1.0).abs() < 1e-6);

    // The contractions travel at the commanded speed.
    let throat = |pipe: &Pipe| {
        let areas = areas(pipe);
        let point = (0..=CELLS / 4)
            .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
            .unwrap();
        share(point) * LENGTH
    };
    let from = throat(&pipe);
    run(&mut pipe, 1.0, 0.25);
    let moved = throat(&pipe) - from;
    assert!(
        (moved - 0.25 * flow::SQUEEZE_SPEED).abs() < LENGTH / CELLS as f32,
        "moved {moved}"
    );

    // What each stretch gains in cross-section is what flows in less what flows out.
    for _ in 0..40 {
        let (mismatch, largest) = continuity(&mut pipe, 1.0);
        assert!(largest > 10.0, "the wall is moving: {largest}");
        assert!(
            mismatch < 0.02 * largest,
            "mismatch {mismatch} against a gain of {largest}"
        );
    }

    // The contents in a throat hold still; between throats they go the way the wall goes,
    // but never as fast as the wall.
    for point in 0..=CELLS {
        let (area, speed) = (pipe.area_at(share(point)), pipe.speed_at(share(point)));
        assert!(speed >= -1e-3 && speed < flow::SQUEEZE_SPEED);
        if area < flow::SQUEEZE_THROAT + 0.01 {
            assert!(speed < 0.05 * flow::SQUEEZE_SPEED, "in a throat: {speed}");
        }
        if area > 0.999 {
            let expected = flow::SQUEEZE_SPEED * (1.0 - flow::SQUEEZE_THROAT);
            assert!((speed - expected).abs() < 0.5, "between throats: {speed}");
        }
    }

    // Over a whole number of contractions the contents have moved on, and a negative
    // command moves them back.
    let marked = pipe.label_at(0.3);
    let period = flow::SQUEEZE_SPACING / flow::SQUEEZE_SPEED;
    run(&mut pipe, 1.0, 2.0 * period);
    let forward = (where_is(&pipe, marked).unwrap() - 0.3) * LENGTH;
    assert!(
        forward > 0.3 * flow::SQUEEZE_SPACING && forward < 2.0 * flow::SQUEEZE_SPACING,
        "forward {forward}"
    );

    run(&mut pipe, -1.0, 2.0 * period);
    let back = (where_is(&pipe, marked).unwrap() - 0.3) * LENGTH;
    assert!(back.abs() < 0.1 * forward, "back to {back}");

    // No command, no motion: the wall holds its grip and nothing is pushed.
    run(&mut pipe, 0.0, 1.0);
    assert!((0..=CELLS).all(|point| pipe.speed_at(share(point)).abs() < 1e-6));
}

#[test]
fn a_beat_of_pressure_runs_along_the_wall_ahead_of_the_contents() {
    let mut pipe = Pipe::new(Mechanism::PressureWave, LENGTH);

    // Nothing asked, nothing happens.
    run(&mut pipe, 0.0, 1.0);
    assert!(areas(&pipe).iter().all(|&area| (area - 1.0).abs() < 1e-6));

    // The first beat widens the pipe at the source, and the widening runs down the pipe
    // at the speed of the wall wave, not at the speed of the contents.
    let front = |pipe: &Pipe| {
        let areas = areas(pipe);
        let peak = areas.iter().copied().fold(1.0, f32::max);
        let half = 0.5f32.mul_add(peak - 1.0, 1.0);
        (0..=CELLS)
            .rev()
            .find(|&point| areas[point] > half)
            .map(|point| share(point) * LENGTH)
    };

    let mut pipe = Pipe::new(Mechanism::PressureWave, LENGTH);
    let marked = pipe.label_at(0.02);

    run(&mut pipe, 1.0, 0.45);
    let early = front(&pipe).expect("the beat has widened the pipe");
    run(&mut pipe, 1.0, 0.6);
    let late = front(&pipe).unwrap();

    let wave_speed = (late - early) / 0.6;
    assert!(
        (wave_speed - flow::WAVE_SPEED).abs() < 0.12 * flow::WAVE_SPEED,
        "the front ran at {wave_speed}"
    );

    let contents_moved = (where_is(&pipe, marked).unwrap() - 0.02) * LENGTH;
    assert!(
        contents_moved > 10.0,
        "the contents do move: {contents_moved}"
    );
    assert!(
        contents_moved < 0.5 * (late - early),
        "the contents moved {contents_moved} while the front ran {}",
        late - early
    );

    // The wall is widest where the beat is, by about a fifth.
    let peak = areas(&pipe).iter().copied().fold(1.0, f32::max);
    assert!(peak > 1.3 && peak < 2.2, "peak cross-section {peak}");

    // What the pipe holds changes by exactly what came in less what went out.
    let mut pipe = Pipe::new(Mechanism::PressureWave, LENGTH);
    let mut expected = pipe.volume(LENGTH);
    for _ in 0..600 {
        let before = flows(&pipe);
        pipe.step(1.0, LENGTH);
        let after = flows(&pipe);
        expected += 0.5 * STEP * ((before[0] - before[CELLS]) + (after[0] - after[CELLS]));
    }
    let held = pipe.volume(LENGTH);
    assert!(
        held > LENGTH + 20.0,
        "the pipe has taken up contents: {held}"
    );
    assert!(
        (held - expected).abs() < 0.02 * (held - LENGTH),
        "holds {held}, flows account for {expected}"
    );
}

#[test]
fn in_a_pressure_wave_the_wall_and_the_flow_drive_each_other() {
    // Push one beat in, then stop the source. From here on nothing is prescribed but
    // that no more comes in.
    let mut pipe = Pipe::new(Mechanism::PressureWave, LENGTH);
    run(&mut pipe, 1.0, 0.3);
    let widest = |pipe: &Pipe| {
        let areas = areas(pipe);
        let point = (0..=CELLS)
            .max_by(|&a, &b| areas[a].total_cmp(&areas[b]))
            .unwrap();
        (share(point), areas[point])
    };
    let (started_at, started_as) = widest(&pipe);
    assert!(started_as > 1.2 && started_at < 0.3);

    run(&mut pipe, 0.0, 0.8);
    let (now_at, now_as) = widest(&pipe);

    // The wall acts on the flow: the widened stretch, with no source behind it, drives
    // the contents under it forward and so moves itself down the pipe.
    assert!(
        now_at > started_at + 0.3,
        "the bulge went from {started_at} to {now_at}"
    );
    assert!(now_as > 1.1, "and is still a bulge: {now_as}");
    assert!(pipe.speed_at(now_at) > 20.0);

    // The flow acts on the wall: where the contents move, the wall is wide, and where
    // they are at rest, behind the bulge, it has come back to rest.
    assert!(pipe.speed_at(0.05).abs() < 0.1 * pipe.speed_at(now_at));
    assert!((pipe.area_at(0.05) - 1.0).abs() < 0.1 * (now_as - 1.0));

    // In a wave running one way the two keep a fixed ratio: the flow is the widening
    // times the speed of the wave.
    let flow = pipe.speed_at(now_at) * now_as;
    let from_wall = flow::WAVE_SPEED * (now_as - 1.0);
    assert!(
        (flow - from_wall).abs() < 0.15 * from_wall,
        "flow {flow}, from the wall {from_wall}"
    );

    // And the wave leaves at the far end without coming back: once it is gone, what is
    // left in the pipe drains away and the wall comes to rest.
    run(&mut pipe, 0.0, 1.2);
    let left = areas(&pipe)
        .iter()
        .map(|area| (area - 1.0).abs())
        .fold(0.0, f32::max);
    assert!(
        left < 0.25 * (now_as - 1.0),
        "{left} left of {}",
        now_as - 1.0
    );
    run(&mut pipe, 0.0, 8.0);
    assert!(areas(&pipe).iter().all(|&area| (area - 1.0).abs() < 0.01));
}

#[test]
fn a_soft_stretch_holds_until_a_threshold_then_balloons_and_stays() {
    let (gives_way, gives_back) = flow::BALLOON_LIMITS;

    // The pressure in the soft stretch rises to a limit, falls as it gives way, and rises
    // again.
    assert!((flow::balloon_pressure(gives_way) - 1.0).abs() < 1e-6);
    assert!(flow::balloon_pressure(gives_way - 0.1) < 1.0);
    assert!(flow::balloon_pressure(gives_way + 0.1) < 1.0);
    let valley = flow::balloon_pressure(gives_back);
    assert!(valley < 0.8 && valley > 0.5, "valley {valley}");
    assert!(flow::balloon_pressure(gives_back + 0.1) > valley);
    assert!(flow::balloon_pressure(gives_back - 0.1) > valley);

    let below = flow::BALLOON_THRESHOLD - 0.1;
    let above = flow::BALLOON_THRESHOLD + 0.2;
    let width = |pipe: &Pipe| pipe.area_at(flow::BALLOON_AT);

    // Under the threshold the stretch widens a little and settles.
    let mut pipe = Pipe::new(Mechanism::Balloon, LENGTH);
    run(&mut pipe, below, 12.0);
    let held = pipe.balloon();
    assert!(held > 0.2 && held < gives_way, "held at {held}");
    assert!(pipe.balloon_rate(below).abs() < 1e-3, "and is settled");
    let width_held = width(&pipe);

    // Over the threshold it gives way and balloons.
    run(&mut pipe, above, 12.0);
    let ballooned = pipe.balloon();
    assert!(ballooned > gives_back, "ballooned to {ballooned}");
    assert!(width(&pipe) > 2.0 * width_held);

    // Back at the command it held under before, it stays ballooned: the same command,
    // another state.
    run(&mut pipe, below, 12.0);
    assert!(pipe.balloon() > gives_back, "stayed at {}", pipe.balloon());
    assert!(pipe.balloon_rate(below).abs() < 1e-3);

    // Only well under the threshold does it let down again.
    run(&mut pipe, valley * flow::BALLOON_THRESHOLD - 0.1, 12.0);
    assert!(pipe.balloon() < gives_way, "let down to {}", pipe.balloon());

    // The width is where the model says, and only around the soft stretch.
    let mut pipe = Pipe::new(Mechanism::Balloon, LENGTH);
    run(&mut pipe, above, 12.0);
    let expected = flow::BALLOON_GROWTH.mul_add(pipe.balloon(), 1.0);
    assert!((width(&pipe) - expected).abs() < 0.05);
    assert!((pipe.area_at(0.1) - 1.0).abs() < 1e-3);
    assert!((pipe.area_at(0.95) - 1.0).abs() < 1e-3);

    // While it fills, less leaves the pipe than enters it, by what the stretch takes up.
    let mut pipe = Pipe::new(Mechanism::Balloon, LENGTH);
    run(&mut pipe, 1.0, 1.0);
    let flows = flows(&pipe);
    let taken_up = pipe.balloon_rate(1.0)
        * flow::BALLOON_GROWTH
        * flow::BALLOON_REACH
        * (2.0 * std::f32::consts::PI).sqrt();
    assert!(taken_up > 5.0);
    assert!((flows[0] - flow::SPEED).abs() < 1e-3);
    assert!((flows[0] - flows[CELLS] - taken_up).abs() < 0.01 * taken_up);

    let (mismatch, largest) = continuity(&mut pipe, 1.0);
    assert!(
        mismatch < 0.05 * largest,
        "mismatch {mismatch}, gain {largest}"
    );
}

/// How much a path turns in all, in radians.
fn curl(points: &[Pos2]) -> f32 {
    points
        .windows(3)
        .map(|triple| {
            let (a, b) = (triple[1] - triple[0], triple[2] - triple[1]);
            (a.x * b.y - a.y * b.x).atan2(a.dot(b)).abs()
        })
        .sum()
}

#[test]
fn a_curled_pipe_straightens_between_ends_that_stay_put() {
    // An arc of a third of a turn, about as curled as the pipe of the example is at rest.
    const TURN: f32 = 2.1;
    let arc: Vec<Pos2> = (0..=60)
        .map(|step| {
            #[allow(clippy::cast_precision_loss)]
            let angle = TURN * step as f32 / 60.0;
            pos2(100.0, 50.0) + 80.0 * vec2(angle.cos(), angle.sin())
        })
        .collect();

    assert_eq!(flow::flex(&arc, 0.0).len(), arc.len());
    for (flexed, &rest) in flow::flex(&arc, 0.0).iter().zip(&arc) {
        assert!(flexed.distance(rest) < 1e-2, "nothing asked, nothing moved");
    }

    let mut curls = Vec::new();
    for straightening in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let flexed = flow::flex(&arc, straightening);
        assert_eq!(flexed.len(), arc.len());

        // Both ends are exactly where they were.
        assert_eq!(flexed[0], arc[0]);
        assert_eq!(flexed[60], arc[60]);

        // Every stretch is longer or shorter by the same share, and not by much.
        let ratios: Vec<f32> = flexed
            .windows(2)
            .zip(arc.windows(2))
            .map(|(now, rest)| now[0].distance(now[1]) / rest[0].distance(rest[1]))
            .collect();
        let (least, most) = ratios
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), &ratio| {
                (low.min(ratio), high.max(ratio))
            });
        assert!(most - least < 2e-3, "straightening {straightening}");

        // It is drawn shorter as it straightens and longer as it curls, by this much.
        let expected = if straightening > 0.0 {
            0.8..1.0
        } else if straightening < 0.0 {
            1.0..1.3
        } else {
            0.999..1.001
        };
        assert!(
            expected.contains(&least) && expected.contains(&most),
            "straightening {straightening}: {least}..{most}"
        );

        curls.push(curl(&flexed));
    }

    // The more pressure, the less curl; pulled instead of pushed, it curls tighter.
    assert!(
        curls.windows(2).all(|pair| pair[0] > pair[1] + 0.05),
        "{curls:?}"
    );
    assert!((curls[2] - TURN * 59.0 / 60.0).abs() < 0.02);

    // A straight pipe has nothing to lose, and odd input comes back as it went in.
    let straight = [pos2(0.0, 0.0), pos2(50.0, 0.0), pos2(100.0, 0.0)];
    for (flexed, rest) in flow::flex(&straight, 1.0).iter().zip(straight) {
        assert!(flexed.distance(rest) < 1e-3);
    }
    assert_eq!(flow::flex(&arc, f32::NAN), arc);
    assert_eq!(flow::flex(&arc[..2], 1.0), &arc[..2]);
    assert!(flow::flex(&[], 1.0).is_empty());

    // The model: the pipe follows the command with a swing and settles on it.
    let mut pipe = Pipe::new(Mechanism::Bourdon, LENGTH);
    assert_eq!(pipe.straightening().to_bits(), 0.0f32.to_bits());
    run(&mut pipe, 1.0, 0.4);
    let early = pipe.straightening();
    assert!(early > 0.3 && early < 1.3, "on its way: {early}");
    run(&mut pipe, 1.0, 4.0);
    assert!((pipe.straightening() - 1.0).abs() < 0.01);
    run(&mut pipe, -1.0, 4.0);
    assert!((pipe.straightening() + 1.0).abs() < 0.01);
}

/// What `graph::Viewer` shows, with nothing asked of the wires.
struct Bare(Viewer);

impl SnarlViewer<Node> for Bare {
    fn title(&mut self, node: &Node) -> String {
        self.0.title(node)
    }

    fn inputs(&mut self, node: &Node) -> usize {
        self.0.inputs(node)
    }

    fn outputs(&mut self, node: &Node) -> usize {
        self.0.outputs(node)
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        self.0.show_input(pin, ui, snarl)
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) -> impl SnarlPin + 'static {
        self.0.show_output(pin, ui, snarl)
    }

    fn center_view(&mut self, snarl: &mut Snarl<Node>) -> bool {
        self.0.center_view(snarl)
    }

    fn measured(&mut self, anatomy: &SnarlAnatomy, ui: &mut Ui, snarl: &mut Snarl<Node>) {
        self.0.measured(anatomy, ui, snarl);
    }
}

const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1280.0, 720.0));

fn view() -> Id {
    Id::unique("pipes under test")
}

struct World {
    ctx: Context,
    time: f64,
    snarl: Snarl<Node>,
    viewer: Viewer,
}

impl World {
    fn new(look: Look, assign: Assign) -> Self {
        let mut viewer = Viewer::new();
        viewer.look = look;
        viewer.assign = assign;
        viewer.fit();

        let mut world = Self {
            ctx: Context::default(),
            time: 0.0,
            snarl: graph::seed(),
            viewer,
        };
        for _ in 0..10 {
            let _ = world.pass(0.0, Vec::new());
        }
        world
    }

    /// One frame: the models move on by `seconds`, then the graph is shown.
    fn pass(&mut self, seconds: f32, events: Vec<Event>) -> FullOutput {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(SCREEN),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            focused: true,
            events,
            ..RawInput::default()
        };

        let Self {
            ctx, snarl, viewer, ..
        } = self;
        let look = viewer.look;
        viewer.advance(snarl, seconds);

        let mut output = ctx.run_ui(input, |ui| {
            graph::show(snarl, viewer, look, view(), ui);
        });
        output.textures_delta.clear();
        output
    }

    fn run(&mut self, seconds: f32) -> FullOutput {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames = (seconds * 60.0).round() as usize;
        for _ in 1..frames {
            let _ = self.pass(1.0 / 60.0, Vec::new());
        }
        self.pass(1.0 / 60.0, Vec::new())
    }

    /// Moves the pointer without letting the models move on.
    fn point_at(&mut self, pos: Pos2) {
        let _ = self.pass(0.0, vec![Event::PointerMoved(pos)]);
        let _ = self.pass(0.0, Vec::new());
    }

    fn anatomy(&self) -> Arc<SnarlAnatomy> {
        get_snarl_anatomy(view(), &self.ctx).expect("the view has been shown")
    }

    fn wires(&self) -> Vec<(WirePins, Mechanism)> {
        let mut wires: Vec<(WirePins, Mechanism)> = self
            .snarl
            .wires()
            .map(|(out_pin, in_pin)| {
                (
                    WirePins::new(out_pin, in_pin),
                    self.viewer.mechanism(&self.snarl, in_pin.node),
                )
            })
            .collect();
        wires.sort_by_key(|(pins, _)| *pins);
        wires
    }
}

fn meshes(output: &FullOutput) -> Vec<Arc<Mesh>> {
    fn take_apart(shape: &Shape, meshes: &mut Vec<Arc<Mesh>>) {
        match shape {
            Shape::Vec(group) => {
                for shape in group {
                    take_apart(shape, meshes);
                }
            }
            Shape::Mesh(mesh) => meshes.push(mesh.clone()),
            _ => {}
        }
    }

    let mut found = Vec::new();
    for clipped in &output.shapes {
        take_apart(&clipped.shape, &mut found);
    }
    found
}

/// Whether a mesh has its points on the two walls of `tube`, in order.
fn fills(mesh: &Mesh, tube: &CableGeometry) -> bool {
    let [near, far] = tube.walls();
    mesh.vertices.len() == 2 * near.len()
        && mesh.vertices.chunks(2).enumerate().all(|(index, pair)| {
            pair[0].pos.distance(near[index]) < 0.05 && pair[1].pos.distance(far[index]) < 0.05
        })
}

#[test]
fn every_pipe_is_drawn_between_the_walls_it_is_picked_between_as_it_moves() {
    let mut world = World::new(Look::Pipes, Assign::Each);
    let wires = world.wires();
    assert_eq!(
        wires
            .iter()
            .map(|&(_, mechanism)| mechanism)
            .collect::<Vec<_>>(),
        Mechanism::ALL
    );

    let mut widths: Vec<Vec<f32>> = vec![Vec::new(); wires.len()];
    let mut middles: Vec<Vec<Pos2>> = vec![Vec::new(); wires.len()];

    for moment in 0..6 {
        // Push for a while, then pull, so every mechanism is caught both ways.
        world.viewer.command = if moment < 4 { 1.0 } else { -1.0 };
        let output = world.run(0.9);

        let anatomy = world.anatomy();
        let to_screen = anatomy.transform.to_global;
        let found = meshes(&output);
        assert_eq!(found.len(), 3 * wires.len(), "three bands to a pipe");

        for (index, &(pins, mechanism)) in wires.iter().enumerate() {
            let what = format!("{mechanism:?} at moment {moment}");
            let measured = anatomy.wire(pins).expect("the pipe was measured");
            let pipe = world.viewer.pipe(pins).expect("the pipe has a model");
            let tube = &measured.geometry;

            // The pipe ends on the two pins, whatever it is doing in between.
            assert_eq!(
                tube.start(),
                anatomy.out_pin(pins.out_pin).unwrap().graph_center,
                "{what}"
            );
            assert_eq!(
                tube.end(),
                anatomy.in_pin(pins.in_pin).unwrap().graph_center,
                "{what}"
            );

            // Its half width is the model's cross-section, everywhere.
            for step in 0..=20 {
                #[allow(clippy::cast_precision_loss)]
                let share = step as f32 / 20.0;
                let expected = pipe::RADIUS * pipe.area_at(share).sqrt();
                assert!(
                    (tube.radius_at(share) - expected).abs() < 0.05,
                    "{what}, {share} along: {} against {expected}",
                    tube.radius_at(share)
                );
            }

            // What is painted fills exactly that pipe.
            let on_screen = tube.transformed(to_screen);
            assert_eq!(
                found.iter().filter(|mesh| fills(mesh, &on_screen)).count(),
                1,
                "{what}: one band from wall to wall of the measured pipe"
            );

            // The contents are one connected run of colour: labels fall all along.
            for point in 0..CELLS {
                assert!(
                    pipe.label_at(share(point)) > pipe.label_at(share(point + 1)),
                    "{what}: contents folded at {point}"
                );
            }

            let widest = (0..=40)
                .map(|step| {
                    #[allow(clippy::cast_precision_loss)]
                    let share = 0.1 + 0.8 * step as f32 / 40.0;
                    share
                })
                .max_by(|&a, &b| on_screen.radius_at(a).total_cmp(&on_screen.radius_at(b)))
                .unwrap();
            let narrowest = (0..=40)
                .map(|step| {
                    #[allow(clippy::cast_precision_loss)]
                    let share = 0.1 + 0.8 * step as f32 / 40.0;
                    share
                })
                .min_by(|&a, &b| on_screen.radius_at(a).total_cmp(&on_screen.radius_at(b)))
                .unwrap();

            widths[index].push(tube.radius_at(widest) / tube.radius_at(narrowest));
            middles[index].push(tube.anchor_at(0.5).pos);

            // The pointer is on the pipe just inside its wall where it is widest, on
            // either side, and off it just outside.
            let anchor = on_screen.anchor_at(widest);
            let wall = on_screen.radius_at(widest);
            for side in [-1.0, 1.0] {
                world.point_at(anchor.pos + anchor.normal * (side * (wall - 1.0)));
                assert_eq!(
                    world.anatomy().hovered_wire().map(|wire| wire.pins),
                    Some(pins),
                    "{what}: inside the wall, {wall} from the centreline"
                );
            }

            // Farther out than the band any wire is picked in, and outside this wall.
            let band = measured.hit_tolerance * anatomy.transform.scale();
            world.point_at(anchor.pos + anchor.normal * (wall.max(band) + 2.0));
            assert_ne!(
                world.anatomy().hovered_wire().map(|wire| wire.pins),
                Some(pins),
                "{what}: outside the wall"
            );
        }
    }

    // Each mechanism did what makes it that mechanism, in the pipe that was drawn.
    let by = |mechanism: Mechanism| {
        wires
            .iter()
            .position(|&(_, other)| other == mechanism)
            .unwrap()
    };

    // A fixed wall: the same width everywhere, always.
    assert!(
        widths[by(Mechanism::Advection)]
            .iter()
            .all(|&ratio| (ratio - 1.0).abs() < 1e-3)
    );

    // Contractions down to the throat.
    let squeeze = (1.0 / flow::SQUEEZE_THROAT).sqrt();
    assert!(
        widths[by(Mechanism::Peristalsis)]
            .iter()
            .all(|&ratio| (ratio - squeeze).abs() < 0.15)
    );

    // A wall that widens under a beat.
    assert!(
        widths[by(Mechanism::PressureWave)]
            .iter()
            .any(|&ratio| ratio > 1.12)
    );

    // A balloon under a full command, more than twice the width of the pipe beside it.
    assert!(widths[by(Mechanism::Balloon)][3] > 2.0);

    // A centreline that moves between its fixed ends, and moves back when pulled.
    let bourdon = &middles[by(Mechanism::Bourdon)];
    let rest_to_pushed = bourdon[3].distance(bourdon[5]);
    assert!(rest_to_pushed > 40.0, "the middle moved {rest_to_pushed}");
    for other in [
        Mechanism::Advection,
        Mechanism::Peristalsis,
        Mechanism::Balloon,
    ] {
        let middle = &middles[by(other)];
        assert!(middle[3].distance(middle[5]) < 1e-3, "{other:?} stays put");
    }
}

#[test]
fn a_pipe_is_selected_and_removed_where_it_is_drawn() {
    let mut world = World::new(Look::Pipes, Assign::All(Mechanism::Balloon));
    world.viewer.command = 1.0;
    let _ = world.run(5.0);

    let (pins, _) = world.wires()[2];
    let anatomy = world.anatomy();
    let tube = anatomy
        .wire(pins)
        .unwrap()
        .geometry
        .transformed(anatomy.transform.to_global);

    // Deep in the balloon, far from the centreline.
    let anchor = tube.anchor_at(flow::BALLOON_AT);
    let wall = tube.radius_at(flow::BALLOON_AT);
    assert!(wall > 2.0 * tube.radius_at(0.1));
    let inside = anchor.pos + anchor.normal * (wall - 1.5);

    let click = |world: &mut World, button| {
        world.point_at(inside);
        for pressed in [true, false] {
            let _ = world.pass(
                0.0,
                vec![Event::PointerButton {
                    pos: inside,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        let _ = world.pass(0.0, Vec::new());
    };

    click(&mut world, egui::PointerButton::Primary);
    assert_eq!(get_selected_wire(view(), &world.ctx), Some(pins));

    click(&mut world, egui::PointerButton::Secondary);
    assert_eq!(world.snarl.wires().count(), 4);
    assert!(world.wires().iter().all(|&(other, _)| other != pins));
}

#[test]
fn a_plain_look_leaves_the_graph_as_a_viewer_with_no_wire_hooks_shows_it() {
    /// What a pass paints, as text, in an order that does not depend on which node a
    /// view happens to draw first.
    fn shapes(output: &FullOutput) -> Vec<String> {
        fn take_apart(shape: &Shape, shapes: &mut Vec<String>) {
            match shape {
                Shape::Vec(group) => {
                    for shape in group {
                        take_apart(shape, shapes);
                    }
                }
                Shape::Noop => {}
                // Where a glyph sits in the font atlas depends on which text was laid
                // out first. What is written, where and in what colour does not.
                Shape::Text(text) => shapes.push(format!(
                    "Text {:?} at {:?} in {:?}",
                    text.galley.text(),
                    text.pos,
                    text.fallback_color
                )),
                shape => shapes.push(format!("{shape:?}")),
            }
        }

        let mut shapes = Vec::new();
        for clipped in &output.shapes {
            take_apart(&clipped.shape, &mut shapes);
        }
        shapes.sort();
        shapes
    }

    let mut plain = World::new(Look::Plain, Assign::Each);
    plain.viewer.command = 1.0;
    let with_viewer = plain.run(1.0);
    assert!(meshes(&with_viewer).is_empty());

    // The same graph, shown by a viewer that has no wire hooks at all, with a style
    // nothing was set in.
    let ctx = Context::default();
    let mut snarl = graph::seed();
    let mut bare = Bare(Viewer::new());
    bare.0.fit();

    let mut without_hooks = None;
    for frame in 0..70 {
        let input = RawInput {
            screen_rect: Some(SCREEN),
            time: Some(f64::from(frame + 1) / 60.0),
            predicted_dt: 1.0 / 60.0,
            focused: true,
            ..RawInput::default()
        };
        bare.0
            .advance(&snarl, if frame < 10 { 0.0 } else { 1.0 / 60.0 });

        let mut output = ctx.run_ui(input, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            SnarlWidget::new()
                .id(view())
                .show(&mut snarl, &mut bare, ui);
        });
        output.textures_delta.clear();
        without_hooks = Some(output);
    }

    assert_eq!(shapes(&with_viewer), shapes(&without_hooks.unwrap()));

    // And the other two looks do paint the wires another way, so the comparison above
    // was not between two empty pictures.
    let mut pipes = World::new(Look::Pipes, Assign::Each);
    assert_ne!(shapes(&pipes.run(1.0)), shapes(&with_viewer));
    let mut beads = World::new(Look::Beads, Assign::Each);
    assert_ne!(shapes(&beads.run(1.0)), shapes(&with_viewer));
}
