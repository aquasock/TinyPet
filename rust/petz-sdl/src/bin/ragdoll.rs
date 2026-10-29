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

use petz_core::ragdoll::{Particle, Ragdoll, Vec2};
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
    r.iterations = 10;
    r.contact = 0.35;

    // Spine: rear haunch to shoulders, tapering.
    let spine_x = [270.0, 300.0, 330.0, 360.0, 390.0];
    let spine_r = [17.0, 16.0, 15.0, 14.0, 13.0];
    let mut spine = Vec::new();
    for i in 0..spine_x.len() {
        spine.push(r.add(Particle::new(spine_x[i], 150.0, spine_r[i], BODY)));
    }
    for i in 0..spine.len() - 1 {
        r.link(spine[i], spine[i + 1]);
    }
    // Soft braces along the spine: a body that folds is a worm, a body that
    // cannot fold is a plank. These give it a preferred posture without
    // locking it rigid.
    for i in 0..spine.len().saturating_sub(2) {
        r.constraints.push(petz_core::Constraint::soft(spine[i], spine[i + 2], 60.0, 0.25));
    }

    // Head: neck, skull, muzzle. The muzzle is lighter, as in the retail pet.
    let neck = r.add(Particle::new(405.0, 130.0, 12.0, BODY));
    let skull = r.add(Particle::new(425.0, 118.0, 20.0, BODY));
    let muzzle = r.add(Particle::new(443.0, 126.0, 10.0, LIGHT));
    let nose = r.add(Particle::new(452.0, 122.0, 4.0, DARK));
    r.link(*spine.last().unwrap(), neck);
    r.link(neck, skull);
    r.link(skull, muzzle);
    r.link(muzzle, nose);
    r.link(skull, *spine.last().unwrap()); // keep the head from lolling

    // Ears.
    let ear_a = r.add(Particle::new(421.0, 101.0, 7.0, BODY));
    let ear_b = r.add(Particle::new(433.0, 103.0, 7.0, BODY));
    r.link(skull, ear_a);
    r.link(skull, ear_b);

    // Legs: shoulder/haunch, knee, paw. Two per end, front and rear.
    let legs: [(usize, f32); 4] = [(0, 272.0), (1, 302.0), (3, 372.0), (4, 392.0)];
    for (spine_idx, x) in legs {
        let upper = r.add(Particle::new(x, 178.0, 9.0, BODY));
        let lower = r.add(Particle::new(x, 205.0, 7.0, BODY));
        let paw = r.add(Particle::new(x, 226.0, 6.0, LIGHT));
        r.link(spine[spine_idx], upper);
        r.link(upper, lower);
        r.link(lower, paw);
    }

    // Tail, hanging off the rear.
    let mut tail_prev = spine[0];
    for i in 0..4 {
        let t = r.add(Particle::new(255.0 - i as f32 * 14.0, 143.0, 6.0 - i as f32 * 0.8, BODY));
        r.link(tail_prev, t);
        tail_prev = t;
    }

    r
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
    let mut doll = build_creature();
    let mut events = sdl.event_pump()?;
    let mut grabbed: Option<usize> = None;
    let mut paused = false;

    // Fixed timestep. The simulation must not care how fast the machine is or
    // a replay stops being reproducible.
    let step = 1.0 / 120.0;
    let mut acc = 0.0f64;
    let mut last = Instant::now();

    println!("drag a ball with the left mouse button; R resets; Space pauses; Esc quits");

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
        order.sort_by(|&a, &b| doll.particles[a].pos.y.partial_cmp(&doll.particles[b].pos.y).unwrap());
        for i in order {
            let p = &doll.particles[i];
            let r = p.radius.round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = (p.pos.x.round() as i32, p.pos.y.round() as i32);
            let d = (r * 2) as usize;
            // Rim first, body over it: the same trick the outline pass uses.
            fill_disc(&mut surface, &table, cx, cy, (d + 4).min(255), OUTLINE);
            fill_disc(&mut surface, &table, cx, cy, d, p.colour);
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
