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

/// Where the floor is. Feet stand on this line.
const GROUND: f32 = 218.0;

/// Rest-pose centre of the body in world x; the camera keeps this centred.
const BODY_CENTRE: f32 = 366.0;

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
}

fn build() -> (Ragdoll, Creature) {
    let mut r = Ragdoll::new();
    r.bounds = Some((-2000.0, 8.0, 1_000_000.0, 460.0));
    r.gravity = 1400.0;
    r.damping = 0.985;
    r.iterations = 16;
    r.contact = 0.35;

    // Radii are ~70% of bone length so neighbours overlap and the silhouette
    // closes up into one mass rather than a row of beads.
    let spacing = 26.0;
    let spine_r = [24.0, 23.0, 22.0, 21.0, 20.0];
    let mut spine = Vec::new();
    for i in 0..spine_r.len() {
        let x = 276.0 + i as f32 * spacing;
        spine.push(r.add(Particle::new(x, GROUND - 68.0, spine_r[i], BODY)));
    }
    for i in 0..spine.len() - 1 {
        r.link(spine[i], spine[i + 1]);
    }
    for i in 0..spine.len().saturating_sub(2) {
        let rest = r.particles[spine[i]].pos.sub(r.particles[spine[i + 2]].pos).length();
        r.constraints.push(petz_core::Constraint::soft(spine[i], spine[i + 2], rest, 0.7));
    }
    {
        let rest = r.particles[spine[0]].pos.sub(r.particles[spine[4]].pos).length();
        r.constraints.push(petz_core::Constraint::soft(spine[0], spine[4], rest, 0.5));
    }

    // Head.
    let base_y = GROUND - 68.0;
    let last = *spine.last().unwrap();
    let neck = r.add(Particle::new(408.0, base_y - 12.0, 17.0, BODY));
    let skull = r.add(Particle::new(426.0, base_y - 28.0, 26.0, BODY));
    let muzzle = r.add(Particle::new(447.0, base_y - 18.0, 13.0, LIGHT));
    let nose = r.add(Particle::new(457.0, base_y - 23.0, 5.0, DARK));
    r.link(last, neck);
    r.link(neck, skull);
    r.link(skull, muzzle);
    r.link(muzzle, nose);
    r.link(skull, spine[3]);
    r.link(neck, spine[3]);

    let ear_a = r.add(Particle::new(420.0, base_y - 50.0, 9.0, BODY));
    let ear_b = r.add(Particle::new(435.0, base_y - 46.0, 9.0, BODY));
    r.link(skull, ear_a);
    r.link(skull, ear_b);

    // Legs: hip under the spine, knee, paw on the ground. Front legs bend one
    // way, hind legs the other, as a dog's do.
    let legs_spec: [(usize, f32, f32); 4] = [
        (1, 286.0, 1.0),  // left front
        (2, 312.0, 1.0),  // right front
        (3, 388.0, -1.0), // left hind
        (2, 414.0, -1.0), // right hind
    ];
    let mut legs = Vec::new();
    for (spine_idx, x, bend) in legs_spec {
        let hip = r.add(Particle::new(x, GROUND - 48.0, 15.0, BODY));
        let knee = r.add(Particle::new(x, GROUND - 22.0, 12.0, BODY));
        let foot = r.add(Particle::new(x, GROUND, 10.0, LIGHT));
        r.link(spine[spine_idx], hip);
        r.link(hip, knee);
        r.link(knee, foot);
        legs.push(Leg { hip, knee, foot, bend, rest: Vec2::new(x, GROUND) });
    }

    // Tail: no target, so it swings with the body.
    let mut tail_prev = spine[0];
    for i in 0..4 {
        let t = r.add(Particle::new(252.0 - i as f32 * 15.0, base_y - 10.0, 10.0 - i as f32 * 1.2, BODY));
        r.link(tail_prev, t);
        tail_prev = t;
    }

    let head = vec![neck, skull, muzzle, nose, ear_a, ear_b];
    (r, Creature { spine, legs, head })
}

/// The walking controller: owns the clock, the world position, and the pose.
struct Walker {
    gait: Gait,
    /// How far the animal has travelled.
    origin_x: f32,
    walking: bool,
}

impl Walker {
    fn new() -> Self {
        Self { gait: Gait::walk(GaitSpec::default()), origin_x: 0.0, walking: true }
    }

    fn advance(&mut self, dt: f32) {
        if !self.walking {
            return;
        }
        self.gait.advance(dt);
        self.origin_x += self.gait.spec.speed * dt;
    }

    /// Drive the skeleton toward the pose the gait describes.
    fn pose(&self, doll: &mut Ragdoll, creature: &Creature, rest: &Pose) {
        // A small vertical bob, twice per cycle: the body rises as the legs
        // pass under it.
        let bob = (self.gait.phase * std::f32::consts::TAU * 2.0).sin() * 2.0;
        let lift = if self.walking { bob } else { 0.0 };

        for (i, &p) in creature.spine.iter().enumerate() {
            let target = Vec2::new(rest.spine[i].x + self.origin_x, rest.spine[i].y + lift);
            doll.drive_to(p, target, 0.45);
        }
        for (i, &p) in creature.head.iter().enumerate() {
            let target = Vec2::new(rest.head[i].x + self.origin_x, rest.head[i].y + lift);
            doll.drive_to(p, target, 0.4);
        }

        for (i, leg) in creature.legs.iter().enumerate() {
            let foot = if self.walking {
                let f = self.gait.foot(i);
                Vec2::new(leg.rest.x + self.origin_x + f.dx, GROUND + f.dy)
            } else {
                Vec2::new(leg.rest.x + self.origin_x, GROUND)
            };

            let hip_target = Vec2::new(
                rest.legs[i].x + self.origin_x,
                rest.legs[i].y + lift,
            );
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
    let mut walker = Walker::new();
    let mut grabbed: Option<usize> = None;

    let mut events = sdl.event_pump()?;
    let mut paused = false;
    let step = 1.0 / 120.0;
    let mut acc = 0.0f64;
    let mut last = Instant::now();

    println!(
        "drag: left mouse   W: walk/stop   +/-: speed   light: arrows   ambience: A/S\n\
         steps: [ ]   texture: T   P: print settings   R: reset   Space: pause   Esc: quit"
    );
    report(light, steps, tex_idx, textures[tex_idx].0);

    'outer: loop {
        for e in events.poll_iter() {
            match e {
                sdl2::event::Event::Quit { .. } => break 'outer,
                sdl2::event::Event::KeyDown { keycode: Some(k), .. } => match k {
                    sdl2::keyboard::Keycode::Escape => break 'outer,
                    sdl2::keyboard::Keycode::R => {
                        let (d, _c) = build();
                        doll = d;
                        walker = Walker::new();
                        grabbed = None;
                    }
                    sdl2::keyboard::Keycode::Space => paused = !paused,
                    sdl2::keyboard::Keycode::W => walker.walking = !walker.walking,
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
                    let cam = walker.origin_x + BODY_CENTRE - W as f32 / 2.0;
                    let p = Vec2::new(x as f32 / scale as f32 + cam, y as f32 / scale as f32);
                    if let Some(i) = doll.nearest(p, 26.0) {
                        // Grabbing hands the animal to physics: the gait stops
                        // driving it and it goes limp.
                        doll.release_all();
                        doll.particles[i].pinned = true;
                        doll.drag_to(i, p);
                        grabbed = Some(i);
                    }
                }
                sdl2::event::Event::MouseButtonUp { mouse_btn: sdl2::mouse::MouseButton::Left, .. } => {
                    if let Some(i) = grabbed.take() {
                        doll.particles[i].pinned = false;
                        // Let the body fall where it is and walk on from there.
                        walker.origin_x = doll.particles[creature.spine[2]].pos.x
                            - rest.spine[2].x;
                    }
                }
                sdl2::event::Event::MouseMotion { x, y, .. } => {
                    if let Some(i) = grabbed {
                        let cam = walker.origin_x + BODY_CENTRE - W as f32 / 2.0;
                        doll.drag_to(
                            i,
                            Vec2::new(x as f32 / scale as f32 + cam, y as f32 / scale as f32),
                        );
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
        }

        let cam = walker.origin_x + BODY_CENTRE - W as f32 / 2.0;
        let mut surface = Surface::new(W, H);
        for y in 0..H {
            for x in 0..W {
                surface.set(x, y, BACKGROUND);
            }
        }

        let mut order: Vec<usize> = (0..doll.particles.len()).collect();
        order.sort_by(|&a, &b| {
            doll.particles[a].pos.y.partial_cmp(&doll.particles[b].pos.y).unwrap()
        });

        // Rim over the whole body, then shaded bodies, so overlaps do not leave
        // a dark ring round each individual ball.
        for &i in &order {
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = ((p.pos.x - cam).round() as i32, p.pos.y.round() as i32);
            fill_disc(&mut surface, &table, cx, cy, (((r * 2) + 4) as usize).min(255), OUTLINE);
        }
        for &i in &order {
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = ((p.pos.x - cam).round() as i32, p.pos.y.round() as i32);
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
            shade_disc_pattern(&mut surface, &table, cx, cy, (r * 2) as usize, light, &pattern);
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
        (doll, c, rest, Walker::new())
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
        let (_, _, w) = run(2.0);
        let expected = w.gait.spec.speed * 2.0;
        assert!(
            (w.origin_x - expected).abs() < 1.0,
            "travelled {} px, expected {expected}",
            w.origin_x
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
