//! Checks the five models of the `pipes` example and what drives them, and runs its graph
//! through headless egui passes to check that each pipe is drawn between the walls it is
//! picked between.

#[allow(dead_code)]
#[path = "../examples/pipes/drive.rs"]
mod drive;
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

use drive::{Drives, FREE_FALL, Forcing, Gravity, Influence, Lay, Role, Signal};
use egui::{
    Context, Event, FullOutput, Id, Mesh, Pos2, RawInput, Rect, Shape, Ui, Vec2, pos2, vec2,
};
use egui_snarl::{
    InPin, NodeId, OutPin, Snarl,
    ui::{
        CableGeometry, SnarlAnatomy, SnarlPin, SnarlViewer, SnarlWidget, WirePins,
        get_selected_wire, get_snarl_anatomy,
    },
};
use flow::{CELLS, Mechanism, Params, Pipe, STEP};
use graph::{Assign, Look, Node, Viewer};
use pipe::Ends;

const LENGTH: f32 = 640.0;

/// A pump pushing at `command` and nothing else.
fn pushed(command: f32) -> Forcing {
    Forcing {
        push: command,
        ..Forcing::REST
    }
}

/// One step of a level pipe under one command, with the numbers the example starts with.
fn step(pipe: &mut Pipe, command: f32) {
    pipe.step(&Params::default(), &pushed(command), &Lay::level(LENGTH));
}

/// Runs a level pipe for `seconds` under one command.
fn run(pipe: &mut Pipe, command: f32, seconds: f32) {
    run_with(
        pipe,
        &Params::default(),
        &pushed(command),
        &Lay::level(LENGTH),
        seconds,
    );
}

/// Runs a pipe for `seconds` under whatever acts on it.
fn run_with(pipe: &mut Pipe, params: &Params, forcing: &Forcing, lay: &Lay, seconds: f32) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (seconds / STEP).round() as usize;
    for _ in 0..steps {
        pipe.step(params, forcing, lay);
    }
}

fn new_pipe(mechanism: Mechanism) -> Pipe {
    Pipe::new(mechanism, &Params::default(), LENGTH)
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

/// Over one step and over each stretch of eight segments: the largest mismatch between
/// what the stretch gained in contents and what flowed in at one end less what flowed out
/// at the other; and the largest gain, to judge it against.
fn continuity(pipe: &mut Pipe, command: f32) -> (f32, f32) {
    let before = (areas(pipe), flows(pipe));
    step(pipe, command);
    unbalanced(&before, &(areas(pipe), flows(pipe)))
}

/// The same for the cross-sections and flows of a pipe before a step and after it.
fn unbalanced(before: &(Vec<f32>, Vec<f32>), after: &(Vec<f32>, Vec<f32>)) -> (f32, f32) {
    let spacing = LENGTH / CELLS as f32;
    let held = |areas: &[f32], from: usize, to: usize| {
        let ends = 0.5 * (areas[from] + areas[to]);
        (areas[from + 1..to].iter().sum::<f32>() + ends) * spacing
    };

    let ((before, flow_before), (after, flow_after)) = (before, after);

    let mut worst = 0.0f32;
    let mut largest = 0.0f32;
    for from in (0..CELLS).step_by(4) {
        let to = (from + 8).min(CELLS);
        let gained = (held(after, from, to) - held(before, from, to)) / STEP;
        let flowed_in =
            0.5 * ((flow_before[from] - flow_before[to]) + (flow_after[from] - flow_after[to]));

        worst = worst.max((gained - flowed_in).abs());
        largest = largest.max(gained.abs());
    }
    (worst, largest)
}

#[test]
fn carried_contents_move_at_the_commanded_speed_and_come_back_when_it_reverses() {
    let speed = Params::default().speed;
    let mut pipe = new_pipe(Mechanism::Advection);

    // At rest the pipe is filled evenly and nothing moves.
    let marked = pipe.label_at(0.25);
    assert!((where_is(&pipe, marked).unwrap() - 0.25).abs() < 1e-3);
    run(&mut pipe, 0.0, 1.0);
    assert!((where_is(&pipe, marked).unwrap() - 0.25).abs() < 1e-3);

    // A full command carries the marked contents on by the speed times the time.
    run(&mut pipe, 1.0, 1.5);
    let carried = (where_is(&pipe, marked).unwrap() - 0.25) * LENGTH;
    assert!((carried - 1.5 * speed).abs() < 1.0, "carried {carried}");

    // Half the command, half the speed.
    run(&mut pipe, 0.5, 1.0);
    let carried = (where_is(&pipe, marked).unwrap() - 0.25) * LENGTH;
    assert!((carried - 2.0 * speed).abs() < 1.0, "carried {carried}");

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
    let rings = Params::default().peristalsis;
    let mut pipe = new_pipe(Mechanism::Peristalsis);

    // The wall is where the model put it: closed down to the throat at each contraction,
    // fully open between them.
    let (narrowest, widest) = areas(&pipe)
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), &area| {
            (low.min(area), high.max(area))
        });
    assert!((narrowest - rings.throat).abs() < 0.02, "{narrowest}");
    assert!((widest - 1.0).abs() < 1e-6);

    // The contractions travel at the commanded speed.
    let throat = |pipe: &Pipe| {
        let areas = areas(pipe);
        // Near the start, where there is one ring and no other.
        let point = (0..=CELLS / 8)
            .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
            .unwrap();
        share(point) * LENGTH
    };
    let from = throat(&pipe);
    run(&mut pipe, 1.0, 0.25);
    let moved = throat(&pipe) - from;
    assert!(
        (moved - 0.25 * rings.speed).abs() < LENGTH / CELLS as f32,
        "moved {moved}"
    );

    // What each stretch gains in cross-section is what flows in less what flows out, as
    // nearly as a ring thirteen points long can be added up.
    for _ in 0..40 {
        let (mismatch, largest) = continuity(&mut pipe, 1.0);
        assert!(largest > 10.0, "the wall is moving: {largest}");
        assert!(
            mismatch < 0.05 * largest,
            "mismatch {mismatch} against a gain of {largest}"
        );
    }

    // And more nearly for rings the points resolve better: it is the adding up that
    // is off, not the model.
    let mut long = Params::default();
    long.peristalsis.spacing = 200.0;
    long.peristalsis.length = 160.0;
    let mut slow = Pipe::new(Mechanism::Peristalsis, &long, LENGTH);
    slow.step(&long, &pushed(1.0), &Lay::level(LENGTH));
    for _ in 0..40 {
        let before = (areas(&slow), flows(&slow));
        slow.step(&long, &pushed(1.0), &Lay::level(LENGTH));
        let after = (areas(&slow), flows(&slow));
        let (mismatch, largest) = unbalanced(&before, &after);
        assert!(largest > 10.0, "the wall is moving: {largest}");
        assert!(
            mismatch < 0.005 * largest,
            "mismatch {mismatch} against a gain of {largest}"
        );
    }

    // The contents in a throat hold still; between throats they go the way the wall goes,
    // but never as fast as the wall.
    for point in 0..=CELLS {
        let (area, speed) = (pipe.area_at(share(point)), pipe.speed_at(share(point)));
        assert!(speed >= -1e-3 && speed < rings.speed);
        if area < rings.throat + 0.01 {
            assert!(speed < 0.05 * rings.speed, "in a throat: {speed}");
        }
        if area > 0.999 {
            let expected = rings.speed * (1.0 - rings.throat);
            assert!((speed - expected).abs() < 0.5, "between throats: {speed}");
        }
    }

    // Over a whole number of contractions the contents have moved on, and a negative
    // command moves them back.
    let marked = pipe.label_at(0.3);
    let period = rings.spacing / rings.speed;
    run(&mut pipe, 1.0, 2.0 * period);
    let forward = (where_is(&pipe, marked).unwrap() - 0.3) * LENGTH;
    assert!(
        forward > 0.3 * rings.spacing && forward < 2.0 * rings.spacing,
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
    let wave = Params::default().wave;
    let mut pipe = new_pipe(Mechanism::PressureWave);

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

    let mut pipe = new_pipe(Mechanism::PressureWave);
    let marked = pipe.label_at(0.02);

    run(&mut pipe, 1.0, 0.45);
    let early = front(&pipe).expect("the beat has widened the pipe");
    run(&mut pipe, 1.0, 0.6);
    let late = front(&pipe).unwrap();

    let wave_speed = (late - early) / 0.6;
    assert!(
        (wave_speed - wave.speed).abs() < 0.12 * wave.speed,
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
    let mut pipe = new_pipe(Mechanism::PressureWave);
    let mut expected = pipe.volume(LENGTH);
    for _ in 0..600 {
        let before = flows(&pipe);
        step(&mut pipe, 1.0);
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
    let mut pipe = new_pipe(Mechanism::PressureWave);
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
    let from_wall = Params::default().wave.speed * (now_as - 1.0);
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
    let params = Params::default();
    let balloon = &params.balloon;
    let (gives_way, gives_back) = balloon.limits();

    // The pressure in the soft stretch rises to a limit, falls as it gives way, and rises
    // again.
    assert!((balloon.pressure(gives_way) - 1.0).abs() < 1e-6);
    assert!(balloon.pressure(gives_way - 0.1) < 1.0);
    assert!(balloon.pressure(gives_way + 0.1) < 1.0);
    let valley = balloon.pressure(gives_back);
    assert!(valley < 0.8 && valley > 0.5, "valley {valley}");
    assert!(balloon.pressure(gives_back + 0.1) > valley);
    assert!(balloon.pressure(gives_back - 0.1) > valley);

    let below = balloon.threshold - 0.1;
    let above = balloon.threshold + 0.2;
    let width = |pipe: &Pipe| pipe.area_at(balloon.at);

    // Under the threshold the stretch widens a little and settles.
    let mut pipe = new_pipe(Mechanism::Balloon);
    run(&mut pipe, below, 12.0);
    let held = pipe.balloon();
    assert!(held > 0.2 && held < gives_way, "held at {held}");
    assert!(
        pipe.balloon_rate(&params, below).abs() < 1e-3,
        "and is settled"
    );
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
    assert!(pipe.balloon_rate(&params, below).abs() < 1e-3);

    // Only well under the threshold does it let down again.
    run(&mut pipe, valley * balloon.threshold - 0.1, 12.0);
    assert!(pipe.balloon() < gives_way, "let down to {}", pipe.balloon());

    // The width is where the model says, and only around the soft stretch.
    let mut pipe = new_pipe(Mechanism::Balloon);
    run(&mut pipe, above, 12.0);
    let expected = balloon.growth.mul_add(pipe.balloon(), 1.0);
    assert!((width(&pipe) - expected).abs() < 0.05);
    assert!((pipe.area_at(0.1) - 1.0).abs() < 1e-3);
    assert!((pipe.area_at(0.95) - 1.0).abs() < 1e-3);

    // While it fills, less leaves the pipe than enters it, by what the stretch takes up.
    let mut pipe = new_pipe(Mechanism::Balloon);
    run(&mut pipe, 1.0, 1.0);
    let held = pipe.volume(LENGTH);
    step(&mut pipe, 1.0);
    let taken_up = (pipe.volume(LENGTH) - held) / STEP;
    let flows = flows(&pipe);
    assert!(taken_up > 5.0, "taken up {taken_up}");
    assert!((flows[0] - params.speed).abs() < 1e-3);
    assert!(
        (flows[0] - flows[CELLS] - taken_up).abs() < 0.02 * taken_up,
        "in {}, out {}, taken up {taken_up}",
        flows[0],
        flows[CELLS]
    );

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

    let bourdon = Params::default().bourdon;
    let flex = |points: &[Pos2], straightening: f32| flow::flex(points, straightening, &bourdon);

    assert_eq!(flex(&arc, 0.0).len(), arc.len());
    for (flexed, &rest) in flex(&arc, 0.0).iter().zip(&arc) {
        assert!(flexed.distance(rest) < 1e-2, "nothing asked, nothing moved");
    }

    let mut curls = Vec::new();
    for straightening in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let flexed = flex(&arc, straightening);
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
    for (flexed, rest) in flex(&straight, 1.0).iter().zip(straight) {
        assert!(flexed.distance(rest) < 1e-3);
    }
    assert_eq!(flex(&arc, f32::NAN), arc);
    assert_eq!(flex(&arc[..2], 1.0), &arc[..2]);
    assert!(flex(&[], 1.0).is_empty());

    // The model: the pipe follows the command with a swing and settles on it.
    let mut pipe = new_pipe(Mechanism::Bourdon);
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

    fn show_header(
        &mut self,
        node: NodeId,
        inputs: &[InPin],
        outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<Node>,
    ) {
        self.0.show_header(node, inputs, outputs, ui, snarl);
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

/// Whether `pos` lies between the walls of `tube`, give or take `slack`.
fn within(tube: &CableGeometry, pos: Pos2, slack: f32) -> bool {
    if !tube.bounds().expand(slack).contains(pos) {
        return false;
    }
    let hit = tube.closest(pos);
    hit.distance <= tube.radius_at(hit.share) + slack
}

/// Every shape of a pass, in the order they are painted.
fn painted(output: &FullOutput) -> Vec<Shape> {
    fn take_apart(shape: &Shape, shapes: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(group) => {
                for shape in group {
                    take_apart(shape, shapes);
                }
            }
            Shape::Noop => {}
            shape => shapes.push(shape.clone()),
        }
    }

    let mut shapes = Vec::new();
    for clipped in &output.shapes {
        take_apart(&clipped.shape, &mut shapes);
    }
    shapes
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
        world.viewer.drives.signal.command = if moment < 4 { 1.0 } else { -1.0 };
        let output = world.run(0.9);

        let anatomy = world.anatomy();
        let to_screen = anatomy.transform.to_global;
        let found = meshes(&output);
        assert_eq!(
            found.len(),
            (1 + pipe::BANDS) * wires.len(),
            "a wall and the bands of its contents to a pipe"
        );

        // Nothing is painted outside the walls the pipes are picked between: not the
        // wall, and not the contents.
        let tubes: Vec<CableGeometry> = wires
            .iter()
            .map(|&(pins, _)| anatomy.wire(pins).unwrap().geometry.transformed(to_screen))
            .collect();
        for mesh in &found {
            for vertex in &mesh.vertices {
                assert!(
                    tubes.iter().any(|tube| within(tube, vertex.pos, 0.15)),
                    "moment {moment}: painted at {:?}, outside every pipe",
                    vertex.pos
                );
            }
        }

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
                let expected = world.viewer.style.radius() * pipe.area_at(share).sqrt();
                // The pipe has a point every three units and the model one every three
                // and a bit; between them each runs straight, so they differ a little
                // where the wall is steep.
                assert!(
                    (tube.radius_at(share) - expected).abs() < 0.15,
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

            let widest = (0..=200)
                .map(|step| {
                    #[allow(clippy::cast_precision_loss)]
                    let share = 0.1 + 0.8 * step as f32 / 200.0;
                    share
                })
                .max_by(|&a, &b| on_screen.radius_at(a).total_cmp(&on_screen.radius_at(b)))
                .unwrap();
            let narrowest = (0..=200)
                .map(|step| {
                    #[allow(clippy::cast_precision_loss)]
                    let share = 0.1 + 0.8 * step as f32 / 200.0;
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
    let squeeze = (1.0 / world.viewer.params.peristalsis.throat).sqrt();
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
    world.viewer.drives.signal.command = 1.0;
    let _ = world.run(5.0);
    let balloon_at = world.viewer.params.balloon.at;

    let (pins, _) = world.wires()[2];
    let anatomy = world.anatomy();
    let tube = anatomy
        .wire(pins)
        .unwrap()
        .geometry
        .transformed(anatomy.transform.to_global);

    // Deep in the balloon, far from the centreline.
    let anchor = tube.anchor_at(balloon_at);
    let wall = tube.radius_at(balloon_at);
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
    plain.viewer.drives.signal.command = 1.0;
    plain.viewer.drives.gravity.strength = 1.0;
    plain.viewer.style.ends = Ends::Fittings;
    let with_viewer = plain.run(1.0);
    assert!(meshes(&with_viewer).is_empty());

    // The style of a plain graph sets how nodes look and nothing about wires: none of
    // what a graph has to ask for is asked for.
    let style = graph::style(Look::Plain);
    assert_eq!(style, graph::canvas());
    assert_eq!(
        (
            style.wire_picking,
            style.nodes_cover_wires,
            style.wire_smoothness_on_screen,
            style.output_rows_clear_pins
        ),
        (None, None, None, None)
    );

    // The same graph, shown by a viewer that has no wire hooks at all, with that style.
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
                .style(graph::canvas())
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

#[test]
fn the_numbers_of_a_model_shape_the_pipe_it_makes() {
    // Peristalsis: how far apart the rings are and how long each is.
    let rings = |params: &Params| {
        let areas = areas(&Pipe::new(Mechanism::Peristalsis, params, LENGTH));
        let count = areas
            .windows(2)
            .filter(|pair| pair[0] >= 0.999 && pair[1] < 0.999)
            .count();
        #[allow(clippy::cast_precision_loss)]
        let open = areas.iter().filter(|&&area| area >= 0.999).count() as f32 / areas.len() as f32;
        (count, open)
    };

    let mut params = Params::default();
    let (count, open) = rings(&params);
    assert!(
        (7..=8).contains(&count),
        "{count} rings at the spacing it starts with"
    );
    let expected = 1.0 - params.peristalsis.length / params.peristalsis.spacing;
    assert!(
        (open - expected).abs() < 0.08,
        "open over {open} of the pipe"
    );

    params.peristalsis.spacing = 160.0;
    params.peristalsis.length = 120.0;
    let (count, open) = rings(&params);
    assert_eq!(count, 4, "rings twice as far apart");
    assert!((open - 0.25).abs() < 0.08, "long rings leave {open} open");

    params.peristalsis.throat = 0.6;
    let narrowest = areas(&Pipe::new(Mechanism::Peristalsis, &params, LENGTH))
        .into_iter()
        .fold(f32::MAX, f32::min);
    assert!(
        (narrowest - 0.6).abs() < 0.02,
        "a wider throat: {narrowest}"
    );

    // Ballooning: where the soft stretch is and how it is shaped.
    let ballooned = |params: &Params| {
        let mut pipe = Pipe::new(Mechanism::Balloon, params, LENGTH);
        run_with(&mut pipe, params, &pushed(1.0), &Lay::level(LENGTH), 8.0);
        pipe
    };
    // How much of the widening in the middle is left at a distance from it, in reaches.
    let left = |pipe: &Pipe, params: &Params, reaches: f32| {
        let balloon = &params.balloon;
        let peak = pipe.area_at(balloon.at) - 1.0;
        (pipe.area_at(balloon.at + reaches * balloon.reach / LENGTH) - 1.0) / peak
    };

    let mut params = Params::default();
    params.balloon.at = 0.25;
    params.balloon.roundness = 2.0;
    let bell = ballooned(&params);
    let widest = (0..=CELLS)
        .max_by(|&a, &b| bell.area_at(share(a)).total_cmp(&bell.area_at(share(b))))
        .unwrap();
    assert!(
        (share(widest) - 0.25).abs() < 0.01,
        "it balloons where it is put"
    );
    assert!((left(&bell, &params, 0.6) - 0.835).abs() < 0.03, "a bell");
    assert!((left(&bell, &params, 1.5) - 0.325).abs() < 0.03);

    params.balloon.roundness = 8.0;
    let sausage = ballooned(&params);
    assert!(
        left(&sausage, &params, 0.6) > 0.97,
        "flat across the middle"
    );
    assert!(
        left(&sausage, &params, 1.5) < 0.01,
        "and done by its shoulders"
    );

    // However it is shaped, a full push fills it to the same pressure, and what flows
    // past it is still what came in less what it takes up.
    assert!((bell.balloon() - sausage.balloon()).abs() < 0.02);
    let mut filling = Pipe::new(Mechanism::Balloon, &params, LENGTH);
    run_with(
        &mut filling,
        &params,
        &pushed(1.0),
        &Lay::level(LENGTH),
        1.0,
    );
    let held = filling.volume(LENGTH);
    filling.step(&params, &pushed(1.0), &Lay::level(LENGTH));
    let taken_up = (filling.volume(LENGTH) - held) / STEP;
    let flows = flows(&filling);
    assert!((flows[0] - flows[CELLS] - taken_up).abs() < 0.02 * taken_up);

    // A reach that is doubled holds about twice as much for the same fill.
    let mut wide = params.clone();
    wide.balloon.reach *= 2.0;
    let wide = ballooned(&wide);
    let gain = (wide.volume(LENGTH) - LENGTH) / (sausage.volume(LENGTH) - LENGTH);
    assert!((gain - 2.0).abs() < 0.1, "holds {gain} times as much");

    // Numbers no slider would give still leave a pipe that can be drawn.
    let odd = Params {
        speed: f32::NAN,
        full_drop: 0.0,
        sag: -1.0,
        peristalsis: flow::Peristalsis {
            speed: 110.0,
            spacing: 0.0,
            length: f32::INFINITY,
            throat: -3.0,
        },
        wave: flow::Wave {
            speed: 0.0,
            friction: f32::NAN,
            rate: f32::NAN,
            share: 0.0,
            ..flow::Wave::default()
        },
        balloon: flow::Balloon {
            reach: 0.0,
            roundness: f32::NAN,
            gives_way: 3.0,
            gives_back: 1.0,
            ..flow::Balloon::default()
        },
        bourdon: flow::Bourdon {
            rate: f32::NAN,
            damping: f32::NAN,
            uncurl: 9.0,
            ..flow::Bourdon::default()
        },
    };
    assert!(odd.full_head() > 0.0 && odd.tension() > 0.0);
    for mechanism in [
        Mechanism::Peristalsis,
        Mechanism::PressureWave,
        Mechanism::Balloon,
        Mechanism::Bourdon,
    ] {
        let params = Params {
            speed: 140.0,
            ..odd.clone()
        };
        let mut pipe = Pipe::new(mechanism, &params, LENGTH);
        run_with(&mut pipe, &params, &pushed(1.0), &Lay::level(LENGTH), 2.0);
        assert!(
            areas(&pipe)
                .iter()
                .all(|area| area.is_finite() && *area >= 0.2 && *area < 50.0),
            "{mechanism:?}"
        );
        assert!(pipe.straightening().is_finite());
    }
}

#[test]
fn the_colour_of_the_contents_runs_evenly_and_survives_a_shift_of_labels() {
    let pigments = pipe::pigments(Mechanism::Advection);
    let apart = |a: [f32; 3], b: [f32; 3]| {
        (0..3)
            .map(|channel| (a[channel] - b[channel]).abs())
            .fold(0.0, f32::max)
    };

    let mut style = pipe::Style::default();
    let largest_step = |style: &pipe::Style| {
        (0..2048)
            .map(|label| {
                #[allow(clippy::cast_precision_loss)]
                let label = label as f32 * 0.5;
                apart(
                    pipe::colour(&pigments, label, style),
                    pipe::colour(&pigments, label + 0.5, style),
                )
            })
            .fold(0.0, f32::max)
    };

    // Blended all along, no two neighbours are far apart: there is no edge to see.
    assert!(largest_step(&style) < 0.012, "{}", largest_step(&style));

    // Every pigment is reached, in turn, a band apart.
    for (index, pigment) in pigments.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let label = index as f32 * style.band;
        assert!(apart(pipe::colour(&pigments, label, &style), *pigment) < 1e-3);
    }

    // Half way between two, the colour is not darker than both, as it would be if
    // the numbers on the screen were blended instead of the light.
    let between = pipe::colour(&pigments, 0.5 * style.band, &style);
    let straight = (pigments[0][1] + pigments[1][1]) / 2.0;
    assert!(between[1] > straight, "{} against {straight}", between[1]);

    // Labels are shifted by whole repeats now and then. The colour must not notice,
    // whatever length a colour is given.
    for band in [128.0, 100.0, 37.0, 500.0] {
        style.band = band;
        for label in [0.0, 13.0, 777.5, -41.25] {
            assert!(
                apart(
                    pipe::colour(&pigments, label, &style),
                    pipe::colour(&pigments, label + flow::LABEL_REPEAT, &style)
                ) < 2e-3,
                "band {band}, label {label}"
            );
        }
    }

    // With little blending the colours sit side by side as blocks.
    style.band = 128.0;
    style.blend = 0.05;
    assert!(largest_step(&style) > 0.05);
}

/// Whether two pipes are in the same state, to within what adding a head up loses.
fn alike(a: &Pipe, b: &Pipe) -> bool {
    (0..=CELLS).all(|point| {
        let share = share(point);
        (a.area_at(share) - b.area_at(share)).abs() < 1e-3
            && (a.speed_at(share) - b.speed_at(share)).abs() < 0.05
            && (a.label_at(share) - b.label_at(share)).abs() < 0.05
    }) && (a.balloon() - b.balloon()).abs() < 1e-3
        && (a.straightening() - b.straightening()).abs() < 1e-3
}

/// An influence of the test's own: a wind that blows on every pipe alike.
struct Wind(Vec2);

impl Influence for Wind {
    fn name(&self) -> &'static str {
        "wind"
    }

    fn act(&self, _lay: &Lay, forcing: &mut Forcing) {
        forcing.body += self.0;
    }
}

#[test]
fn a_pipe_is_driven_by_a_signal_by_physics_or_by_both() {
    let params = Params::default();
    let full_head = params.full_head();

    // A pipe that drops by as much as pushes like a full command, and one that rises.
    let down = Lay {
        wire: None,
        from: pos2(100.0, 40.0),
        to: pos2(500.0, 40.0 + params.full_drop),
        length: 420.0,
    };
    let up = Lay {
        to: pos2(500.0, 40.0 - params.full_drop),
        ..down
    };
    let level = Lay {
        to: pos2(500.0, 40.0),
        ..down
    };

    let gravity = Gravity {
        strength: 1.0,
        angle: 0.0,
    };
    assert!((gravity.pull() - vec2(0.0, FREE_FALL)).length() < 1e-2);
    let sideways = Gravity {
        strength: 0.5,
        angle: 90.0,
    };
    assert!((sideways.pull() - vec2(-0.5 * FREE_FALL, 0.0)).length() < 1e-2);

    // Along the pipe an even acceleration is a head between the ends; across it, a load.
    let falling = Forcing {
        body: gravity.pull(),
        ..Forcing::REST
    };
    assert!((falling.head(&down) - full_head).abs() < 1e-3 * full_head);
    assert!((falling.head(&up) + full_head).abs() < 1e-3 * full_head);
    assert!(falling.head(&level).abs() < 1e-3);
    assert!((falling.load(&level) - gravity.pull()).length() < 1e-2);
    let across = falling.load(&down);
    assert!(across.dot(down.to - down.from).abs() < 1e-2 * FREE_FALL);
    assert!(across.length() < FREE_FALL && across.length() > 0.9 * FREE_FALL);

    // The pump and the head add up, the valve lets a share of both through, and nothing
    // pushes harder than a full command.
    let drive = |forcing: Forcing, lay: &Lay| forcing.drive(lay, full_head);
    assert!((drive(falling, &down) - 1.0).abs() < 1e-3);
    assert!((drive(falling, &up) + 1.0).abs() < 1e-3);
    assert!(drive(falling, &level).abs() < 1e-6);
    let half = Forcing {
        body: 0.5 * gravity.pull(),
        ..Forcing::REST
    };
    assert!((drive(half, &down) - 0.5).abs() < 1e-3);
    assert!((drive(Forcing { push: -0.2, ..half }, &down) - 0.3).abs() < 1e-3);
    assert!(
        (drive(
            Forcing {
                push: 0.3,
                opening: 0.5,
                ..half
            },
            &down
        ) - 0.4)
            .abs()
            < 1e-3
    );
    assert!(
        (drive(
            Forcing {
                push: 0.9,
                ..falling
            },
            &down
        ) - 1.0)
            .abs()
            < 1e-6
    );
    assert_eq!(drive(Forcing::REST, &down).to_bits(), 0.0f32.to_bits());

    // The signal alone, as the example starts.
    let mut drives = Drives::default();
    assert_eq!(drives.active(), ["signal as pump"]);
    drives.signal.command = 0.7;
    assert!((drive(drives.forcing(&down), &down) - 0.7).abs() < 1e-6);
    assert!((drive(drives.forcing(&up), &up) - 0.7).abs() < 1e-6);

    // Gravity alone: no signal, and the contents run downhill.
    drives.signal.command = 0.0;
    drives.gravity.strength = 0.5;
    assert_eq!(drives.active(), ["signal as pump", "gravity"]);
    assert!((drive(drives.forcing(&down), &down) - 0.5).abs() < 1e-3);
    assert!((drive(drives.forcing(&up), &up) + 0.5).abs() < 1e-3);

    // Both: the pump works with gravity one way and against it the other.
    drives.signal.command = 0.25;
    assert!((drive(drives.forcing(&down), &down) - 0.75).abs() < 1e-3);
    assert!((drive(drives.forcing(&up), &up) + 0.25).abs() < 1e-3);

    // The signal as a valve: it pushes nothing itself and lets gravity through.
    drives.signal = Signal {
        command: -0.5,
        role: Role::Valve,
    };
    assert_eq!(drives.active(), ["signal as valve", "gravity"]);
    assert!((drive(drives.forcing(&down), &down) - 0.25).abs() < 1e-3);
    assert!(drive(drives.forcing(&level), &level).abs() < 1e-6);
    drives.signal.command = 0.0;
    assert!(drive(drives.forcing(&down), &down).abs() < 1e-6);

    // Something else is plugged in without a model or a drive being touched.
    drives.signal = Signal {
        command: 0.0,
        role: Role::Pump,
    };
    drives.gravity.strength = 0.0;
    drives
        .more
        .push(Box::new(Wind(vec2(0.25 * FREE_FALL, 0.0))));
    assert_eq!(drives.active(), ["signal as pump", "wind"]);
    let blown = drives.forcing(&level);
    assert!((blown.body - vec2(0.25 * FREE_FALL, 0.0)).length() < 1e-2);
    let expected = 0.25 * (level.to.x - level.from.x) / params.full_drop;
    assert!((drive(blown, &level) - expected.min(1.0)).abs() < 1e-3);

    // The models take what they are given. Carried contents run downhill under gravity
    // alone at the speed a pump of the same strength would give them, uphill never.
    let carried = |forcing: &Forcing, lay: &Lay| {
        let mut pipe = Pipe::new(Mechanism::Advection, &params, lay.length);
        let marked = pipe.label_at(0.5);
        run_with(&mut pipe, &params, forcing, lay, 1.0);
        assert!((pipe.drive() - forcing.drive(lay, full_head)).abs() < 1e-6);

        let now_at = (0..CELLS)
            .find_map(|point| {
                let (here, next) = (pipe.label_at(share(point)), pipe.label_at(share(point + 1)));
                #[allow(clippy::cast_precision_loss)]
                (here >= marked && marked > next)
                    .then(|| share(point) + (here - marked) / (here - next) / CELLS as f32)
            })
            .unwrap();
        (now_at - 0.5) * lay.length
    };
    assert!((carried(&half, &down) - 0.5 * params.speed).abs() < 1.0);
    assert!((carried(&half, &up) + 0.5 * params.speed).abs() < 1.0);
    assert!(carried(&half, &level).abs() < 1e-3);
    assert!((carried(&pushed(0.5), &level) - 0.5 * params.speed).abs() < 1.0);

    // Every mechanism is driven the same way: under gravity alone each does downhill
    // what a pump makes it do, and nothing on the level.
    for mechanism in Mechanism::ALL {
        let mut downhill = Pipe::new(mechanism, &params, down.length);
        let mut pumped = downhill.clone();
        let mut on_the_level = downhill.clone();
        run_with(&mut downhill, &params, &falling, &down, 3.0);
        run_with(&mut pumped, &params, &pushed(1.0), &level, 3.0);
        run_with(&mut on_the_level, &params, &falling, &level, 3.0);

        assert!(alike(&downhill, &pumped), "{mechanism:?}");
        assert!(
            alike(&on_the_level, &Pipe::new(mechanism, &params, down.length)),
            "{mechanism:?}"
        );
        if mechanism != Mechanism::Advection {
            assert!(!alike(&downhill, &on_the_level), "{mechanism:?}");
        }
    }
}

#[test]
fn a_path_under_a_load_hangs_between_ends_that_stay_put() {
    let path = CableGeometry::line(pos2(0.0, 0.0), pos2(400.0, 0.0)).subdivided(4.0);
    let load = vec2(0.0, 1000.0);
    let tension = 2.0e6;

    // An even weight hangs in a parabola: deepest in the middle, by the load times the
    // length squared over eight times the tension.
    let hung = pipe::hang(&path, |_| 1.0, load, tension);
    assert_eq!(hung.len(), path.points().len());
    assert_eq!(hung[0], path.start());
    assert_eq!(hung[hung.len() - 1], path.end());

    let deepest = 1000.0 * 400.0 * 400.0 / (8.0 * tension);
    for (&now, &rest) in hung.iter().zip(path.points()) {
        let along = rest.x / 400.0;
        let expected = 4.0 * deepest * along * (1.0 - along);
        assert!(
            (now.x - rest.x).abs() < 1e-3,
            "it hangs the way the load points"
        );
        assert!(
            (now.y - expected).abs() < 0.02,
            "{} against {expected}",
            now.y
        );
    }

    // Twice the weight, or half the tension, twice the sag.
    let heavy = pipe::hang(&path, |_| 2.0, load, tension);
    let slack = pipe::hang(&path, |_| 1.0, load, 0.5 * tension);
    let middle = hung.len() / 2;
    assert!((heavy[middle].y - 2.0 * hung[middle].y).abs() < 0.02);
    assert!((slack[middle].y - 2.0 * hung[middle].y).abs() < 0.02);

    // Where the path is heavier it hangs lower: the lowest point moves toward a weight
    // that sits to one side.
    let lopsided = pipe::hang(
        &path,
        |share| {
            if (0.65..0.85).contains(&share) {
                6.0
            } else {
                1.0
            }
        },
        load,
        tension,
    );
    let lowest = lopsided.iter().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    assert!(
        lowest.x > 240.0 && lowest.x < 340.0,
        "lowest at {}",
        lowest.x
    );
    assert!(lowest.y > 1.5 * hung[middle].y);
    assert_eq!(lopsided[lopsided.len() - 1], path.end());

    // A load from another side is hung toward, and none leaves the path where it was.
    let sideways = pipe::hang(&path, |_| 1.0, vec2(0.0, -1000.0), tension);
    assert!((sideways[middle].y + hung[middle].y).abs() < 1e-3);
    assert_eq!(
        pipe::hang(&path, |_| 1.0, Vec2::ZERO, tension),
        path.points()
    );
    assert_eq!(pipe::hang(&path, |_| 1.0, load, 0.0), path.points());
    assert_eq!(
        pipe::hang(&path, |_| 1.0, vec2(f32::NAN, 0.0), tension),
        path.points()
    );
}

#[test]
fn under_gravity_a_pipe_runs_downhill_and_is_picked_where_it_hangs() {
    let mut world = World::new(Look::Pipes, Assign::All(Mechanism::Advection));
    world.viewer.drives.signal.command = 0.0;
    let _ = world.run(0.5);
    let wires = world.wires();

    let middles = |world: &World| -> Vec<Pos2> {
        let anatomy = world.anatomy();
        wires
            .iter()
            .map(|&(pins, _)| anatomy.wire(pins).unwrap().geometry.anchor_at(0.5).pos)
            .collect()
    };
    let level = middles(&world);

    // Nothing pushes, so nothing moves.
    for &(pins, _) in &wires {
        assert_eq!(
            world.viewer.pipe(pins).unwrap().speed_at(0.5).to_bits(),
            0.0f32.to_bits()
        );
    }

    world.viewer.drives.gravity.strength = 1.0;
    let _ = world.run(0.5);
    let hung = middles(&world);
    let anatomy = world.anatomy();
    let to_screen = anatomy.transform.to_global;
    let params = world.viewer.params.clone();

    let mut downhill = 0;
    for (index, &(pins, _)) in wires.iter().enumerate() {
        let measured = anatomy.wire(pins).unwrap();
        let tube = &measured.geometry;
        let (from, to) = (
            anatomy.out_pin(pins.out_pin).unwrap().graph_center,
            anatomy.in_pin(pins.in_pin).unwrap().graph_center,
        );

        // Both ends are still on their pins.
        assert_eq!((tube.start(), tube.end()), (from, to));

        // The pipe hangs, by what a string of its weight hangs by.
        let sag = hung[index].y - level[index].y;
        let expected = 1.35 * params.sag * (tube.length() / 500.0).powi(2);
        assert!(
            (sag - expected).abs() < 0.15 * expected,
            "pipe {index} hangs {sag}, a string would hang {expected}"
        );

        // Its contents run toward the lower end, as fast as the drop says.
        let drop = (to.y - from.y) / params.full_drop;
        let pipe = world.viewer.pipe(pins).unwrap();
        assert!((pipe.drive() - drop).abs() < 0.01, "pipe {index}");
        assert!((pipe.speed_at(0.5) - drop * params.speed).abs() < 1.5);
        if drop > 0.1 {
            downhill += 1;
        }

        // It is picked where it hangs now, and not where it ran before.
        let on_screen = tube.transformed(to_screen);
        let anchor = on_screen.anchor_at(0.5);
        world.point_at(anchor.pos + anchor.normal * (on_screen.radius_at(0.5) - 1.0));
        assert_eq!(
            world.anatomy().hovered_wire().map(|wire| wire.pins),
            Some(pins),
            "pipe {index}: where it hangs"
        );

        world.point_at(to_screen * level[index]);
        assert_ne!(
            world.anatomy().hovered_wire().map(|wire| wire.pins),
            Some(pins),
            "pipe {index}: where it used to run"
        );
    }
    assert!(downhill >= 2, "some pipes of the graph run downhill");

    // The signal as a valve lets through a share of that, and shut, none.
    world.viewer.drives.signal = Signal {
        command: 0.5,
        role: Role::Valve,
    };
    let _ = world.run(0.2);
    let anatomy = world.anatomy();
    for &(pins, _) in &wires {
        let (from, to) = (
            anatomy.out_pin(pins.out_pin).unwrap().graph_center,
            anatomy.in_pin(pins.in_pin).unwrap().graph_center,
        );
        let drop = (to.y - from.y) / params.full_drop;
        assert!((world.viewer.pipe(pins).unwrap().drive() - 0.5 * drop).abs() < 0.01);
    }
}

#[test]
fn a_fitting_sits_in_front_of_the_node_and_is_picked_as_it_is_drawn() {
    let mut world = World::new(Look::Pipes, Assign::Each);
    world.viewer.style.ends = Ends::Fittings;
    world.viewer.drives.signal.command = 1.0;
    let output = world.run(1.0);
    let wires = world.wires();

    let anatomy = world.anatomy();
    let to_screen = anatomy.transform.to_global;
    let zoom = anatomy.transform.scale();
    let radius = world.viewer.style.radius();

    let tubes: Vec<CableGeometry> = wires
        .iter()
        .map(|&(pins, _)| anatomy.wire(pins).unwrap().geometry.transformed(to_screen))
        .collect();

    // A pipe is a wall and its contents, and each of its two fittings is as many bands.
    let shapes = painted(&output);
    let found: Vec<usize> = shapes
        .iter()
        .enumerate()
        .filter_map(|(index, shape)| matches!(shape, Shape::Mesh(_)).then_some(index))
        .collect();
    let body = (1 + pipe::BANDS) * wires.len();
    assert_eq!(found.len(), body + 2 * pipe::BANDS * wires.len());

    // The pipes are painted behind the nodes and their fittings in front of them.
    let frames: Vec<usize> = anatomy
        .nodes
        .iter()
        .map(|node| {
            shapes
                .iter()
                .position(|shape| {
                    matches!(shape, Shape::Rect(rect)
                        if rect.rect.min.distance(node.screen_rect.min) < 0.5
                            && rect.rect.max.distance(node.screen_rect.max) < 0.5)
                })
                .expect("the frame of every node was painted")
        })
        .collect();
    let (first_frame, last_frame) = (*frames.iter().min().unwrap(), *frames.iter().max().unwrap());
    assert!(found[..body].iter().all(|&mesh| mesh < first_frame));
    assert!(found[body..].iter().all(|&mesh| mesh > last_frame));

    // Everything painted, fittings included, lies between the walls that are picked.
    for &index in &found {
        let Shape::Mesh(mesh) = &shapes[index] else {
            unreachable!()
        };
        for vertex in &mesh.vertices {
            assert!(
                tubes.iter().any(|tube| within(tube, vertex.pos, 0.15)),
                "painted at {:?}, outside every pipe",
                vertex.pos
            );
        }
    }

    for (&(pins, mechanism), tube) in wires.iter().zip(&tubes) {
        let measured = anatomy.wire(pins).unwrap();
        let (reach, scale) = world
            .viewer
            .style
            .fitting(measured.geometry.length())
            .expect("the pipe has fittings");
        assert!(
            (scale - 1.0).abs() < 1e-6,
            "{mechanism:?}: a pipe this long has room"
        );

        for from_end in [false, true] {
            // In the nut of the fitting the pipe is as wide as the nut, whatever the
            // model is doing under it.
            let share = 9.0 * scale / measured.geometry.length();
            let share = if from_end { 1.0 - share } else { share };
            let wall = tube.radius_at(share);
            assert!(
                (wall - 1.4 * radius * zoom).abs() < 0.02 * wall,
                "{mechanism:?}: {wall} on screen"
            );

            // The pointer is on the wire on the fitting, far from the centreline, and
            // off it beyond the fitting.
            let anchor = tube.anchor_at(share);
            for side in [-1.0, 1.0] {
                world.point_at(anchor.pos + anchor.normal * (side * (wall - 1.5)));
                assert_eq!(
                    world.anatomy().hovered_wire().map(|wire| wire.pins),
                    Some(pins),
                    "{mechanism:?}: on the fitting"
                );
            }
            let band = measured.hit_tolerance * zoom;
            world.point_at(anchor.pos + anchor.normal * (wall.max(band) + 2.0));
            assert_ne!(
                world.anatomy().hovered_wire().map(|wire| wire.pins),
                Some(pins),
                "{mechanism:?}: beside the fitting"
            );
        }

        // Past the fitting the pipe is the model's again.
        let past = (reach + 6.0) / measured.geometry.length();
        let pipe = world.viewer.pipe(pins).unwrap();
        assert!(
            (measured.geometry.radius_at(past) - radius * pipe.area_at(past).sqrt()).abs() < 0.3,
            "{mechanism:?}"
        );
    }

    // A fitting only paints. The pin under it still takes the pointer: a new pipe is
    // pulled from the pin under one fitting to the pin under another.
    let from = to_screen * anatomy.out_pin(wires[0].0.out_pin).unwrap().graph_center;
    let to = to_screen * anatomy.in_pin(wires[1].0.in_pin).unwrap().graph_center;
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    world.point_at(from);
    let _ = world.pass(0.0, vec![button(from, true)]);
    for step in 1..=6 {
        #[allow(clippy::cast_precision_loss)]
        let pos = from.lerp(to, step as f32 / 6.0);
        let _ = world.pass(0.0, vec![Event::PointerMoved(pos)]);
    }
    let _ = world.pass(0.0, Vec::new());
    let _ = world.pass(0.0, vec![button(to, false)]);
    let _ = world.pass(0.0, Vec::new());
    assert_eq!(world.snarl.wires().count(), wires.len() + 1);
    assert!(
        world
            .snarl
            .wires()
            .any(|(out_pin, in_pin)| out_pin == wires[0].0.out_pin && in_pin == wires[1].0.in_pin)
    );

    // Without fittings nothing is painted in front of the nodes.
    world.viewer.style.ends = Ends::Behind;
    let shapes = painted(&world.run(0.1));
    let last_frame = shapes
        .iter()
        .rposition(|shape| matches!(shape, Shape::Rect(_)))
        .unwrap();
    assert!(
        shapes[last_frame..]
            .iter()
            .all(|shape| !matches!(shape, Shape::Mesh(_)))
    );
}
