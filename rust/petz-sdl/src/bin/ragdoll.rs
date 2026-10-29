//! ragdoll - an interactive sandbox for the sphere-spring creature.
//!
//! This exists to answer one question by eye: does a cloud of circles joined by
//! springs, rendered with the game's own table-driven rasterizer, read as a
//! *body*? Petz's pets are exactly this, and the way to know whether our solver
//! captures that feel is to grab one and dangle it, which is what the game does
//! when you pick a pet up by the scruff.
//!
//! Drag with the left mouse button to grab the nearest ball and throw it around.
//! R rebuilds the creature, Escape quits, Space pauses.
//!
//! Nothing here is final art: the skeleton is a guess at the shape of a pet, to
//! be replaced by parameters fitted from captured trajectories.

use std::time::{Duration, Instant};

use std::collections::HashMap;

use petz_core::ragdoll::{Particle, Ragdoll, Vec2};
use petz_core::fur::{dapple, shade_disc_pattern, stripes, Pattern, Texture};
use petz_core::shade::{Light, Ramp, Vec3};
use petz_core::{fill_disc, Palette, SpanTable, Surface};
use std::fs;
use std::path::PathBuf;

const W: usize = 640;
const H: usize = 480;
const BACKGROUND: u8 = 130;

/// The colour ladder borrowed from the pet palette we captured: dark rim,
/// body tan, and two lights for the chest and muzzle.
const OUTLINE: u8 = 89;
const BODY: u8 = 61;
const LIGHT: u8 = 10;
const DARK: u8 = 0;

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

/// Build a quadruped out of balls: a spine, a head, four two-segment legs and a
/// tail. Radii taper the way a body's do, and the whole thing is laid out in
/// the same 640x480 space the renderer works in.
fn build_creature() -> Ragdoll {
    let mut r = Ragdoll::new();
    r.bounds = Some((8.0, 8.0, W as f32 - 8.0, H as f32 - 8.0));
    r.gravity = 1400.0;
    r.damping = 0.985;
    r.iterations = 16;
    r.contact = 0.35;

    // Radii are roughly 70% of the bone length, so neighbours overlap heavily
    // and the silhouette closes up. Spheres that only touch read as beads.
    let spacing = 26.0;
    let spine_r = [24.0, 23.0, 22.0, 21.0, 20.0];
    let mut spine = Vec::new();
    for i in 0..spine_r.len() {
        let x = 276.0 + i as f32 * spacing;
        spine.push(r.add(Particle::new(x, 150.0, spine_r[i], BODY)));
    }
    for i in 0..spine.len() - 1 {
        r.link(spine[i], spine[i + 1]);
    }
    // Braces along the spine: a body that folds at every joint is a worm, one
    // that cannot fold is a plank. Stiff enough to keep a posture, soft enough
    // to squash when it lands.
    for i in 0..spine.len().saturating_sub(2) {
        let rest = r.particles[spine[i]].pos.sub(r.particles[spine[i + 2]].pos).length();
        r.constraints.push(petz_core::Constraint::soft(spine[i], spine[i + 2], rest, 0.7));
    }
    // A long brace across the whole torso. This is what stops the spine
    // concertina-ing when the pet is lifted by the middle.
    {
        let rest = r.particles[spine[0]].pos.sub(r.particles[spine[4]].pos).length();
        r.constraints.push(petz_core::Constraint::soft(spine[0], spine[4], rest, 0.5));
    }

    // Head: neck, skull, muzzle. The muzzle is lighter, as in the retail pet.
    let last = *spine.last().unwrap();
    let neck = r.add(Particle::new(408.0, 138.0, 17.0, BODY));
    let skull = r.add(Particle::new(426.0, 122.0, 26.0, BODY));
    let muzzle = r.add(Particle::new(447.0, 132.0, 13.0, LIGHT));
    let nose = r.add(Particle::new(457.0, 127.0, 5.0, DARK));
    r.link(last, neck);
    r.link(neck, skull);
    r.link(skull, muzzle);
    r.link(muzzle, nose);
    // Two braces hold the head up instead of one joint doing all the work.
    r.link(skull, spine[3]);
    r.link(neck, spine[3]);

    // Ears.
    let ear_a = r.add(Particle::new(420.0, 100.0, 9.0, BODY));
    let ear_b = r.add(Particle::new(435.0, 104.0, 9.0, BODY));
    r.link(skull, ear_a);
    r.link(skull, ear_b);

    // Legs: shoulder/haunch, knee, paw. Each attached to two spine balls so the
    // limb cannot rotate freely about a point.
    let legs: [(usize, usize, f32); 4] = [(0, 1, 278.0), (1, 2, 304.0), (3, 4, 384.0), (2, 3, 410.0)];
    for (a, b, x) in legs {
        // Close under the spine so the shoulder ball always overlaps the
        // torso balls; a gap there is what makes a paw detach on screen.
        let upper = r.add(Particle::new(x, 170.0, 15.0, BODY));
        let lower = r.add(Particle::new(x, 196.0, 12.0, BODY));
        let paw = r.add(Particle::new(x, 218.0, 10.0, LIGHT));
        r.link(spine[a], upper);
        r.link(spine[b], upper);
        r.link(upper, lower);
        r.link(lower, paw);
    }

    // Tail, hanging off the rear.
    let mut tail_prev = spine[0];
    for i in 0..4 {
        let t = r.add(Particle::new(
            252.0 - i as f32 * 15.0,
            140.0,
            10.0 - i as f32 * 1.2,
            BODY,
        ));
        r.link(tail_prev, t);
        tail_prev = t;
    }

    // Stiffen every bone. Default links already sit at 1.0; this is explicit so
    // the intent survives edits.
    for c in r.constraints.iter_mut() {
        if c.stiffness >= 0.7 {
            c.stiffness = 1.0;
        }
    }

    r
}

/// Spin the light around the vertical axis without changing its elevation.
fn rotate_light(dir: Vec3, degrees: f32) -> Vec3 {
    let yaw = dir.x.atan2(dir.z).to_degrees() + degrees;
    let pitch = dir.y.asin().to_degrees();
    Vec3::from_angles(yaw, pitch)
}

/// Raise or lower the light, clamped short of the poles so the direction never
/// degenerates to straight up or straight down.
fn elevate_light(dir: Vec3, degrees: f32) -> Vec3 {
    let yaw = dir.x.atan2(dir.z).to_degrees();
    let pitch = (dir.y.asin().to_degrees() + degrees).clamp(-80.0, 80.0);
    Vec3::from_angles(yaw, pitch)
}

/// Print the current look so a tuned setup can be copied into the defaults
/// instead of being lost when the process exits.
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
    let scale = (display.width() as i32 / W as i32).min(display.height() as i32 / H as i32).max(1) as usize;

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

    // One ramp per material, derived from the game palette. Light picks the
    // band; the ramp belongs to the matter.
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

    // Patterns are procedural, so nothing here is game art. T cycles them.
    let textures: Vec<(&str, Texture)> = vec![
        ("plain", Texture::flat(64, 64, 255)),
        ("dapple", dapple(64, 64, 17, 255)),
        ("stripes", stripes(64, 64, 5, 255)),
    ];
    let mut tex_idx = 0usize;
    let patterns = |base: u8, alt: u8, steps: usize, tex: &Texture| Pattern {
        base: Ramp::from_palette(&palette, base, steps),
        alt: Ramp::from_palette(&palette, alt, steps),
        texture: tex.clone(),
        threshold: 128,
        phase: 0.0,
    };

    let mut doll = build_creature();
    let mut events = sdl.event_pump()?;
    let mut grabbed: Option<usize> = None;
    let mut paused = false;

    // Fixed timestep. The simulation must not care how fast the machine is or
    // a replay stops being reproducible.
    let step = 1.0 / 120.0;
    let mut acc = 0.0f64;
    let mut last = Instant::now();

    println!("drag: left mouse   light: arrows   ambience: A/S   steps: [ ]   texture: T   P: print settings   R reset   Space pause   Esc quit");
    report(light, steps, tex_idx, textures[tex_idx].0);

    'outer: loop {
        for e in events.poll_iter() {
            match e {
                sdl2::event::Event::Quit { .. } => break 'outer,
                sdl2::event::Event::KeyDown { keycode: Some(k), .. } => match k {
                    sdl2::keyboard::Keycode::Escape => break 'outer,
                    sdl2::keyboard::Keycode::R => {
                        doll = build_creature();
                        grabbed = None;
                    }
                    sdl2::keyboard::Keycode::Space => paused = !paused,
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
                    sdl2::keyboard::Keycode::A => {
                        light.ambient = (light.ambient - 0.05).max(0.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::S => {
                        light.ambient = (light.ambient + 0.05).min(0.9);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::T => {
                        tex_idx = (tex_idx + 1) % textures.len();
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::P => report(light, steps, tex_idx, textures[tex_idx].0),
                    _ => {}
                },
                sdl2::event::Event::MouseButtonDown { x, y, mouse_btn: sdl2::mouse::MouseButton::Left, .. } => {
                    let p = Vec2::new(x as f32 / scale as f32, y as f32 / scale as f32);
                    if let Some(i) = doll.nearest(p, 26.0) {
                        doll.particles[i].pinned = true;
                        doll.drag_to(i, p);
                        grabbed = Some(i);
                    }
                }
                sdl2::event::Event::MouseButtonUp { mouse_btn: sdl2::mouse::MouseButton::Left, .. } => {
                    if let Some(i) = grabbed.take() {
                        doll.particles[i].pinned = false;
                    }
                }
                sdl2::event::Event::MouseMotion { x, y, .. } => {
                    if let Some(i) = grabbed {
                        let p = Vec2::new(x as f32 / scale as f32, y as f32 / scale as f32);
                        doll.drag_to(i, p);
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
                doll.step(step as f32);
                acc -= step;
            }
        }

        let mut surface = Surface::new(W, H);
        for y in 0..H {
            for x in 0..W {
                surface.set(x, y, BACKGROUND);
            }
        }
        // Painter's order by y, so nearer balls cover further ones. That is the
        // whole depth model - the original has no z-buffer either.
        let mut order: Vec<usize> = (0..doll.particles.len()).collect();
        order.sort_by(|&a, &b| {
            doll.particles[a].pos.y.partial_cmp(&doll.particles[b].pos.y).unwrap()
        });

        // Rim and body are separate passes over the whole body, not two fills
        // per ball. Drawing ball-by-ball would leave a dark ring wherever one
        // ball overlaps another; the rim belongs to the silhouette, not to each
        // sphere.
        for &i in &order {
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = (p.pos.x.round() as i32, p.pos.y.round() as i32);
            fill_disc(&mut surface, &table, cx, cy, (((r * 2) + 4) as usize).min(255), OUTLINE);
        }
        for &i in &order {
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = (p.pos.x.round() as i32, p.pos.y.round() as i32);
            let d = (r * 2) as usize;
            let alt_base = match p.colour {
                BODY => LIGHT,
                LIGHT => BODY,
                other => other,
            };
            let pat = patterns(p.colour, alt_base, steps, &textures[tex_idx].1);
            shade_disc_pattern(&mut surface, &table, cx, cy, d, light, &pat);
        }
        let rgb = palette.to_rgb(&surface);
        tex.update(None, &rgb, W * 3).map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&tex, None, None).map_err(|e| e.to_string())?;
        canvas.present();

        if std::env::var("PETZ_ONESHOT").is_ok() {
            std::thread::sleep(Duration::from_millis(400));
            break;
        }
    }
    Ok(())
}
