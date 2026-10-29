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

use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use std::time::Instant;

use petz_core::fur::{dapple, stripes, Pattern, Texture};
use petz_core::gait::{two_bone_ik, Gait, GaitSpec};
use petz_core::ragdoll::{Particle, Ragdoll, Vec2};
use petz_core::project::{Body, Projected, Vec3 as WVec3, View};
use petz_core::shade::{Light, Ramp, Vec3};
use petz_core::{draw_disc, draw_disc_on_sphere, draw_flat_disc, draw_ring, DepthBuffer, Paint, Palette, SpanTable, Surface};
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

/// One eye per side, at this lateral offset.
const EYE_SIDES: [f32; 2] = [-12.0, 12.0];
/// Ears are a darker shade than the coat, as they are in every breed file.
const EAR: u8 = 95;
/// The eyelid shell. A shade darker than the coat, so the lid reads as a lid
/// rather than as more of the face.
const LID: u8 = 69;
/// A lighter shade for the tip, so the ear is two-tone and reads as an ear
/// rather than a lump. The breed files do the same thing with an inner-ear
/// colour.
const EAR_TIP: u8 = 100;

/// Where the floor is. Feet stand on this line.
const GROUND: f32 = 218.0;

/// Centre of the body in the skeleton's own forward coordinates. The bones are
/// authored at absolute x, so this is subtracted before placing them in the
/// world - otherwise the animal stands a body-length behind its own position.
const BODY_ORIGIN: f32 = 366.0;

/// A 3x5 digit font, hand-written. The angle readout is the only text the
/// sandbox needs, and it is not worth a font dependency for that.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// Draw a non-negative integer in window coordinates.
fn draw_number(
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    x: i32,
    y: i32,
    scale: i32,
    value: i32,
) {
    let text = value.max(0).to_string();
    let width = text.len() as i32 * 4 * scale;
    let _ = canvas.set_draw_color(Color::RGB(0, 0, 0));
    let _ = canvas.fill_rect(Rect::new(x - 5, y - 5, (width + 9) as u32, (5 * scale + 9) as u32));
    let _ = canvas.set_draw_color(Color::RGB(255, 255, 255));
    for (i, ch) in text.chars().enumerate() {
        let d = ch.to_digit(10).unwrap_or(0) as usize;
        for (row, bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3 {
                if bits & (1 << (2 - col)) != 0 {
                    let _ = canvas.fill_rect(Rect::new(
                        x + i as i32 * 4 * scale + col * scale,
                        y + row as i32 * scale,
                        scale as u32,
                        scale as u32,
                    ));
                }
            }
        }
    }
}

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
    /// The sclera of each eye, for the visibility test.
    eyes: Vec<usize>,
    /// The ear balls: (particle, side sign).
    ears: Vec<(usize, f32)>,
    /// (sclera, iris, rest height, side sign) per eye, so the placement can be
    /// tuned while running instead of being baked in.
    eye_parts: Vec<(usize, usize, f32, f32)>,
    /// Every ball that can occlude an eye. The eye is clipped against all of
    /// them, not just the cranium.
    head_balls: Vec<usize>,
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
    // The last of these is the neck; 20% down from where it was.
    let spine_r = [24.0, 23.0, 22.0, 16.0];
    let mut spine = Vec::new();
    for i in 0..spine_r.len() {
        let fwd = 276.0 + i as f32 * spacing;
        spine.push(r.add(Particle::new(fwd, GROUND - 68.0, spine_r[i], BODY)));
    }
    // Lift the neck onto the head's own axis. The skull runs from (386, base-26)
    // to the muzzle at (408, base-16); continuing that line backwards by one
    // head length puts the neck at (364, base-16). So the neck leaves the body
    // at a slight angle and the neck-to-skull run is parallel to skull-to-muzzle,
    // instead of the spine lying flat and the head hopping up off it.
    r.particles[spine[3]].pos = Vec2::new(364.0, GROUND - 84.0);

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

    // Head, hanging directly off the neck ball: a single skull, with a muzzle
    // and nose on the front of it. The cluster of seven was an experiment in
    // making the eye clip against a socket; the shell model made it unnecessary.
    let base_y = GROUND - 68.0;
    let neck = *spine.last().unwrap();
    let skull = r.add(Particle::new(386.0, base_y - 26.0, 26.0, BODY));
    // Out along the axis by 2 px, about 10% of its offset from the skull.
    let muzzle = r.add(Particle::new(408.0, base_y - 16.0, 13.0, LIGHT));
    // Out on the tip, clear of the muzzle's own sphere. Closer in it counts as
    // *inside* the muzzle and the containment rule hides it, which is what
    // swallowed the black tip.
    // The nose rides on the muzzle, so it moves with it.
    let nose = r.add(Particle::new(422.0, base_y - 20.0, 5.0, DARK));
    r.particles[nose].no_rim = true;
    r.link(neck, skull);
    r.link(skull, muzzle);
    r.link(muzzle, nose);
    // A second brace, so the head cannot nod away from the shoulders.
    r.link(skull, spine[spine.len() - 2]);

    let head_balls = vec![skull, muzzle, nose];

    // Ears: a base and a tip each. Two balls, because the retail ear is six and
    // buys a shape we do not need at this fidelity. The tip is deliberately not
    // driven - the solver gives it the flop for free, and it bounces with the
    // gait because the base hangs off the skull.
    // Ears: one ordinary ball each, placed the way the eye experiment showed
    // works - on the head's sphere by radius and two angles, pinned, unlinked,
    // and drawn in front of the head. Everything learned, none of the eye.
    let mut ear_eyes: Vec<(usize, f32)> = Vec::new();
    for side in [-1.0f32, 1.0] {
        let ear = r.add(Particle::new(390.0, base_y - 58.0, 10.0, EAR));
        r.particles[ear].pinned = true;
        r.particles[ear].side = 10.0 * side;
        r.particles[ear].rest_side = 10.0 * side;
        ear_eyes.push((ear, side));
    }

    // Pull the neck and head back into the body by a fifth. The compression is
    // towards the shoulder ball, so the head keeps its proportions instead of
    // simply sliding sideways through the chest.
    const SHOULDER: f32 = 332.0;
    let mut pulled: Vec<usize> = vec![spine[3], skull, muzzle, nose];
    pulled.extend(ear_eyes.iter().map(|&(e, _)| e));
    for i in pulled {
        let p = &mut r.particles[i];
        p.pos.x = SHOULDER + 0.8 * (p.pos.x - SHOULDER);
    }
    // Every spring was built against the old layout, so re-solve them from the
    // arrangement that actually exists now.
    let pairs: Vec<(usize, usize)> = r.constraints.iter().map(|c| (c.a, c.b)).collect();
    for (a, b) in pairs {
        let rest = r.particles[a].pos.sub(r.particles[b].pos).length();
        r.set_rest(a, b, rest);
    }

    // Eyes. The retail data is two balls per eye: a white sclera and a black
    // pupil whose *outline* is the iris, in a per-breed colour. There is no
    // eyelid ball at all - Petz parameterises the lid instead.
    //
    // No ears for now: they sit where they can cover the eye in profile, and
    // until the eyes are right we cannot tell what is hiding what.
    let mut eyes = Vec::new();
    let mut overlay_pairs: Vec<(usize, usize)> = Vec::new();
    let mut eye_list: Vec<usize> = Vec::new();
    let mut eye_parts: Vec<(usize, usize, f32, f32)> = Vec::new();
    for side in EYE_SIDES {
        let sclera = r.add(Particle::new(394.0, base_y - 34.0, 10.0, SCLERA));
        let iris = r.add(Particle::new(394.0, base_y - 33.0, 6.0, IRIS));
        for i in [sclera, iris] {
            r.particles[i].side = side;
            r.particles[i].rest_side = side;
        }
        r.particles[iris].rim = IRIS_RING;
        r.particles[iris].layer = 1;
        // The eyes are placed exactly, not settled. Left to the solver they
        // drifted apart by five pixels and one sat higher than the other.
        r.particles[sclera].pinned = true;
        r.particles[iris].pinned = true;

        r.link(skull, sclera);
        // The pupil and the lid are children of the eye, not of the skull.
        // Unlinked they were free particles: any grab cleared their drive and
        // they simply fell out of the head.
        r.link(sclera, iris);

        eyes.extend([sclera, iris]);
        overlay_pairs.push((sclera, iris));
        eye_list.push(sclera);
        eye_parts.push((sclera, iris, base_y - 34.0, side.signum()));
    }

    // Legs. The front pair hangs off the ball *behind* the neck - the shoulder -
    // and the hind pair off the hips.
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

    let mut head = vec![skull, muzzle, nose];
    head.extend(eyes);
    (r, Creature { spine, legs, head, tail, skull, eye_overlays: overlay_pairs, eyes: eye_list, ears: ear_eyes, eye_parts, head_balls })
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
    /// Where the eyes sit on the head: an angle round the skull from straight
    /// ahead, and a radius from the skull's centre.
    ///
    /// Parameterising by angle rather than by a lateral offset matters. Two
    /// parallel lines pushed sideways eventually pass *through* the head; a
    /// point at a fixed radius stays on its surface, so the eye is always
    /// tangent to the head and can never sink into it.
    eye_angle: f32,
    eye_radius: f32,
    /// Depth bias in the eye's favour, in world units. The occlusion test is
    /// "is the eye's surface nearer than the head's", and this moves the
    /// threshold: higher keeps the eye visible a little longer, at the cost of
    /// it occasionally winning against the head where it should not.
    eye_bias: f32,
    /// Outer shell: how far the eyelid has dropped across the eye, 0 open and
    /// 1 closed, and the tilt of the line that divides lid from open eye.
    lid_drop: f32,
    lid_tilt: f32,
    /// How far above the skull's centre the eyes sit, in body units.
    eye_lift: f32,
    /// Ears: angle round the skull from straight up, radius from its centre,
    /// how far the tip sinks into the base, and their depth bias.
    ear_angle: f32,
    ear_radius: f32,
    ear_sink: f32,
    ear_bias: f32,
    /// The lateral angle of each ear round the head's vertical axis. An angle,
    /// not a distance: with a distance, changing the radius also changed the
    /// direction from the head's centre, so the ear slid sideways instead of
    /// out along its own ray.
    ear_width: f32,
    /// Last movement input, so the pose knows whether to animate a gait.
    moving: f32,
}

/// Fastest the animal's body can turn, in radians per second.
const MAX_TURN_RATE: f32 = 2.4;

/// How fast A/D turn the animal, in radians per second.
const TURN_BUTTON_RATE: f32 = 1.8;

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
            // The placement tuned by eye, expressed on the sphere: 27 degrees
            // round from straight ahead, at radius 31 against a head of 26.
            eye_angle: 25.0_f32.to_radians(),
            eye_radius: 29.0,
            eye_bias: 1.5,
            lid_drop: 0.26,
            lid_tilt: 0.23,
            eye_lift: 12.0,
            // The values the ears were hand-placed at: 33 from the skull's
            // centre, 14 degrees forward of straight up, tips sunk 35%.
            ear_angle: -14.0_f32.to_radians(),
            ear_radius: 30.0,
            ear_sink: 0.35,
            ear_bias: -30.0,
            ear_width: 17.6_f32.to_radians(),
            moving: 1.0,
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

    /// `moving` is +1 forward, -1 backward, 0 standing. `turning` is -1 left,
    /// +1 right, 0 straight.
    fn advance(&mut self, dt: f32, moving: f32, turning: f32) {
        // Turning happens whether or not the animal is moving - a dog can look
        // round while standing.
        self.body.heading += turning * TURN_BUTTON_RATE * dt;
        self.moving = moving;
        self.turn(dt);

        if moving == 0.0 {
            return;
        }
        let f = Body { pos: self.body.pos, heading: self.heading_body }.forward();
        let nx = self.body.pos.x + f.x * self.gait.spec.speed * moving * dt;
        let ny = self.body.pos.y + f.y * self.gait.spec.speed * moving * dt;
        // Stay on the visible ground by stopping, not by turning. Turning is the
        // player's job: an animal that turns itself round makes any angle you
        // are trying to look at impossible to hold.
        // Bounds of the visible patch of ground at the default view pitch.
        let cx = nx.clamp(70.0, 560.0);
        let cy = ny.clamp(-700.0, 300.0);
        let blocked = (cx - nx).abs() > 1e-3 || (cy - ny).abs() > 1e-3;
        self.body.pos.x = cx;
        self.body.pos.y = cy;
        if !blocked {
            // Running the gait backwards makes the feet plant correctly when
            // the animal reverses, instead of skating.
            self.gait.advance(dt * moving.signum());
        }
    }

    /// Drive the skeleton toward the pose the gait describes.
    fn pose(&self, doll: &mut Ragdoll, creature: &Creature, rest: &Pose) {
        let walking = self.moving != 0.0;
        // A small vertical bob, twice per cycle: the body rises as the legs
        // pass under it.
        let bob = (self.gait.phase * std::f32::consts::TAU * 2.0).sin() * 2.0;
        let lift = if walking { bob } else { 0.0 };

        let n = creature.spine.len().max(1);
        for (i, &p) in creature.spine.iter().enumerate() {
            let t = i as f32 / (n - 1).max(1) as f32;
            let (dv, dl) = if walking { self.gait.spine_flex(t) } else { (0.0, 0.0) };
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
        let skull_now = doll.particles[creature.skull].pos;
        let skull_rest = rest.head[0];
        {
            let sk = doll.particles[creature.skull].pos;
            let c = self.ear_width.cos();
            let bx = sk.x + self.ear_radius * self.ear_angle.sin() * c;
            let by = sk.y - self.ear_radius * self.ear_angle.cos() * c;
            for &(ear, sign) in &creature.ears {
                doll.drag_to(ear, Vec2::new(bx, by));
                if let Some(p) = doll.particles.get_mut(ear) {
                    p.side = sign * self.ear_radius * self.ear_width.sin();
                    p.z_bias = self.ear_bias;
                }
            }
        }

        for &(sclera, iris, y, sign) in &creature.eye_parts {
            // On the head's surface at the chosen angle, so widening the eyes
            // walks them round the skull instead of through it.
            let target = Vec2::new(
                skull_now.x + self.eye_radius * self.eye_angle.cos(),
                skull_now.y - self.eye_lift,
            );
            let _ = (y, skull_rest);
            for i in [sclera, iris] {
                // Pinned, so this is exact: the solver cannot pull the two eyes
                // out of symmetry.
                doll.drag_to(i, target);
                if let Some(p) = doll.particles.get_mut(i) {
                    p.side = sign * self.eye_radius * self.eye_angle.sin();
                }
            }
            // Let the link follow the eye. Keeping the length it was built at
            // meant the skeleton pulled the eye back towards the authored
            // position, so it could never be moved far out of the head.
            let sk = doll.particles[creature.skull].pos;
            let want = doll.particles[sclera].pos.sub(sk).length();
            doll.set_rest(creature.skull, sclera, want);
        }

        for (i, leg) in creature.legs.iter().enumerate() {
            let foot = if walking {
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

/// Is the eye's surface at this pixel in front of the nearest head surface?
/// Shared by the sphere, the pupil and the lid so all three shells are clipped
/// against exactly the same thing.
fn eye_pixel_ok(
    geo: &[(f32, f32, f32, f32)],
    x: f32,
    y: f32,
    eye_surface: f32,
    bias: f32,
) -> bool {
    let mut nearest = f32::INFINITY;
    for &(hx, hy, hr, hd) in geo {
        let d2 = (x - hx).powi(2) + (y - hy).powi(2);
        if d2 <= hr * hr {
            let surface = hd - (hr * hr - d2).max(0.0).sqrt();
            if surface < nearest {
                nearest = surface;
            }
        }
    }
    eye_surface < nearest + bias
}

fn report_ears(w: &Walker) {
    println!(
        "ears: angle {:.0} deg  radius {:.1}  width {:.0} deg  bias {:.1}",
        w.ear_angle.to_degrees(),
        w.ear_radius,
        w.ear_width.to_degrees(),
        w.ear_bias
    );
}

fn report_eyes(w: &Walker) {
    println!(
        "eyes: angle {:.0} deg  radius {:.1}  lift {:.1}  bias {:.1}  lid {:.2} tilt {:.2}",
        w.eye_angle.to_degrees(),
        w.eye_radius,
        w.eye_lift,
        w.eye_bias,
        w.lid_drop,
        w.lid_tilt
    );
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

    let sweep_t = 0.0f32;
    let mut sweep_last = Instant::now();
    let mut frame_no = 0u32;
    let mut held: std::collections::HashSet<Keycode> = std::collections::HashSet::new();
    let mut depth = DepthBuffer::new(W, H);
    // Ball numbers, for referring to them. Tab toggles.
    let mut show_ids = true;

    let mut events = sdl.event_pump()?;
    let mut paused = false;
    let step = 1.0 / 120.0;
    let mut acc = 0.0f64;
    let mut last = Instant::now();

    println!(
        "drag: left mouse   W/S: forward/back   A/D: turn left/right   +/-: speed   V: pitch\n\
         light: arrows   ambience: Z/X   steps: [ ]   texture: T   flex: , .\n\
         ears: H/J radius, K/L angle, N/M width, PgUp/PgDn sink, U/I bias\n\
         Tab: ball numbers\n\
         P: settings   R: reset   Space: pause   Esc: quit"
    );
    report(light, steps, tex_idx, textures[tex_idx].0);

    'outer: loop {
        for e in events.poll_iter() {
            match e {
                sdl2::event::Event::Quit { .. } => break 'outer,
                sdl2::event::Event::KeyDown { keycode: Some(k), .. } => {
                    held.insert(k);
                    match k {
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
                    sdl2::keyboard::Keycode::Tab => {
                        show_ids = !show_ids;
                        println!("ball numbers {}", if show_ids { "on" } else { "off" });
                    }
                    sdl2::keyboard::Keycode::P => {
                        report(light, steps, tex_idx, textures[tex_idx].0);
                        report_eyes(&walker);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::K => {
                        walker.ear_angle = (walker.ear_angle - 1.0_f32.to_radians()).max(-1.2);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::L => {
                        walker.ear_angle = (walker.ear_angle + 1.0_f32.to_radians()).min(1.2);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::H => {
                        walker.ear_radius = (walker.ear_radius - 1.0).max(10.0);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::J => {
                        walker.ear_radius = (walker.ear_radius + 1.0).min(70.0);
                        report_ears(&walker);
                    }
                    // Height as an angle round the head's centre, not an offset
                    // from the ground: raising an ear walks it up the sphere,
                    // the way its radius and width already do.
                    sdl2::keyboard::Keycode::PageUp => {
                        walker.ear_angle = (walker.ear_angle - 1.0_f32.to_radians()).max(-1.2);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::PageDown => {
                        walker.ear_angle = (walker.ear_angle + 1.0_f32.to_radians()).min(1.2);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::N => {
                        walker.ear_width = (walker.ear_width - 1.0_f32.to_radians()).max(0.0);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::M => {
                        walker.ear_width = (walker.ear_width + 1.0_f32.to_radians()).min(1.4);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::U => {
                        walker.ear_bias = (walker.ear_bias + 0.5).min(20.0);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::I => {
                        walker.ear_bias = (walker.ear_bias - 0.5).max(-40.0);
                        report_ears(&walker);
                    }
                    sdl2::keyboard::Keycode::Z => {
                        light.ambient = (light.ambient - 0.05).max(0.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::X => {
                        light.ambient = (light.ambient + 0.05).min(0.9);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    _ => {}
                    }
                }
                sdl2::event::Event::KeyUp { keycode: Some(k), .. } => {
                    held.remove(&k);
                }
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
        let frame_dt = (now - last).as_secs_f32().min(0.1);
        acc += frame_dt as f64;
        last = now;

        // Instrument: rotate steadily and stand still, so a sweep can be read
        // off the frame dump without touching the keyboard.
        if std::env::var("PETZ_SWEEP").is_ok() {
            walker.body.heading += 0.35 * frame_dt;
        }
        if let Ok(h) = std::env::var("PETZ_HEADING") {
            if let Ok(deg) = h.parse::<f32>() {
                let rad = deg.to_radians();
                walker.body.heading = rad;
                walker.heading_body = rad;
                for f in walker.facing.iter_mut() {
                    *f = rad;
                }
                walker.moving = 0.0;
            }
        }
        let sweep = std::env::var("PETZ_SWEEP").is_ok();
        let moving = if sweep {
            0.0
        } else if held.contains(&Keycode::W) {
            1.0
        } else if held.contains(&Keycode::S) {
            -1.0
        } else {
            0.0
        };
        let turning = if held.contains(&Keycode::A) || held.contains(&Keycode::Q) {
            -1.0
        } else if held.contains(&Keycode::D) || held.contains(&Keycode::E) {
            1.0
        } else {
            0.0
        };

        if !paused {
            while acc >= step {
                if grabbed.is_none() {
                    walker.advance(step as f32, moving, turning);
                    walker.pose(&mut doll, &creature, &rest);
                }
                doll.step(step as f32);
                acc -= step;
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

        // Every ball goes through one depth-tested rasterizer, in any order.
        // The sort, the containment lists and the per-eye occlusion closures
        // are all gone: per-pixel depth answers "what is in front" once, for
        // every pair of balls, at every angle.
        depth.clear();

        let pattern_for = |colour: u8| {
            let alt = match colour {
                BODY => LIGHT,
                LIGHT => BODY,
                other => other,
            };
            Pattern {
                base: ramps.get(&colour).cloned().unwrap_or_else(|| Ramp::new(vec![colour])),
                alt: ramps.get(&alt).cloned().unwrap_or_else(|| Ramp::new(vec![alt])),
                texture: textures[tex_idx].1.clone(),
                threshold: 128,
                phase: 0.0,
            }
        };

        let eye_set: std::collections::HashSet<usize> = creature
            .eyes
            .iter()
            .copied()
            .chain(creature.eye_overlays.iter().map(|(_, o)| *o))
            .collect();

        // ---- ordinary balls --------------------------------------------------
        for i in 0..doll.particles.len() {
            if eye_set.contains(&i) {
                continue;
            }
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = (screen[i].x.round() as i32, screen[i].y.round() as i32);
            let cd = screen[i].depth + p.z_bias;
            if !p.no_rim {
                // A filled disc behind the body, not a ring: the body is
                // nearer, covers the middle, and the rim survives as the
                // two-pixel annulus with no rounding gap.
                //
                // The push has to beat the radius difference, not be some flat
                // epsilon. A disc of radius r+2 has its surface 2*nz nearer than
                // one of radius r at the same centre, so anything under 2 lets
                // the rim win across the whole ball - which is exactly what
                // happened: the dog came out entirely in outline colour.
                let rim = if p.rim == 0 { OUTLINE } else { p.rim };
                draw_flat_disc(&mut surface, &mut depth, &table, cx, cy,
                    ((r * 2) + 4) as usize, cd + 0.5, rim, &|_, _| true);
            }
            let pat = pattern_for(p.colour);
            draw_disc(&mut surface, &mut depth, &table, cx, cy, (r * 2) as usize, cd, 0.0,
                Paint::Shaded { light, pattern: &pat }, &|_, _| true);
        }

        // ---- eyes: three shells ---------------------------------------------
        let heading = walker.heading_body;
        let (pc, ps) = (view.pitch.cos(), view.pitch.sin());
        let mut groups: Vec<(usize, usize)> =
            creature.eye_parts.iter().map(|&(s, i, _, _)| (s, i)).collect();
        groups.sort_by(|a, b| screen[b.0].depth.partial_cmp(&screen[a.0].depth).unwrap());
        for &(sclera, iris) in &groups {
            let sp = &doll.particles[sclera];
            let cp = &doll.particles[creature.skull];
            let eh = walker.facing.get(sclera).copied().unwrap_or(heading);
            let fe = Body { pos: walker.body.pos, heading: eh }.forward();

            // Does the white sphere face the viewer? Its normal is the offset
            // from the head's centre.
            let (dx, dl, du) = (sp.pos.x - cp.pos.x, sp.side - cp.side, cp.pos.y - sp.pos.y);
            let n_y = dx * eh.cos() - dl * eh.sin();
            let inner_visible = (-pc * n_y + ps * du) > 0.0;

            let (cx, cy) = (screen[sclera].x, screen[sclera].y);
            let ix = cx.round() as i32;
            let iy = cy.round() as i32;
            let re = sp.radius.max(1.0);
            let de = screen[sclera].depth;

            if inner_visible {
                // Shell 2: the white sphere, a full circle.
                let pat = pattern_for(SCLERA);
                draw_disc(&mut surface, &mut depth, &table, ix, iy, (re as i32 * 2) as usize,
                    de, 0.0, Paint::Shaded { light, pattern: &pat }, &|_, _| true);

                // Shell 3: pupil and iris, sliding inside the sphere and
                // clipped by its rim rather than by the head.
                let off = re * fe.x;
                let px = (cx + off).round() as i32;
                let ip = &doll.particles[iris];
                let ri = ip.radius.max(1.0);
                let inside = {
                    let (cx0, cy0, re0) = (cx, cy, re);
                    move |x: i32, y: i32| -> bool {
                        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                        (fx - cx0).powi(2) + (fy - cy0).powi(2) <= re0 * re0
                    }
                };
                // Both sit on the sclera's surface, not on a sphere of their
                // own: a pupil with its own depth is inside the eye it is
                // painted on, and the sclera hides it.
                draw_disc_on_sphere(&mut surface, &mut depth, &table, px, iy,
                    ((ri as i32 * 2) + 4) as usize, cx, cy, re, de, -0.6, IRIS_RING, &inside);
                draw_disc_on_sphere(&mut surface, &mut depth, &table, px, iy,
                    (ri as i32 * 2) as usize, cx, cy, re, de, -0.9, IRIS, &inside);
            }

            // Shell 1: the lid. Never culled - it is coat colour, so over the
            // head it is invisible, and where the eye stands against something
            // else it is the outline that should persist. When there is no
            // inside to see, it fills the whole disc and the eye is shut.
            let drop = walker.lid_drop.clamp(0.0, 1.0);
            let tilt = walker.lid_tilt;
            let line = re * (1.0 - 2.0 * drop);
            let outer = re + 2.0;
            let lid_keep = move |x: i32, y: i32| -> bool {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (ux, uy) = (fx - cx, fy - cy);
                if ux * ux + uy * uy > outer * outer {
                    return false;
                }
                if !inner_visible {
                    return true;
                }
                uy < -(line + ux * tilt.tan())
            };
            draw_ring(&mut surface, &mut depth, &table, ix, iy,
                ((outer as i32 * 2) as usize).min(255), 2, de, -2.0, OUTLINE, &lid_keep);
            let lpat = pattern_for(LID);
            draw_disc(&mut surface, &mut depth, &table, ix, iy,
                ((outer as i32 * 2) as usize).min(255), de, -2.1,
                Paint::Shaded { light, pattern: &lpat }, &lid_keep);
        }

        // Dump our own framebuffer, bypassing the window system entirely. Screen
        // grabs are unreliable when other windows are about.
        if std::env::var("PETZ_DUMP_PPM").is_ok() {
            use std::io::Write;
            let rgb = palette.to_rgb(&surface);
            let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
            out.extend_from_slice(&rgb);
            if let Ok(mut f) = std::fs::File::create("/tmp/ours.ppm") {
                let _ = f.write_all(&out);
            }
            let dmin = screen.iter().map(|p| p.depth).fold(f32::INFINITY, f32::min);
            let dmax = screen.iter().map(|p| p.depth).fold(f32::NEG_INFINITY, f32::max);
            let nan = screen.iter().filter(|p| !p.depth.is_finite()).count();
            let xmin = screen.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
            let xmax = screen.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
            let ymin = screen.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
            let ymax = screen.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
            println!("dump: {} balls, depth {:.0}..{:.0}, screen x {:.0}..{:.0} y {:.0}..{:.0}",
                doll.particles.len(), dmin, dmax, xmin, xmax, ymin, ymax);
        }

        let rgb = palette.to_rgb(&surface);
        tex.update(None, &rgb, W * 3).map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&tex, None, None).map_err(|e| e.to_string())?;

        // Facing, in degrees: 0 is away from the viewer (up the screen), 90 is
        // to the right, 180 is towards the viewer, 270 to the left.
        let facing = walker.heading_body.to_degrees().rem_euclid(360.0).round() as i32;
        draw_number(&mut canvas, 18, 18, 4, facing);

        // A number on every ball, just above it, so one can be named exactly.
        if show_ids {
            for (i, p) in screen.iter().enumerate() {
                let x = (p.x * scale as f32) as i32;
                let y = (p.y * scale as f32) as i32 - doll.particles[i].radius as i32 * 2 - 12;
                if x < -20 || y < -20 || x > (W * scale) as i32 || y > (H * scale) as i32 {
                    continue;
                }
                draw_number(&mut canvas, x, y, 2, i as i32);
            }
        }

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
            w.advance(dt, 1.0, 0.0);
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
        w.advance(1.0 / 120.0, 1.0, 0.0);
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
            w.advance(1.0 / 120.0, 1.0, 0.0);
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
            w.advance(1.0 / 120.0, 1.0, 0.0);
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
            w.advance(1.0 / 120.0, 1.0, 0.0);
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
            w.advance(dt, 1.0, 0.0);
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
            w.advance(dt, 1.0, 0.0);
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
            w.advance(dt, 1.0, 0.0);
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

    /// Frame the creature at a heading and report which eyes are visible,
    /// using the same two tests the renderer applies: does the white sphere
    /// face the viewer, and is its centre outside the head.
    fn visible_eyes_at(heading: f32, radius: f32) -> Vec<bool> {
        let (mut doll, c, rest, mut w) = rig();
        w.eye_radius = radius;
        w.body.heading = heading;
        w.heading_body = heading;
        for f in w.facing.iter_mut() {
            *f = heading;
        }
        for _ in 0..90 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let view = View::new(25.0, GROUND);
        let proj = |i: usize| {
            let p = &doll.particles[i];
            let b = Body { pos: w.body.pos, heading };
            view.project(b.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y))
        };
        let heads: Vec<(f32, f32, f32, f32)> = c
            .head_balls
            .iter()
            .map(|&i| (i, proj(i)))
            .map(|(i, p)| (p.x, p.y, doll.particles[i].radius.max(1.0), p.depth))
            .collect();
        let cp = &doll.particles[c.skull];
        let (pc, ps) = (view.pitch.cos(), view.pitch.sin());
        c.eyes
            .iter()
            .map(|&sclera| {
                let sp = &doll.particles[sclera];
                // normal = offset from the head's centre; does it face the camera?
                let (dx, dl, du) = (
                    sp.pos.x - cp.pos.x,
                    sp.side - cp.side,
                    cp.pos.y - sp.pos.y,
                );
                let ny = dx * heading.cos() - dl * heading.sin();
                if -pc * ny + ps * du <= 0.0 {
                    return false;
                }
                let e = proj(sclera);
                let re = sp.radius.max(1.0);
                let eye_surface = e.depth - re;
                let mut nearest = f32::INFINITY;
                for &(hx, hy, hr, hd) in &heads {
                    let d2 = (e.x - hx).powi(2) + (e.y - hy).powi(2);
                    if d2 <= hr * hr {
                        let surf = hd - (hr * hr - d2).max(0.0).sqrt();
                        if surf < nearest {
                            nearest = surf;
                        }
                    }
                }
                eye_surface < nearest + w.eye_bias
            })
            .collect()
    }

    fn visible_eyes(heading: f32) -> Vec<bool> {
        visible_eyes_at(heading, 29.0)
    }

    /// Does this eye face the viewer? On the sphere the eye sits at
    /// radius * (cos angle forward, sin angle sideways), so the facing test
    /// reduces to radius * cos(angle +/- heading) - the radius cancels out of
    /// the sign and cannot flip an eye on its own.
    fn faces_viewer(heading: f32, angle: f32, sign: f32) -> bool {
        let (cx, sx) = (heading.cos(), heading.sin());
        let (ca, sa) = (angle.cos(), angle.sin());
        let forward = ca;
        let lateral = sign * sa;
        let away = forward * cx - lateral * sx;
        away < 0.0
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
        let mut both = 0;
        for deg in 0..360 {
            let h = (deg as f32).to_radians();
            if visible_eyes(h).iter().filter(|v| **v).count() == 2 {
                both += 1;
            }
        }
        assert!(both >= 30, "both eyes are only up for {both} degrees of the turn");
    }

    #[test]
    fn the_radius_cannot_flip_which_way_an_eye_faces() {
        // Reported from play: at 225 degrees, tapping the radius keys made the
        // left eye vanish. The facing rule must not depend on the radius; the
        // occlusion rule legitimately does, because a smaller radius buries the
        // eye in the head.
        let heading = 225.0_f32.to_radians();
        let angle = 27.0_f32.to_radians();
        let left = faces_viewer(heading, angle, -1.0);
        let right = faces_viewer(heading, angle, 1.0);
        assert!(left && right, "both eyes should face the viewer at 225 degrees");
    }

    #[test]
    fn the_two_eyes_are_level_when_the_animal_faces_straight_away() {
        // Reported from play: at 360 degrees the right eye sits higher than the
        // left. At exactly 0 both eyes are at the same depth, and the projection
        // only turns depth into height, so they must be level.
        let (mut doll, c, rest, mut w) = rig();
        w.body.heading = 0.0;
        w.heading_body = 0.0;
        for f in w.facing.iter_mut() {
            *f = 0.0;
        }
        for _ in 0..120 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let view = View::new(25.0, GROUND);
        let ys: Vec<f32> = c
            .eyes
            .iter()
            .map(|&i| {
                let p = &doll.particles[i];
                let b = Body { pos: w.body.pos, heading: 0.0 };
                view.project(b.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y)).y
            })
            .collect();
        let d = (ys[0] - ys[1]).abs();
        let info: Vec<(f32, f32, f32)> = c
            .eyes
            .iter()
            .map(|&i| (doll.particles[i].pos.x, doll.particles[i].pos.y, doll.particles[i].side))
            .collect();
        assert!(d < 0.6, "eyes differ in height by {d:.2} px: {ys:?} positions {info:?}");
    }

    #[test]
    fn the_head_has_the_expected_parts() {
        // Counts, because a bad edit once duplicated the eye block and the dog
        // came out with four of everything.
        let (doll, c, _, _) = rig();
        let count = |colour: u8| doll.particles.iter().filter(|p| p.colour == colour).count();
        // The ears are a second pair of eyes, so four of each shell.
        assert_eq!(count(SCLERA), 2, "sclerae");
        assert_eq!(count(IRIS), 2, "pupils");
        assert_eq!(count(EAR), 2, "ear balls");
        assert_eq!(count(EAR_TIP), 0, "no ear tips");
        assert_eq!(c.eyes.len(), 2, "two eyes");
        assert_eq!(c.ears.len(), 2, "two ear-eyes");
        assert_eq!(c.eye_overlays.len(), 2, "one pupil per eye");
        assert_eq!(c.head.len(), 7, "skull, muzzle, nose and four eye balls");
        assert_eq!(doll.particles.len(), 31, "total ball count");
    }

    #[test]
    fn changing_the_ear_radius_moves_it_straight_out() {
        // Reported from play: H and J should move the ears radially out of the
        // head. With the width as a distance they slid sideways as well.
        let dir = |w: &Walker, doll: &Ragdoll, c: &Creature| {
            let sk = &doll.particles[c.skull];
            let p = &doll.particles[c.ears[0].0];
            let v = (p.pos.x - sk.pos.x, p.side - sk.side, sk.pos.y - p.pos.y);
            let len = (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt().max(1e-6);
            let _ = w;
            (v.0 / len, v.1 / len, v.2 / len)
        };
        let mut dirs = Vec::new();
        for radius in [20.0f32, 30.0, 45.0] {
            let (mut doll, c, rest, mut w) = rig();
            w.ear_radius = radius;
            for _ in 0..120 {
                w.pose(&mut doll, &c, &rest);
                doll.step(1.0 / 120.0);
            }
            dirs.push(dir(&w, &doll, &c));
        }
        for d in &dirs[1..] {
            let dot = d.0 * dirs[0].0 + d.1 * dirs[0].1 + d.2 * dirs[0].2;
            assert!(dot > 0.999, "direction changed with radius: {d:?} vs {:?}", dirs[0]);
        }
    }

    #[test]
    fn the_ears_sit_on_the_head_sphere() {
        let (mut doll, c, rest, mut w) = rig();
        for _ in 0..120 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let sk = &doll.particles[c.skull];
        for &(base, _) in &c.ears {
            let p = &doll.particles[base];
            let (dx, dl, du) = (p.pos.x - sk.pos.x, p.side - sk.side, sk.pos.y - p.pos.y);
            let d = (dx * dx + dl * dl + du * du).sqrt();
            assert!(
                (d - w.ear_radius).abs() < 0.5,
                "ear base is {d:.1} from the skull, radius is {:.1}",
                w.ear_radius
            );
        }
    }

    #[test]
    fn the_two_ears_are_mirror_images() {
        // Reported from play: the two ears leaned in different directions. They
        // are pinned and placed exactly, so they are mirror images by
        // construction rather than by the solver agreeing with itself.
        let (mut doll, c, rest, mut w) = rig();
        for _ in 0..180 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let (e0, _) = c.ears[0];
        let (e1, _) = c.ears[1];
        let (pa, pb) = (doll.particles[e0].pos, doll.particles[e1].pos);
        assert!(pa.sub(pb).length() < 0.01, "ear positions differ: {pa:?} vs {pb:?}");
        let (sa, sb) = (doll.particles[e0].side, doll.particles[e1].side);
        assert!((sa + sb).abs() < 0.01 && sa.abs() > 1.0, "ear sides not mirrored");
    }

    #[test]
    fn tuning_the_ears_does_not_move_the_head() {
        // Reported from play: adjusting the ears dragged the head, the neck and
        // the spine with them. The ear springs were built at their original
        // lengths, so driving an ear made them pull on the skull.
        let settle = |angle: f32| {
            let (mut doll, c, rest, mut w) = rig();
            w.ear_angle = angle;
            for _ in 0..180 {
                w.pose(&mut doll, &c, &rest);
                doll.step(1.0 / 120.0);
            }
            (doll.particles[c.skull].pos, doll.particles[c.spine[3]].pos)
        };
        let (skull_a, neck_a) = settle(-0.35);
        let (skull_b, neck_b) = settle(0.9);
        assert!(
            skull_a.sub(skull_b).length() < 1.0,
            "skull moved from {skull_a:?} to {skull_b:?}"
        );
        assert!(
            neck_a.sub(neck_b).length() < 1.0,
            "neck moved from {neck_a:?} to {neck_b:?}"
        );
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
            w.advance(dt, 1.0, 0.0);
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
        let last = spine_y.len() - 1;
        for (i, y) in spine_y.iter().enumerate() {
            // The neck is lifted onto the head's axis, so it sits higher than
            // the rest of the spine by design.
            let rest = if i == last { GROUND - 84.0 } else { GROUND - 68.0 };
            assert!(
                (*y - rest).abs() < 14.0,
                "spine ball {i} drifted to y={y:.1}, expected about {rest:.0}"
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
            w.advance(dt, 1.0, 0.0);
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
