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
