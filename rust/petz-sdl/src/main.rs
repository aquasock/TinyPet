//! petz-sdl - play captured animation with our own rasterizer.
//!
//! The renderer is `petz-core`; this crate is only presentation: a window,
//! nearest-neighbour integer scaling, and a 30 Hz clock. Nothing here may alter
//! pixels - the surface is rendered at 640x480 and stretched, never resampled
//! with filtering.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use petz_core::{fill_disc, Palette, SpanTable, Surface};

const W: usize = 640;
const H: usize = 480;
const FPS: u64 = 30;
/// Palette index used as the backdrop (a grass green from the game palette).
const BACKGROUND: u8 = 130;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Animation {
    width: usize,
    height: usize,
    frames: Vec<Vec<Ball>>,
}

struct Ball {
    cx: i32,
    cy: i32,
    w: usize,
    colour: u8,
    /// 0 rotated, 1 normal, 2 emboss, 3 outline, 4 circleEx
    kind: u8,
}

fn load_animation(path: &PathBuf) -> Animation {
    let b = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(&b[0..4], b"PZAN", "not a petz animation file");
    let rd = |p: &mut usize| -> u32 {
        let v = u32::from_le_bytes(b[*p..*p + 4].try_into().unwrap());
        *p += 4;
        v
    };
    let mut p = 4;
    let _version = rd(&mut p);
    let width = rd(&mut p) as usize;
    let height = rd(&mut p) as usize;
    let frame_count = rd(&mut p) as usize;
    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        let n = rd(&mut p) as usize;
        let mut balls = Vec::with_capacity(n);
        for _ in 0..n {
            let cx = i16::from_le_bytes(b[p..p + 2].try_into().unwrap()) as i32;
            let cy = i16::from_le_bytes(b[p + 2..p + 4].try_into().unwrap()) as i32;
            let w = u16::from_le_bytes(b[p + 4..p + 6].try_into().unwrap()) as usize;
            let colour = b[p + 8];
            let kind = b[p + 9];
            p += 12;
            balls.push(Ball { cx, cy, w, colour, kind });
        }
        frames.push(balls);
    }
    Animation { width, height, frames }
}

fn load_table() -> SpanTable {
    SpanTable::new(fs::read(repo_root().join("evidence/ball/tables.raw"))
        .expect("evidence/ball/tables.raw"))
}

fn load_palette() -> Palette {
    let raw = fs::read(repo_root().join("evidence/ball/ball-input.raw"))
        .expect("evidence/ball/ball-input.raw");
    // Header size varies between captures; derive it.
    let stride = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as usize;
    let height = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let header = raw.len() - (1024 + 512 + 2 * stride * height);
    Palette::from_rgbquad(&raw[header..header + 1024])
}

fn main() -> Result<(), String> {
    let anim_path = repo_root().join("build/animation.petz");
    if !anim_path.exists() {
        return Err("build/animation.petz missing - run tools/export-frames.py".into());
    }
    let anim = load_animation(&anim_path);
    // Follow-camera: the clip covers the pet's whole travel range, so at any one
    // frame it occupies a small corner. Centre each frame on its own bounds and
    // size the canvas to the largest frame, which lets the window scale up.
    let mut frames = anim.frames;
    let (mut maxw, mut maxh) = (1usize, 1usize);
    for f in &frames {
        if f.is_empty() { continue; }
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for b in f {
            x0 = x0.min(b.cx - b.w as i32 / 2);
            y0 = y0.min(b.cy - b.w as i32 / 2);
            x1 = x1.max(b.cx + b.w as i32 / 2);
            y1 = y1.max(b.cy + b.w as i32 / 2);
        }
        maxw = maxw.max((x1 - x0) as usize);
        maxh = maxh.max((y1 - y0) as usize);
    }
    let (w, h) = (maxw + 32, maxh + 32);
    for f in &mut frames {
        if f.is_empty() { continue; }
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for b in f.iter() {
            x0 = x0.min(b.cx - b.w as i32 / 2);
            y0 = y0.min(b.cy - b.w as i32 / 2);
            x1 = x1.max(b.cx + b.w as i32 / 2);
            y1 = y1.max(b.cy + b.w as i32 / 2);
        }
        let (dx, dy) = ((w as i32 - (x1 - x0)) / 2 - x0, (h as i32 - (y1 - y0)) / 2 - y0);
        for b in f.iter_mut() { b.cx += dx; b.cy += dy; }
    }
    let anim = Animation { width: w, height: h, frames };
    let table = load_table();
    let palette = load_palette();
    println!("loaded {} frames", anim.frames.len());

    let sdl = sdl2::init()?;
    let video = sdl.video()?;

    // Largest integer scale that fits the display. Fractional scaling would
    // resample pixels, which defeats the point.
    let display = video.display_bounds(0)
        .map(|r| (r.width() as usize, r.height() as usize))
        .unwrap_or((1280, 720));
    let scale = (display.0 / w).min(display.1 / h).max(1);
    println!("display {:?}, scale {scale}", display);
    let (ww, wh) = (w * scale, h * scale);
    let window = video
        .window("petz", ww as u32, wh as u32)
        .position_centered()
        .build()
        .map_err(|e| e.to_string())?;
    println!("video driver: {}", video.current_video_driver());
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;
    canvas.window_mut().show();
    println!("window size: {:?}", canvas.window().size());
    let tc = canvas.texture_creator();
    let mut tex = tc
        .create_texture_streaming(sdl2::pixels::PixelFormatEnum::RGB24, w as u32, h as u32)
        .map_err(|e| e.to_string())?;
    tex.set_scale_mode(sdl2::render::ScaleMode::Nearest);

    let mut events = sdl.event_pump()?;
    let pinned: Option<usize> = env::var("PETZ_FRAME").ok().and_then(|v| v.parse().ok());
    let mut frame_idx = pinned.unwrap_or(0);
    let mut last = Instant::now();
    let start = Instant::now();

    'outer: loop {
        for e in events.poll_iter() {
            match e {
                sdl2::event::Event::Quit { .. } => break 'outer,
                sdl2::event::Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::Escape), .. } => break 'outer,
                _ => {}
            }
        }

        // Render at the game's resolution, then stretch with nearest filtering.
        let mut surface = Surface::new(w, h);
        for y in 0..h {
            for x in 0..w {
                surface.set(x, y, BACKGROUND);
            }
        }
        if frame_idx % 60 == 0 { eprintln!("balls this frame: {}", anim.frames[frame_idx].len()); }
        // Outlines are filled discs slightly larger than the body, so they must
        // be laid down first for the body to punch out the middle and leave a
        // rim. Draw order in the captured stream is not reliable for this.
        let frame = &anim.frames[frame_idx];
        for ball in frame.iter().filter(|b| b.kind == 3) {
            if ball.w == 0 || ball.w > table.max_diameter() { continue; }
            fill_disc(&mut surface, &table, ball.cx, ball.cy, ball.w, ball.colour);
        }
        for ball in frame.iter().filter(|b| b.kind != 3) {
            if ball.w == 0 || ball.w > table.max_diameter() {
                continue;
            }
            if ball.kind == 0 {
                // Iris: drawn last over the pupil, so it must be a ring or the
                // pupil disappears. Hole sized from the pupil record (w-8).
                petz_core::fill_ring(&mut surface, &table, ball.cx, ball.cy, ball.w, 4, ball.colour);
            } else {
                fill_disc(&mut surface, &table, ball.cx, ball.cy, ball.w, ball.colour);
            }
        }
        let painted = surface.pixels().iter().filter(|&&p| p != 12).count();
        if frame_idx % 30 == 0 { eprintln!("painted {painted} px on frame {frame_idx}"); }
        let rgb = palette.to_rgb(&surface);

        tex.update(None, &rgb, w * 3).map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&tex, None, None).map_err(|e| e.to_string())?;
        canvas.present();

        if pinned.is_none() { frame_idx = (frame_idx + 1) % anim.frames.len(); }
        if frame_idx % 30 == 0 { eprintln!("frame {frame_idx}"); }

        // Fixed-rate playback, and a running count in the title.
        let target = Duration::from_millis(1000 / FPS);
        let elapsed = last.elapsed();
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
        last = Instant::now();
        let _ = start;
    }
    Ok(())
}
