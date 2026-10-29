//! ragdoll - an interactive sandbox for the sphere-spring creature.
//!
//! Three layers meet here, and the split between them is the design:
//!
//!   solver   `petz-core::ragdoll` - springs, gravity, grab. Always running.
//!   gait     `petz-core::gait`    - where the feet want to be, and the IK that
//!                                   gets them there.
//!   look     `petz-core::shade` / `fur` - light and material per pixel.
//!
//! The body does not walk by physics. Feet are kinematic - planted where the
//! gait says - and the spine and knees are *driven* toward a pose with springs
//! that are deliberately not stiff, so the solver still supplies the wobble.
//! The tail and ears have no target at all, so they swing freely.
//!
//! Grab a ball and the whole drive is cancelled: the animal goes limp in your
//! hand and the ragdoll takes over. Let go and it picks the walk back up.
//!
//! Keys: drag = grab, W = walk/stop, + / - = speed, arrows = move light,
//! A/S = ambience, [ ] = ramp steps, T = texture, P = print settings,
//! R = reset, Space = pause, Esc = quit.

use std::collections::HashMap;
use std::time::Instant;

use petz_core::fur::{dapple, shade_disc_pattern, stripes, Pattern, Texture};
use petz_core::gait::{two_bone_ik, Gait, GaitSpec};
use petz_core::ragdoll::{Particle, Ragdoll, Vec2};
use petz_core::project::{Body, Projected, Vec3 as WVec3, View};
use petz_core::shade::{Light, Ramp, Vec3};
use petz_core::{fill_disc, Palette, SpanTable, Surface};
use std::fs;
use std::path::PathBuf;

const W: usize = 640;
const H: usize = 480;
const BACKGROUND: u8 = 130;

const OUTLINE: u8 = 89;
const BODY: u8 = 61;
const LIGHT: u8 = 10;
const DARK: u8 = 0;
/// Retail `;L eye` colour: white sclera.
const SCLERA: u8 = 245;
/// Retail `;L iris` colour: black pupil, with the iris ring drawn as its
/// outline colour. Teal is the Dalmatian's; any breed colour works.
const IRIS: u8 = 244;
const IRIS_RING: u8 = 6;
/// Ears are a darker shade than the coat, as they are in every breed file.
const EAR: u8 = 95;

/// Where the floor is. Feet stand on this line.
const GROUND: f32 = 218.0;

/// Centre of the body in the skeleton's own forward coordinates. The bones are
/// authored at absolute x, so this is subtracted before placing them in the
/// world - otherwise the animal stands a body-length behind its own position.
const BODY_ORIGIN: f32 = 366.0;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_table() -> SpanTable {
    SpanTable::new(
        fs::read(repo_root().join("evidence/ball/tables.raw"))
            .expect("evidence/ball/tables.raw"),
    )
}

fn load_palette() -> Palette {
    let raw = fs::read(repo_root().join("evidence/ball/ball-input.raw"))
        .expect("evidence/ball/ball-input.raw");
    let stride = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as usize;
    let height = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let header = raw.len() - (1024 + 512 + 2 * stride * height);
    Palette::from_rgbquad(&raw[header..header + 1024])
}

/// One leg, by particle index.
struct Leg {
    hip: usize,
    knee: usize,
    foot: usize,
    /// Which way the knee juts out from the hip-foot line. A dog's front legs
    /// bend backward and its hind legs forward.
    bend: f32,
    /// Rest position of the foot, so a stopped animal stands still.
    rest: Vec2,
}

/// The skeleton, described by indices into the ragdoll.
struct Creature {
    spine: Vec<usize>,
    legs: Vec<Leg>,
    head: Vec<usize>,
    tail: Vec<usize>,
    /// (base, overlay) pairs drawn as one unit: the iris sits on its sclera.
    eye_overlays: Vec<(usize, usize)>,
    /// The skull, used as the reference the eyes are tested against.
    skull: usize,
    /// (sclera, lid) for each eye. The lid is not a retail ball - Petz
    /// parameterises it - but a lid ball is the cheapest way to draw one.
    eye_lids: Vec<(usize, usize)>,
}

fn build() -> (Ragdoll, Creature) {
    let mut r = Ragdoll::new();
    r.bounds = Some((-4000.0, 8.0, 4000.0, 460.0));
    r.gravity = 1400.0;
    r.damping = 0.985;
    r.iterations = 16;
    r.contact = 0.35;

    // Everything below is in *body* coordinates: x is forward, y is down from
    // the body's own frame (so `GROUND` is the floor), and `side` is lateral.
    //
    // Four balls, not five: hips, two along the back, and the neck at the
    // front. The neck is a spine ball rather than a separate link, because an
    // extra ball there put the head a body-length away from the shoulders and
    // the head read as detached.
    let spacing = 28.0;
    let spine_r = [24.0, 23.0, 22.0, 20.0];
    let mut spine = Vec::new();
    for i in 0..spine_r.len() {
        let fwd = 276.0 + i as f32 * spacing;
        spine.push(r.add(Particle::new(fwd, GROUND - 68.0, spine_r[i], BODY)));
    }
    for i in 0..spine.len() - 1 {
        r.link(spine[i], spine[i + 1]);
    }
    for i in 0..spine.len().saturating_sub(2) {
        let rest = r.particles[spine[i]].pos.sub(r.particles[spine[i + 2]].pos).length();
        r.constraints.push(petz_core::Constraint::soft(spine[i], spine[i + 2], rest, 0.7));
    }
    {
        let last = *spine.last().unwrap();
        let rest = r.particles[spine[0]].pos.sub(r.particles[last].pos).length();
        r.constraints.push(petz_core::Constraint::soft(spine[0], last, rest, 0.5));
    }

    // Head, hanging directly off the neck ball.
    let base_y = GROUND - 68.0;
    let neck = *spine.last().unwrap();
    let skull = r.add(Particle::new(386.0, base_y - 26.0, 26.0, BODY));
    let muzzle = r.add(Particle::new(406.0, base_y - 16.0, 13.0, LIGHT));
    let nose = r.add(Particle::new(416.0, base_y - 21.0, 5.0, DARK));
    r.link(neck, skull);
    r.link(skull, muzzle);
    r.link(muzzle, nose);
    // A second brace, so the head cannot nod away from the shoulders.
    r.link(skull, spine[spine.len() - 2]);

    // Eyes. The retail data is two balls per eye: a white sclera and a black
    // pupil whose *outline* is the iris, in a per-breed colour. There is no
    // eyelid ball at all - Petz parameterises the lid instead.
    let mut eyes = Vec::new();
    let mut overlay_pairs: Vec<(usize, usize)> = Vec::new();
    let mut lid_pairs: Vec<(usize, usize)> = Vec::new();
    for side in [-12.0f32, 12.0f32] {
        let sclera = r.add(Particle::new(394.0, base_y - 34.0, 10.0, SCLERA));
        let iris = r.add(Particle::new(394.0, base_y - 33.0, 6.0, IRIS));
        for i in [sclera, iris] {
            r.particles[i].side = side;
            r.particles[i].rest_side = side;
        }
        r.particles[iris].rim = IRIS_RING;
        r.particles[iris].layer = 1;
        // A lid ball, in coat colour, that drops over the eye to blink.
        let lid = r.add(Particle::new(394.0, base_y - 58.0, 11.0, BODY));
        r.particles[lid].side = side;
        r.particles[lid].rest_side = side;

        r.link(skull, sclera);
        // The pupil and the lid are children of the eye, not of the skull.
        // Unlinked they were free particles: any grab cleared their drive and
        // they simply fell out of the head.
        r.link(sclera, iris);
        r.link(sclera, lid);
        eyes.push(sclera);
        eyes.push(iris);
        eyes.push(lid);
        overlay_pairs.push((sclera, iris));
        overlay_pairs.push((sclera, lid));
        lid_pairs.push((sclera, lid));
    }

    // Ears removed for now. They sit on top of the skull and were landing
    // nearer the viewer than the eye in profile, so they get rebuilt once the
    // eyes are right - not before, or we cannot tell what is covering what.

    // Eyes. The retail data is two balls per eye: a white sclera and a black
    // pupil whose *outline* is the iris, in a per-breed colour. There is no
    // eyelid ball at all - Petz parameterises the lid instead.
    let mut eyes = Vec::new();
    let mut overlay_pairs: Vec<(usize, usize)> = Vec::new();
    let mut lid_pairs: Vec<(usize, usize)> = Vec::new();
    for side in [-12.0f32, 12.0f32] {
        let sclera = r.add(Particle::new(394.0, base_y - 34.0, 10.0, SCLERA));
        let iris = r.add(Particle::new(394.0, base_y - 33.0, 6.0, IRIS));
        for i in [sclera, iris] {
            r.particles[i].side = side;
            r.particles[i].rest_side = side;
        }
        r.particles[iris].rim = IRIS_RING;
        r.particles[iris].layer = 1;
        // A lid ball, in coat colour, that drops over the eye to blink.
        let lid = r.add(Particle::new(394.0, base_y - 58.0, 11.0, BODY));
        r.particles[lid].side = side;
        r.particles[lid].rest_side = side;

        r.link(skull, sclera);
        eyes.push(sclera);
        eyes.push(iris);
        eyes.push(lid);
        overlay_pairs.push((sclera, iris));
        overlay_pairs.push((sclera, lid));
        lid_pairs.push((sclera, lid));
    }

    // Ears: three balls a side, in a darker shade, as the breed files have
    // them. The pair of brown balls that used to sit here read as something
    // else entirely - eyelids, by the look of it.
    let mut ears = Vec::new();
    for side in [-1.0f32, 1.0] {
        // On top of the skull and slightly behind it. They used to sit back at
        // x=372, level with the eyes, where in profile they were nearer the
        // viewer than the eye and covered it.
        let a = r.add(Particle::new(378.0, base_y - 58.0, 9.0, EAR));
        let b = r.add(Particle::new(370.0, base_y - 68.0, 7.5, EAR));
        let c = r.add(Particle::new(386.0, base_y - 70.0, 7.0, EAR));
        for i in [a, b, c] {
            r.particles[i].side = 10.0 * side;
            r.particles[i].rest_side = 10.0 * side;
        }
        r.link(skull, a);
        r.link(a, b);
        r.link(a, c);
        ears.extend([a, b, c]);
    }

    // Legs. The front pair hangs off the ball *behind* the neck - the shoulder -
    // and the hind pair off the hips. Getting this the wrong way round is what
    // made the front legs look like they were attached to the head.
    let legs_spec: [(f32, f32, f32); 4] = [
        (332.0, 9.0, 1.0),   // left front
        (332.0, -9.0, 1.0),  // right front
        (276.0, 9.0, -1.0),  // left hind
        (276.0, -9.0, -1.0), // right hind
    ];
    let mut legs = Vec::new();
    for (fwd, side, bend) in legs_spec {
        let hip = r.add(Particle::new(fwd, GROUND - 48.0, 15.0, BODY));
        let knee = r.add(Particle::new(fwd, GROUND - 22.0, 12.0, BODY));
        let foot = r.add(Particle::new(fwd, GROUND, 10.0, LIGHT));
        for i in [hip, knee, foot] {
            r.particles[i].side = side;
            r.particles[i].rest_side = side;
        }
        let attach = if fwd > 300.0 { spine[spine.len() - 2] } else { spine[0] };
        r.link(attach, hip);
        r.link(hip, knee);
        r.link(knee, foot);
        legs.push(Leg { hip, knee, foot, bend, rest: Vec2::new(fwd, GROUND) });
    }

    // Tail: no target, so it swings with the body.
    let mut tail = Vec::new();
    let mut tail_prev = spine[0];
    for i in 0..4 {
        let t = r.add(Particle::new(252.0 - i as f32 * 15.0, base_y - 10.0, 10.0 - i as f32 * 1.2, BODY));
        r.link(tail_prev, t);
        tail_prev = t;
        tail.push(t);
    }

    // Ears sit either side of the skull.

    let mut head = vec![skull, muzzle, nose];
    head.extend(eyes);
    (r, Creature { spine, legs, head, tail, skull, eye_overlays: overlay_pairs, eye_lids: lid_pairs })
}

/// The walking controller: owns the clock, the world position, and the pose.
struct Walker {
    gait: Gait,
    /// Where the animal is in the world and which way it faces. `heading` is
    /// what the player asked for; `heading_body` is where the body has actually
    /// got to, because a real animal cannot pivot instantly.
    body: Body,
    heading_body: f32,
    /// Per-ball heading, lagging the body by a part-specific time constant.
    /// This is what makes a turn travel down the animal instead of teleporting
    /// the whole skeleton to a new orientation.
    facing: Vec<f32>,
    lag: Vec<f32>,
    walking: bool,
    /// Blink clock, in seconds, cycling through a short close-and-open.
    clock: f32,
    /// 1.0 is a fully open eye, 0.0 fully closed.
    lid_open: f32,
}

/// Fastest the animal's body can turn, in radians per second.
const MAX_TURN_RATE: f32 = 2.4;

impl Walker {
    fn new(creature: &Creature, particles: usize) -> Self {
        let body = Body { pos: WVec3::new(320.0, -520.0, 0.0), heading: 0.42 };
        // Lag by part. The head leads, the body follows, the tail trails - the
        // same overlapping action an animator would key by hand.
        let mut lag = vec![0.28; particles];
        for &i in &creature.spine {
            lag[i] = 0.34;
        }
        for (n, &i) in creature.spine.iter().enumerate() {
            // Front of the spine answers sooner than the back of it.
            lag[i] = 0.42 - 0.06 * n as f32;
        }
        for &i in &creature.head {
            lag[i] = 0.11;
        }
        for leg in &creature.legs {
            lag[leg.hip] = 0.24;
            lag[leg.knee] = 0.30;
            lag[leg.foot] = 0.36;
        }
        for (n, &i) in creature.tail.iter().enumerate() {
            lag[i] = 0.50 + 0.08 * n as f32;
        }
        Self {
            gait: Gait::walk(GaitSpec::default()),
            body,
            heading_body: body.heading,
            facing: vec![body.heading; particles],
            lag,
            walking: true,
            clock: 0.0,
            lid_open: 1.0,
        }
    }

    /// Ease every ball's facing toward where the body has got to.
    fn turn(&mut self, dt: f32) {
        let delta = wrap_pi(self.body.heading - self.heading_body);
        let max = MAX_TURN_RATE * dt;
        self.heading_body = wrap_pi(self.heading_body + delta.clamp(-max, max));

        for i in 0..self.facing.len() {
            let want = wrap_pi(self.heading_body - self.facing[i]);
            let lag = self.lag.get(i).copied().unwrap_or(0.25).max(1e-3);
            let k = (dt / lag).min(1.0);
            self.facing[i] = wrap_pi(self.facing[i] + want * k);
        }
    }

    /// The body transform for one ball, using that ball's own facing.
    fn part(&self, i: usize) -> Body {
        Body { pos: self.body.pos, heading: self.facing.get(i).copied().unwrap_or(self.heading_body) }
    }

    fn advance(&mut self, dt: f32) {
        // Turning happens whether or not the animal is moving - a dog can look
        // round while standing.
        self.turn(dt);

        // A blink every few seconds. Cheap, and it proves the lid is real.
        self.clock += dt;
        if self.clock > 3.6 {
            self.clock = 0.0;
        }
        self.lid_open = if self.clock < 0.09 {
            self.clock / 0.09
        } else if self.clock < 0.18 {
            (0.18 - self.clock) / 0.09
        } else {
            1.0
        };
        if !self.walking {
            return;
        }
        self.gait.advance(dt);
        let f = Body { pos: self.body.pos, heading: self.heading_body }.forward();
        self.body.pos.x += f.x * self.gait.spec.speed * dt;
        self.body.pos.y += f.y * self.gait.spec.speed * dt;
    }

    /// Drive the skeleton toward the pose the gait describes.
    fn pose(&self, doll: &mut Ragdoll, creature: &Creature, rest: &Pose) {
        // A small vertical bob, twice per cycle: the body rises as the legs
        // pass under it.
        let bob = (self.gait.phase * std::f32::consts::TAU * 2.0).sin() * 2.0;
        let lift = if self.walking { bob } else { 0.0 };

        let n = creature.spine.len().max(1);
        for (i, &p) in creature.spine.iter().enumerate() {
            let t = i as f32 / (n - 1).max(1) as f32;
            let (dv, dl) = if self.walking { self.gait.spine_flex(t) } else { (0.0, 0.0) };
            // dv is upward, and body y runs down, so it subtracts.
            doll.drive_to(p, Vec2::new(rest.spine[i].x, rest.spine[i].y + lift - dv), 0.45);
            // Lateral flex is authored rather than solved: the spine's sway is
            // a curve through `side`, which the solver does not model.
            if let Some(part) = doll.particles.get_mut(p) {
                part.side = dl;
            }
        }
        for (i, &p) in creature.head.iter().enumerate() {
            doll.drive_to(p, Vec2::new(rest.head[i].x, rest.head[i].y + lift), 0.4);
        }

        // Drop the lid over the eye as it closes. Open, it sits clear above the
        // eye; closed, it is centred on it.
        for &(sclera, lid) in &creature.eye_lids {
            let k = creature.head.iter().position(|&h| h == sclera);
            let Some(k) = k else { continue };
            let base = rest.head[k];
            let r = doll.particles[sclera].radius;
            let y = base.y + lift - self.lid_open * (r * 2.0 + 3.0);
            doll.drive_to(lid, Vec2::new(base.x, y), 0.8);
        }

        for (i, leg) in creature.legs.iter().enumerate() {
            let foot = if self.walking {
                let f = self.gait.foot(i);
                Vec2::new(leg.rest.x + f.dx, GROUND + f.dy)
            } else {
                Vec2::new(leg.rest.x, GROUND)
            };

            let hip_target = Vec2::new(rest.legs[i].x, rest.legs[i].y + lift);
            // Bone lengths are the rest distances, so IK matches the skeleton.
            let upper = rest.hip_to_knee[i];
            let lower = rest.knee_to_foot[i];
            let knee = two_bone_ik(hip_target, foot, upper, lower, leg.bend);

            // The foot is exact; the hip and knee lag slightly, which is what
            // keeps the legs from looking like rigid rods.
            doll.drive_to(leg.foot, foot, 1.0);
            doll.drive_to(leg.knee, knee, 0.85);
            doll.drive_to(leg.hip, hip_target, 0.6);
        }
    }
}

/// Bone lengths measured once from the rest pose.
struct Pose {
    spine: Vec<Vec2>,
    head: Vec<Vec2>,
    legs: Vec<Vec2>,
    hip_to_knee: Vec<f32>,
    knee_to_foot: Vec<f32>,
}

fn measure(doll: &Ragdoll, c: &Creature) -> Pose {
    let spine = c.spine.iter().map(|&i| doll.particles[i].pos).collect();
    let head = c.head.iter().map(|&i| doll.particles[i].pos).collect();
    let legs: Vec<Vec2> = c.legs.iter().map(|l| doll.particles[l.hip].pos).collect();
    let hip_to_knee = c
        .legs
        .iter()
        .map(|l| doll.particles[l.hip].pos.sub(doll.particles[l.knee].pos).length())
        .collect();
    let knee_to_foot = c
        .legs
        .iter()
        .map(|l| doll.particles[l.knee].pos.sub(doll.particles[l.foot].pos).length())
        .collect();
    Pose { spine, head, legs, hip_to_knee, knee_to_foot }
}

/// Shortest signed difference between two angles, in (-pi, pi].
fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn rotate_light(dir: Vec3, degrees: f32) -> Vec3 {
    let yaw = dir.x.atan2(dir.z).to_degrees() + degrees;
    let pitch = dir.y.asin().to_degrees();
    Vec3::from_angles(yaw, pitch)
}

fn elevate_light(dir: Vec3, degrees: f32) -> Vec3 {
    let yaw = dir.x.atan2(dir.z).to_degrees();
    let pitch = (dir.y.asin().to_degrees() + degrees).clamp(-80.0, 80.0);
    Vec3::from_angles(yaw, pitch)
}

fn report(light: Light, steps: usize, tex: usize, texture: &str) {
    let yaw = light.dir.x.atan2(light.dir.z).to_degrees();
    let pitch = light.dir.y.asin().to_degrees();
    println!(
        "settings: steps={steps} yaw={yaw:.1} pitch={pitch:.1} ambient={:.2} diffuse={:.2} tex={tex} ({texture})",
        light.ambient, light.diffuse
    );
}

fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let display = video.display_bounds(0).map_err(|e| e.to_string())?;
    let scale = (display.width() as i32 / W as i32)
        .min(display.height() as i32 / H as i32)
        .max(1) as usize;

    let window = video
        .window("petz ragdoll sandbox", (W * scale) as u32, (H * scale) as u32)
        .position_centered()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;
    let tc = canvas.texture_creator();
    let mut tex = tc
        .create_texture_streaming(sdl2::pixels::PixelFormatEnum::RGB24, W as u32, H as u32)
        .map_err(|e| e.to_string())?;
    tex.set_scale_mode(sdl2::render::ScaleMode::Nearest);

    let table = load_table();
    let palette = load_palette();

    let mut steps = 6usize;
    let mut ramps: HashMap<u8, Ramp> = HashMap::new();
    let mut rebuild_ramps = |steps: usize, ramps: &mut HashMap<u8, Ramp>| {
        ramps.clear();
        for base in [BODY, LIGHT, DARK] {
            ramps.insert(base, Ramp::from_palette(&palette, base, steps));
        }
    };
    rebuild_ramps(steps, &mut ramps);
    let mut light = Light::default();

    let textures: Vec<(&str, Texture)> = vec![
        ("plain", Texture::flat(64, 64, 255)),
        ("dapple", dapple(64, 64, 17, 255)),
        ("stripes", stripes(64, 64, 5, 255)),
    ];
    let mut tex_idx = 0usize;

    let (mut doll, creature) = build();
    let rest = measure(&doll, &creature);
    let mut walker = Walker::new(&creature, doll.particles.len());
    let mut grabbed: Option<usize> = None;
    let mut grab_point = Vec2::new(0.0, 0.0);
    let mut grab_home = Vec2::new(0.0, 0.0);
    let mut grab_side = 0.0f32;
    let mut view = View::new(25.0, GROUND);
    // Projected positions from the last frame; the picker works in screen space.
    let mut screen: Vec<Projected> = Vec::new();

    let mut sweep_t = 0.0f32;
    let mut sweep_last = Instant::now();
    let mut frame_no = 0u32;
    let mut events = sdl.event_pump()?;
    let mut paused = false;
    let step = 1.0 / 120.0;
    let mut acc = 0.0f64;
    let mut last = Instant::now();

    println!(
        "drag: left mouse   W: walk/stop   +/-: speed   Q/E: turn   V: view pitch\n\
         light: arrows   ambience: A/S   steps: [ ]   texture: T   flex: , .   P: settings\n\
         R: reset   Space: pause   Esc: quit"
    );
    report(light, steps, tex_idx, textures[tex_idx].0);

    'outer: loop {
        for e in events.poll_iter() {
            match e {
                sdl2::event::Event::Quit { .. } => break 'outer,
                sdl2::event::Event::KeyDown { keycode: Some(k), .. } => match k {
                    sdl2::keyboard::Keycode::Escape => break 'outer,
                    sdl2::keyboard::Keycode::R => {
                        // Rebuilding resets indices as well as pose, so the
                        // walker is rebuilt to match the new skeleton.
                        let (d, c) = build();
                        doll = d;
                        walker = Walker::new(&c, doll.particles.len());
                        grabbed = None;
                    }
                    sdl2::keyboard::Keycode::Space => paused = !paused,
                    sdl2::keyboard::Keycode::W => walker.walking = !walker.walking,
                    sdl2::keyboard::Keycode::Comma => {
                        walker.gait.spec.flex_vertical = (walker.gait.spec.flex_vertical - 0.5).max(0.0);
                        walker.gait.spec.flex_lateral = (walker.gait.spec.flex_lateral - 0.5).max(0.0);
                        println!(
                            "flex v{:.1} l{:.1}",
                            walker.gait.spec.flex_vertical, walker.gait.spec.flex_lateral
                        );
                    }
                    sdl2::keyboard::Keycode::Period => {
                        walker.gait.spec.flex_vertical = (walker.gait.spec.flex_vertical + 0.5).min(16.0);
                        walker.gait.spec.flex_lateral = (walker.gait.spec.flex_lateral + 0.5).min(16.0);
                        println!(
                            "flex v{:.1} l{:.1}",
                            walker.gait.spec.flex_vertical, walker.gait.spec.flex_lateral
                        );
                    }
                    sdl2::keyboard::Keycode::Q => {
                        walker.body.heading -= 0.26;
                        println!("heading {:.0} deg", walker.body.heading.to_degrees());
                    }
                    sdl2::keyboard::Keycode::E => {
                        walker.body.heading += 0.26;
                        println!("heading {:.0} deg", walker.body.heading.to_degrees());
                    }
                    sdl2::keyboard::Keycode::V => {
                        let degrees = [0.0f32, 15.0, 25.0, 35.0];
                        let now = view.pitch.to_degrees();
                        let next = degrees.iter().find(|d| **d > now + 0.5).copied().unwrap_or(0.0);
                        view.pitch = next.to_radians();
                        println!("view pitch {next:.0} deg");
                    }
                    sdl2::keyboard::Keycode::Equals | sdl2::keyboard::Keycode::Plus => {
                        walker.gait.spec.speed += 15.0;
                        println!("speed {:.0} px/s", walker.gait.spec.speed);
                    }
                    sdl2::keyboard::Keycode::Minus => {
                        walker.gait.spec.speed = (walker.gait.spec.speed - 15.0).max(0.0);
                        println!("speed {:.0} px/s", walker.gait.spec.speed);
                    }
                    sdl2::keyboard::Keycode::Left => {
                        light.dir = rotate_light(light.dir, -12.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::Right => {
                        light.dir = rotate_light(light.dir, 12.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::Up => {
                        light.dir = elevate_light(light.dir, 8.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::Down => {
                        light.dir = elevate_light(light.dir, -8.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::LeftBracket => {
                        steps = (steps - 1).max(1);
                        rebuild_ramps(steps, &mut ramps);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::RightBracket => {
                        steps = (steps + 1).min(24);
                        rebuild_ramps(steps, &mut ramps);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::T => {
                        tex_idx = (tex_idx + 1) % textures.len();
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::P => report(light, steps, tex_idx, textures[tex_idx].0),
                    sdl2::keyboard::Keycode::A => {
                        light.ambient = (light.ambient - 0.05).max(0.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::S => {
                        light.ambient = (light.ambient + 0.05).min(0.9);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    _ => {}
                },
                sdl2::event::Event::MouseButtonDown {
                    x, y, mouse_btn: sdl2::mouse::MouseButton::Left, ..
                } => {
                    // Pick in screen space: with a projection there is no single
                    // world point under the cursor.
                    let (mx, my) = (x as f32 / scale as f32, y as f32 / scale as f32);
                    let mut best: Option<(usize, f32)> = None;
                    for (i, p) in screen.iter().enumerate() {
                        let d = ((p.x - mx).powi(2) + (p.y - my).powi(2)).sqrt();
                        if d < 30.0 && best.map_or(true, |(_, bd)| d < bd) {
                            best = Some((i, d));
                        }
                    }
                    if let Some((i, _)) = best {
                        // Grabbing hands the animal to physics: the gait stops
                        // driving it and it goes limp.
                        doll.release_all();
                        doll.particles[i].pinned = true;
                        grab_point = Vec2::new(mx, my);
                        grab_home = doll.particles[i].pos;
                        grab_side = doll.particles[i].side;
                        grabbed = Some(i);
                        println!("grab ball {i}");
                    } else {
                        println!("grab: nothing within reach");
                    }
                }
                sdl2::event::Event::MouseButtonUp { mouse_btn: sdl2::mouse::MouseButton::Left, .. } => {
                    if let Some(i) = grabbed.take() {
                        println!("release ball {i}");
                        doll.particles[i].pinned = false;
                        // Side is authored, not solved: undo any lateral drag.
                        doll.restore_sides();

                        // The grab moved the skeleton *within* body space, and
                        // the pose drive pulls body space back to rest - which
                        // is why letting go rewound the animal to where the
                        // grab began. Re-centre body space on the rest pose and
                        // carry the world position with it.
                        let n = creature.spine.len().max(1) as f32;
                        let mut cur = Vec2::new(0.0, 0.0);
                        let mut rst = Vec2::new(0.0, 0.0);
                        for (k, &idx) in creature.spine.iter().enumerate() {
                            cur.x += doll.particles[idx].pos.x;
                            cur.y += doll.particles[idx].pos.y;
                            rst.x += rest.spine[k].x;
                            rst.y += rest.spine[k].y;
                        }
                        let shift = Vec2::new((cur.x - rst.x) / n, (cur.y - rst.y) / n);
                        for p in doll.particles.iter_mut() {
                            p.pos.x -= shift.x;
                            p.pos.y -= shift.y;
                            p.prev.x -= shift.x;
                            p.prev.y -= shift.y;
                        }
                        // Horizontally, move the origin by the same distance so
                        // the picture does not jump. Vertically we let it fall,
                        // which is what dropping something should look like.
                        let f = walker.body.forward();
                        walker.body.pos.x += shift.x * f.x;
                        walker.body.pos.y += shift.x * f.y;
                    }
                }
                sdl2::event::Event::MouseMotion { x, y, .. } => {
                    if let Some(i) = grabbed {
                        // Drag by *delta*, never by absolute position. An
                        // absolute unprojection depends on a depth that goes
                        // stale, and a wrong depth throws the ball hundreds of
                        // pixels - which the springs then convert into an
                        // explosion.
                        let mouse = Vec2::new(x as f32 / scale as f32, y as f32 / scale as f32);
                        let (dx, dy) = (mouse.x - grab_point.x, mouse.y - grab_point.y);

                        // Screen x maps onto whichever body axis points across
                        // the view; screen y is mostly height, compressed by
                        // the view pitch.
                        let f = walker.body.forward();
                        let r = walker.body.right();
                        let mut fwd = dx * f.x;
                        let mut lat = dx * r.x;
                        let mut up = -dy / view.pitch.cos().max(0.35);

                        // Keep a single yank from outrunning the springs.
                        const LIMIT: f32 = 90.0;
                        let len = (fwd * fwd + lat * lat + up * up).sqrt();
                        if len > LIMIT {
                            let k = LIMIT / len;
                            fwd *= k;
                            lat *= k;
                            up *= k;
                        }

                        if let Some(p) = doll.particles.get_mut(i) {
                            p.pos = Vec2::new(grab_home.x + fwd, grab_home.y - up);
                            p.prev = p.pos;
                            p.side = grab_side + lat;
                        }
                    }
                }
                _ => {}
            }
        }

        let now = Instant::now();
        acc += (now - last).as_secs_f64().min(0.1);
        last = now;
        if !paused {
            while acc >= step {
                if grabbed.is_none() {
                    walker.advance(step as f32);
                    walker.pose(&mut doll, &creature, &rest);
                }
                doll.step(step as f32);
                acc -= step;
            }
            // Turn around at the edges rather than teleporting. Wrapping put
            // the animal half off-screen for stretches at a time, and turning
            // is something the animal can actually do - the lag we built for
            // Q/E plays it out.
            {
                let f = walker.body.forward();
                let p = walker.body.pos;
                let at_edge = (p.x > 540.0 && f.x > 0.0)
                    || (p.x < 70.0 && f.x < 0.0)
                    || (p.y > 120.0 && f.y > 0.0)
                    || (p.y < -470.0 && f.y < 0.0);
                if at_edge {
                    walker.body.heading += std::f32::consts::PI;
                }
            }
        }

        // Place every ball in the world and project it. The z we get back is
        // the draw order: there is no z-buffer, only a painter.
        screen.clear();
        for (i, p) in doll.particles.iter().enumerate() {
            let world = walker
                .part(i)
                .to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y);
            screen.push(view.project(world));
        }

        let mut surface = Surface::new(W, H);
        for y in 0..H {
            for x in 0..W {
                surface.set(x, y, BACKGROUND);
            }
        }

        // Depth order, nothing else. Painting on top unconditionally is how
        // the eyes ended up showing through the skull when the animal walked
        // away: the far eye is behind the head, and depth has to decide that.
        let overlays_for = |i: usize| -> Vec<usize> {
            creature
                .eye_overlays
                .iter()
                .filter(|(b, _)| *b == i)
                .map(|(_, o)| *o)
                .collect()
        };
        let is_overlay = |i: usize| creature.eye_overlays.iter().any(|(_, o)| *o == i);

        // An eye is visible when it sits nearer to the viewer than the centre
        // of the head. Testing "which side of the body" instead was wrong: it
        // hid *both* eyes when the animal faced the camera, where both should
        // be visible, and showed one even when it faced away.
        let mut hidden: Vec<bool> = vec![false; doll.particles.len()];
        let eye_h = walker.heading_body;
        for &(sclera, _) in &creature.eye_lids {
            // An eye is visible while it is on the viewer's side of the head.
            // This has to be the horizontal offset only: comparing projected
            // depth folds in the eye's height above the skull centre, which
            // biased the test and pulled the far eye out about thirty degrees
            // early.
            let (eye, head) = (&doll.particles[sclera], &doll.particles[creature.skull]);
            let dfwd = eye.pos.x - head.pos.x;
            let dlat = eye.side - head.side;
            // World y is "away from the viewer"; positive means behind the head.
            let away = dfwd * eye_h.cos() - dlat * eye_h.sin();
            if away >= 0.0 {
                hidden[sclera] = true;
                for &(base, over) in &creature.eye_overlays {
                    if base == sclera {
                        hidden[over] = true;
                    }
                }
            }
        }

        let mut order: Vec<usize> = (0..doll.particles.len()).collect();
        order.sort_by(|&a, &b| screen[b].depth.partial_cmp(&screen[a].depth).unwrap());

        // Rims for the body only. An iris rim drawn here would be painted over
        // by its own sclera, so it is drawn with the pair instead.
        for &i in order.iter().filter(|&&i| !is_overlay(i) && !hidden[i]) {
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = (screen[i].x.round() as i32, screen[i].y.round() as i32);
            let rim = if p.rim == 0 { OUTLINE } else { p.rim };
            fill_disc(&mut surface, &table, cx, cy, (((r * 2) + 4) as usize).min(255), rim);
        }

        let mut draw = |i: usize, surface: &mut Surface| {
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { return; }
            let (cx, cy) = (screen[i].x.round() as i32, screen[i].y.round() as i32);
            let alt = match p.colour {
                BODY => LIGHT,
                LIGHT => BODY,
                other => other,
            };
            let pattern = Pattern {
                base: ramps.get(&p.colour).cloned().unwrap_or_else(|| Ramp::new(vec![p.colour])),
                alt: ramps.get(&alt).cloned().unwrap_or_else(|| Ramp::new(vec![alt])),
                texture: textures[tex_idx].1.clone(),
                threshold: 128,
                phase: 0.0,
            };
            shade_disc_pattern(surface, &table, cx, cy, (r * 2) as usize, light, &pattern);
        };

        for &i in order.iter().filter(|&&i| !is_overlay(i) && !hidden[i]) {
            draw(i, &mut surface);
            // The pair is atomic: rim, body, then the pupil on top of it. It
            // still sorts by the sclera's depth, so the head can hide it.
            for over in overlays_for(i) {
                if hidden[over] {
                    continue;
                }
                let p = &doll.particles[over];
                let r = p.radius.round() as i32;
                if r >= 1 {
                    let (cx, cy) =
                        (screen[over].x.round() as i32, screen[over].y.round() as i32);
                    let rim = if p.rim == 0 { OUTLINE } else { p.rim };
                    fill_disc(&mut surface, &table, cx, cy, (((r * 2) + 4) as usize).min(255), rim);
                }
                draw(over, &mut surface);
            }
        }

        if std::env::var("PETZ_SWEEP").is_ok() {
            sweep_t += (now - sweep_last).as_secs_f32().min(0.1);
            sweep_last = now;
            walker.walking = false;
            walker.body.heading = sweep_t * 0.35; // 20 deg per second
            frame_no += 1;
            if frame_no % 5 == 0 {
                let iris = surface.pixels().iter().filter(|c| **c == IRIS_RING).count();
                let pupil = surface.pixels().iter().filter(|c| **c == IRIS).count();
                println!(
                    "sweep heading={:.0} facing={:.0} iris={} pupil={}",
                    walker.body.heading.to_degrees() % 360.0,
                    walker.heading_body.to_degrees(),
                    iris,
                    pupil
                );
            }
        }

        let rgb = palette.to_rgb(&surface);
        tex.update(None, &rgb, W * 3).map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&tex, None, None).map_err(|e| e.to_string())?;
        canvas.present();
    }
    Ok(())
}

#[cfg(test)]
mod walk_tests {
    use super::*;

    fn rig() -> (Ragdoll, Creature, Pose, Walker) {
        let (doll, c) = build();
        let rest = measure(&doll, &c);
        let w = Walker::new(&c, doll.particles.len());
        (doll, c, rest, w)
    }

    /// Run the walk loop exactly as `main` does, without a window.
    fn run(seconds: f32) -> (Ragdoll, Creature, Walker) {
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let steps = (seconds / dt) as usize;
        for _ in 0..steps {
            w.advance(dt);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
        }
        (doll, c, w)
    }

    #[test]
    fn walking_moves_the_animal_at_the_gait_speed() {
        let (d0, c0, _r, w0) = rig();
        let start = w0.body.pos;
        let _ = (d0, c0);
        let (_, _, w) = run(2.0);
        let travelled = w.body.pos.sub(start).length();
        let expected = w.gait.spec.speed * 2.0;
        assert!(
            (travelled - expected).abs() < 1.0,
            "travelled {travelled} px, expected {expected}"
        );
    }

    #[test]
    fn walking_away_takes_the_animal_up_the_screen() {
        let view = View::new(25.0, GROUND);
        let (doll, c, w) = run(2.0);
        let midpoint = |d: &Ragdoll| {
            let p = &d.particles[c.spine[2]];
            view.project(w.body.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y))
        };
        let now = midpoint(&doll);
        let (d0, _, _, w0) = rig();
        let p0 = &d0.particles[c.spine[2]];
        let start = View::new(25.0, GROUND).project(
            w0.body.to_world(p0.pos.x - BODY_ORIGIN, p0.side, GROUND - p0.pos.y),
        );
        assert!(
            now.y < start.y - 10.0,
            "walking away did not move up the screen: {:.1} -> {:.1}",
            start.y,
            now.y
        );
        let _ = w;
    }

    #[test]
    fn the_body_turn_is_rate_limited() {
        let (_, _, _, mut w) = rig();
        let before = w.heading_body;
        w.body.heading = before + 1.2; // ask for a hard turn
        w.advance(1.0 / 120.0);
        let moved = (w.heading_body - before).abs();
        assert!(
            moved <= MAX_TURN_RATE / 120.0 + 1e-4,
            "body turned {moved} rad in one step, limit is {}",
            MAX_TURN_RATE / 120.0
        );
        assert!(moved > 0.0, "the body ignored the turn entirely");
    }

    #[test]
    fn a_turn_reaches_the_head_before_the_tail() {
        let (_, c, _, mut w) = rig();
        let target = w.heading_body + 1.0;
        w.body.heading = target;
        for _ in 0..12 {
            w.advance(1.0 / 120.0);
        }
        let head_err = wrap_pi(target - w.facing[c.head[1]]).abs();
        let tail_err = wrap_pi(target - w.facing[*c.tail.last().unwrap()]).abs();
        assert!(
            head_err < tail_err,
            "head lagged the tail: head {head_err:.3}, tail {tail_err:.3}"
        );
        // ...and the whole animal must not have arrived at once.
        assert!(tail_err > 0.05, "the tail snapped instantly: {tail_err:.3}");
    }

    #[test]
    fn a_turn_propagates_and_then_settles() {
        let (_, c, _, mut w) = rig();
        let target = w.heading_body - 0.9;
        w.body.heading = target;
        for _ in 0..900 {
            w.advance(1.0 / 120.0);
        }
        // The head is close to the command almost immediately, the tail last.
        let head_err = wrap_pi(target - w.facing[c.head[1]]).abs();
        let tail_err = wrap_pi(target - w.facing[*c.tail.last().unwrap()]).abs();
        assert!(head_err < 0.05, "head never arrived: {head_err:.3}");
        assert!(tail_err < 0.05, "tail never arrived: {tail_err:.3}");
    }

    #[test]
    fn parts_hold_different_facings_during_a_turn() {
        // The whole point: mid-turn the animal is not rigid.
        let (_, c, _, mut w) = rig();
        w.body.heading = w.heading_body + 1.2;
        for _ in 0..20 {
            w.advance(1.0 / 120.0);
        }
        let head = w.facing[c.head[1]];
        let tail = w.facing[*c.tail.last().unwrap()];
        let spread = wrap_pi(head - tail).abs();
        assert!(spread > 0.08, "the body is still rigid: {spread:.3} rad between head and tail");
    }

    #[test]
    fn the_spine_bends_while_walking() {
        // Fit a line through the spine and measure how far its points sit off
        // it. A rigid spine keeps them on the line; a flexing one does not.
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let mut worst: f32 = 0.0;
        for _ in 0..240 {
            w.advance(dt);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
            let pts: Vec<(f32, f32)> = c
                .spine
                .iter()
                .map(|&i| (doll.particles[i].pos.x, doll.particles[i].pos.y))
                .collect();
            let (x0, y0) = pts[0];
            let (x1, y1) = pts[pts.len() - 1];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            for &(px, py) in &pts[1..pts.len() - 1] {
                // Perpendicular distance from the root-to-shoulder line.
                let d = ((px - x0) * dy - (py - y0) * dx).abs() / len;
                worst = worst.max(d);
            }
        }
        assert!(worst > 0.4, "spine stayed straight; max deviation {worst:.2} px");
    }

    #[test]
    fn nothing_floats_after_a_grab() {
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        for _ in 0..120 {
            w.advance(dt);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
        }
        // Grab the middle of the back and yank it up and forward.
        let i = c.spine[2];
        doll.release_all();
        doll.particles[i].pinned = true;
        let home = doll.particles[i].pos;
        let side0 = doll.particles[i].side;
        for k in 0..90 {
            let p = &mut doll.particles[i];
            p.pos = Vec2::new(home.x + k as f32 * 0.6, home.y - k as f32 * 0.6);
            p.prev = p.pos;
            doll.step(dt);
        }
        doll.particles[i].pinned = false;
        doll.restore_sides();
        let _ = side0;
        // Same re-centring the release handler does.
        let n = c.spine.len().max(1) as f32;
        let mut cur = Vec2::new(0.0, 0.0);
        let mut rst = Vec2::new(0.0, 0.0);
        for (k, &idx) in c.spine.iter().enumerate() {
            cur.x += doll.particles[idx].pos.x;
            cur.y += doll.particles[idx].pos.y;
            rst.x += rest.spine[k].x;
            rst.y += rest.spine[k].y;
        }
        let shift = Vec2::new((cur.x - rst.x) / n, (cur.y - rst.y) / n);
        for p in doll.particles.iter_mut() {
            p.pos.x -= shift.x;
            p.pos.y -= shift.y;
            p.prev.x -= shift.x;
            p.prev.y -= shift.y;
        }
        // Let it walk on for three seconds.
        for _ in 0..360 {
            w.advance(dt);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
        }
        // Every ball should end up within a body's reach of the spine.
        let mut cx = 0.0;
        let mut cy = 0.0;
        for &idx in &c.spine {
            cx += doll.particles[idx].pos.x;
            cy += doll.particles[idx].pos.y;
        }
        cx /= n;
        cy /= n;
        for (k, p) in doll.particles.iter().enumerate() {
            let d = (p.pos.x - cx).hypot(p.pos.y - cy);
            assert!(d < 260.0, "ball {k} stranded {d:.0} px from the body");
        }
    }

    /// Frame the creature at a heading and report which eyes are visible.
    fn visible_eyes(heading: f32) -> Vec<bool> {
        let (doll, c, _rest, mut w) = rig();
        w.body.heading = heading;
        w.heading_body = heading;
        for f in w.facing.iter_mut() {
            *f = heading;
        }
        c.eye_lids
            .iter()
            .map(|&(sclera, _)| {
                let (eye, head) = (&doll.particles[sclera], &doll.particles[c.skull]);
                let dfwd = eye.pos.x - head.pos.x;
                let dlat = eye.side - head.side;
                dfwd * heading.cos() - dlat * heading.sin() < 0.0
            })
            .collect()
    }

    #[test]
    fn one_eye_in_profile_and_two_from_the_front() {
        use std::f32::consts::{FRAC_PI_2, PI};
        assert_eq!(visible_eyes(0.0), vec![false, false], "facing away should show no eyes");
        let profile = visible_eyes(FRAC_PI_2);
        assert_eq!(profile.iter().filter(|v| **v).count(), 1, "profile shows one eye: {profile:?}");
        let front = visible_eyes(PI);
        assert_eq!(front.iter().filter(|v| **v).count(), 2, "facing the camera shows both: {front:?}");
    }

    #[test]
    fn both_eyes_stay_visible_across_the_front_of_the_turn() {
        // The complaint was that the far eye vanished too early as the animal
        // turned. Sweep the heading and measure the window where both eyes are
        // up: it should be a good wide arc centred on facing the camera, not a
        // sliver.
        let mut both = 0;
        let mut first_both: Option<f32> = None;
        let mut last_both: Option<f32> = None;
        for deg in 0..360 {
            let h = (deg as f32).to_radians();
            let n = visible_eyes(h).iter().filter(|v| **v).count();
            if n == 2 {
                both += 1;
                first_both.get_or_insert(h.to_degrees());
                last_both = Some(h.to_degrees());
            }
        }
        assert!(
            both >= 60,
            "both eyes are only up for {both} degrees of the turn (from {first_both:?} to {last_both:?})"
        );
        let (lo, hi) = (first_both.unwrap(), last_both.unwrap());
        assert!(lo < 180.0 && hi > 180.0, "the both-eyes arc is not centred on the front: {lo}..{hi}");
    }

    #[test]
    fn nearer_balls_are_drawn_last() {
        // Painter's order, keyed on depth: the far side of the body must be
        // drawn before the near side, or a dog walking away shows its chest
        // through its back.
        let (doll, c, w) = run(1.0);
        let view = View::new(25.0, GROUND);
        let depths: Vec<f32> = c
            .legs
            .iter()
            .map(|l| {
                let p = &doll.particles[l.foot];
                view.project(w.body.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y)).depth
            })
            .collect();
        let mut order: Vec<usize> = (0..4).collect();
        order.sort_by(|&a, &b| depths[b].partial_cmp(&depths[a]).unwrap());
        // Sorted first == drawn first == furthest away.
        assert!(depths[order[0]] >= depths[order[3]]);
        assert!(
            (depths[0] - depths[1]).abs() > 1.0,
            "left and right feet are at the same depth - side offsets are not reaching the projection"
        );
    }

    #[test]
    fn feet_are_on_the_floor_for_most_of_the_cycle() {
        // A foot is above the floor during swing; that is the point of a swing.
        // It must spend most of its time planted, and never fly higher than a
        // step.
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let mut planted = vec![0usize; c.legs.len()];
        let mut highest = vec![f32::MIN; c.legs.len()];
        let samples = 240;
        for _ in 0..samples {
            w.advance(dt);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
            for (i, leg) in c.legs.iter().enumerate() {
                let y = doll.particles[leg.foot].pos.y;
                if (y - GROUND).abs() < 1.5 {
                    planted[i] += 1;
                }
                highest[i] = highest[i].max(GROUND - y);
            }
        }
        for i in 0..c.legs.len() {
            let share = planted[i] as f32 / samples as f32;
            assert!(share > 0.35, "leg {i} planted only {:.0}% of the time", share * 100.0);
            assert!(
                highest[i] < w.gait.spec.step_height + 6.0,
                "leg {i} lifted {:.1} px, step is {:.1}",
                highest[i],
                w.gait.spec.step_height
            );
        }
    }

    #[test]
    fn the_body_stays_up_and_keeps_its_height() {
        let (doll, c, _) = run(2.0);
        let spine_y: Vec<f32> = c.spine.iter().map(|&i| doll.particles[i].pos.y).collect();
        for (i, y) in spine_y.iter().enumerate() {
            assert!(
                (*y - (GROUND - 68.0)).abs() < 14.0,
                "spine ball {i} drifted to y={y:.1}"
            );
        }
    }

    #[test]
    fn the_legs_actually_cycle() {
        // A foot's horizontal offset from its hip should sweep through a range,
        // not sit still - that is the difference between walking and sliding.
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for _ in 0..240 {
            w.advance(dt);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
            let leg = &c.legs[0];
            let off = doll.particles[leg.foot].pos.x - doll.particles[leg.hip].pos.x;
            lo = lo.min(off);
            hi = hi.max(off);
        }
        assert!(hi - lo > 20.0, "front foot only swept {:.1} px", hi - lo);
    }

    #[test]
    fn the_tail_is_left_to_physics() {
        let (doll, c, _) = run(1.0);
        // Tail balls are the last four particles and carry no target.
        let n = doll.particles.len();
        for i in (n - 4)..n {
            assert!(doll.particles[i].target.is_none(), "tail ball {i} is being driven");
        }
        // ... and they should have settled somewhere plausible rather than run away.
        for i in (n - 4)..n {
            let p = doll.particles[i].pos;
            assert!(p.y > 0.0 && p.y < 480.0, "tail ball {i} at {p:?}");
        }
        let _ = &c;
    }
}
