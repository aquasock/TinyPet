//! petz-headless - render without a display, for the oracle.
//!
//! The point of this binary is to be scriptable: give it a task, get pixels and
//! a hash, compare against captured evidence. No window, no SDL, no timing.

use std::env;
use std::fs;
use std::path::PathBuf;

use petz_core::{fill_disc, Lnz, Palette, SpanTable, Surface};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// FNV-1a. Small, dependency-free, and adequate to spot a changed framebuffer
/// in a test. The authoritative hashing stays in the Python oracle (sha256).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn load_palette() -> Option<Palette> {
    let path = repo_root().join("evidence/ball/ball-input.raw");
    let raw = fs::read(path).ok()?;
    let start = 10 * 4;
    Some(Palette::from_rgbquad(&raw[start..start + 1024]))
}

fn load_table() -> Option<SpanTable> {
    let path = repo_root().join("evidence/ball/tables.raw");
    Some(SpanTable::new(fs::read(path).ok()?))
}

fn write_ppm(path: &PathBuf, width: usize, height: usize, rgb: &[u8]) -> std::io::Result<()> {
    let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
    out.extend_from_slice(rgb);
    fs::write(path, out)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("ball");

    match cmd {
        "ball" => {
            let table = load_table().expect("evidence/ball/tables.raw missing");
            let palette = load_palette().expect("evidence/ball/ball-input.raw missing");

            let (w, h) = (64usize, 48usize);
            let mut surface = Surface::new(w, h);
            // A disc of the same diameter as the captured ball, index 18.
            fill_disc(&mut surface, &table, 32, 24, 19, 18);

            let painted = surface.pixels().iter().filter(|&&p| p == 18).count();
            let rgb = palette.to_rgb(&surface);
            let out = args.get(1).map(PathBuf::from)
                .unwrap_or_else(|| repo_root().join("build/ball.ppm"));
            if let Some(parent) = out.parent() { let _ = fs::create_dir_all(parent); }
            write_ppm(&out, w, h, &rgb).expect("write ppm");

            println!("table max diameter : {}", table.max_diameter());
            println!("rows(19)           : {:?}", table.rows(19));
            println!("painted pixels     : {painted}");
            println!("pixel hash (fnv1a) : {:016x}", fnv1a(surface.pixels()));
            println!("wrote              : {}", out.display());
        }
        "demo" => {
            let table = load_table().expect("evidence/ball/tables.raw missing");
            let palette = load_palette().expect("evidence/ball/ball-input.raw missing");

            // Contact sheet: the rasterizer's primitive across the diameter
            // range the table supports, drawn with the game's own palette.
            let (w, h) = (640usize, 400usize);
            let mut surface = Surface::new(w, h);
            // Background: a mid palette entry rather than black, so silhouettes read.
            for y in 0..h {
                for x in 0..w {
                    surface.set(x, y, 12);
                }
            }

            let diameters = [12usize, 19, 27, 38, 52, 70, 92, 120, 155, 190, 230];
            let mut cx = 70i32;
            let mut cy = 90i32;
            let mut per_row = 0;
            for d in diameters {
                fill_disc(&mut surface, &table, cx, cy, d, 80);
                cx += 100;
                per_row += 1;
                if per_row == 5 { cx = 70; cy = 250; }
            }

            let rgb = palette.to_rgb(&surface);
            let out = args.get(1).map(PathBuf::from)
                .unwrap_or_else(|| repo_root().join("build/demo.ppm"));
            if let Some(parent) = out.parent() { let _ = fs::create_dir_all(parent); }
            write_ppm(&out, w, h, &rgb).expect("write ppm");
            println!("diameters drawn : {:?}", diameters);
            println!("pixel hash      : {:016x}", fnv1a(surface.pixels()));
            println!("wrote           : {}", out.display());
        }
        "compare" => {
            // Captured ball vs our render, side by side and enlarged.
            let table = load_table().expect("tables.raw missing");
            let palette = load_palette().expect("ball-input.raw missing");

            let raw = fs::read(repo_root().join("evidence/ball/ball-input.raw"))
                .expect("read capture");
            let word = |i: usize| u32::from_le_bytes(raw[i*4..i*4+4].try_into().unwrap());
            let (stride, min_x, min_y) = (word(3) as usize, word(4) as usize, word(5) as usize);
            let (bw, bh) = (word(6) as usize, word(7) as usize);
            let header = raw.len() - (1024 + 512 + 2 * stride * (word(1) as usize));
            let after_off = header + 1024 + 512 + stride * (word(1) as usize);

            let scale = 8usize;
            let panel_w = bw * scale;
            let (w, h) = (panel_w * 2 + 16, bh * scale);
            let mut out = Surface::new(w, h);
            for y in 0..h { for x in 0..w { out.set(x, y, 12); } }

            // Left panel: the real capture.
            for y in 0..bh {
                for x in 0..bw {
                    let px = raw[after_off + (min_y + y) * stride + min_x + x];
                    for sy in 0..scale { for sx in 0..scale {
                        out.set(x*scale+sx, y*scale+sy, px);
                    }}
                }
            }

            // Right panel: our rasterizer, same diameter, same centre.
            let mut ours = Surface::new(bw, bh);
            fill_disc(&mut ours, &table, 9, 9, 19, 18);
            for y in 0..bh {
                for x in 0..bw {
                    let px = ours.get(x, y).unwrap_or(12);
                    for sy in 0..scale { for sx in 0..scale {
                        out.set(panel_w + 16 + x*scale+sx, y*scale+sy, px);
                    }}
                }
            }

            let rgb = palette.to_rgb(&out);
            let path = args.get(1).map(PathBuf::from)
                .unwrap_or_else(|| repo_root().join("build/compare.ppm"));
            if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
            write_ppm(&path, w, h, &rgb).expect("write ppm");
            println!("left: captured   right: petz-core fill_disc (same palette)");
            println!("wrote: {}", path.display());
        }
        "frame" => {
            // Render one captured animation frame with our own rasterizer.
            let table = load_table().expect("tables.raw missing");
            let palette = load_palette().expect("ball-input.raw missing");
            let path = repo_root().join("build/animation.petz");
            let bytes = fs::read(&path).expect("build/animation.petz - run tools/export-frames.py");
            let mut p = 4usize;
            let rd_u32 = |p: &mut usize| -> u32 {
                let v = u32::from_le_bytes(bytes[*p..*p+4].try_into().unwrap()); *p += 4; v
            };
            let _ver = rd_u32(&mut p);
            let aw = rd_u32(&mut p) as usize;
            let ah = rd_u32(&mut p) as usize;
            let frame_count = rd_u32(&mut p) as usize;
            let want: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(frame_count / 2);
            let want = want.min(frame_count.saturating_sub(1));

            for idx in 0..=want {
                let nballs = rd_u32(&mut p) as usize;
                let start = p;
                p += nballs * 12;
                if idx != want { continue; }

                let (w, h) = (aw, ah);
                let mut surface = Surface::new(w, h);
                for y in 0..h { for x in 0..w { surface.set(x, y, 12); } }
                for b in 0..nballs {
                    let o = start + b * 12;
                    let cx = i16::from_le_bytes(bytes[o..o+2].try_into().unwrap()) as i32;
                    let cy = i16::from_le_bytes(bytes[o+2..o+4].try_into().unwrap()) as i32;
                    let dw = u16::from_le_bytes(bytes[o+4..o+6].try_into().unwrap()) as usize;
                    let colour = bytes[o+8];
                    let kind = bytes[o+9];
                    if dw == 0 || dw > table.max_diameter() { continue; }
                    if kind == 3 {
                        petz_core::fill_ring(&mut surface, &table, cx, cy, dw, 1, colour);
                    } else {
                        fill_disc(&mut surface, &table, cx, cy, dw, colour);
                    }
                }
                let rgb = palette.to_rgb(&surface);
                let out = repo_root().join(format!("build/frame-{want}.ppm"));
                write_ppm(&out, w, h, &rgb).expect("write ppm");
                println!("frame {want}/{frame_count}: {nballs} balls -> {}", out.display());
                break;
            }
        }
        "lnz" => {
            let path = args.get(1).expect("usage: petz-headless lnz FILE");
            let text = fs::read_to_string(path).expect("read lnz");
            let lnz = Lnz::parse(&text);
            println!("{} sections", lnz.len());
            for name in lnz.section_names() {
                println!("  {name}");
            }
            if let Some(fur) = lnz.section("Ballz Info") {
                let rows = fur.numbers();
                println!("\nBallz Info: {} rows, first = {:?}", rows.len(),
                         rows.first());
            }
        }
        other => {
            eprintln!("unknown task {other:?}; expected 'ball' or 'lnz FILE'");
            std::process::exit(2);
        }
    }
}
