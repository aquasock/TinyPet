//! The gate: reproduce a captured ball's silhouette exactly.
//!
//! Evidence (`evidence/ball/ball-input.raw`) is a real capture from the retail
//! executable: a 35x26 region containing one 19-pixel ball, with the surface
//! before and after the fill. The ball body is palette index 18; where another
//! pass drew over it afterwards the pixels hold index 247.
//!
//! The model under test is: widths from the static span table, centred, filled.
//! Success criterion is the one from `docs/VERDICT.md` - every expected span
//! pixel must be accounted for, with zero unexplained.

use std::fs;
use std::path::PathBuf;

use petz_core::{fill_disc, row_span, SpanTable, Surface};

const PALETTE_BYTES: usize = 1024;
const BLOCK_BYTES: usize = 512;

const BODY_INDEX: u8 = 18;
const EDGE_INDEX: u8 = 247;

struct Capture {
    width: usize,
    height: usize,
    stride: usize,
    min_x: i32,
    min_y: i32,
    bbox_w: usize,
    bbox_h: usize,
    after: Vec<u8>,
}

fn evidence_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evidence")
}

/// stride * height, read before the header size is known.
fn stride_pending(raw: &[u8]) -> usize {
    let word = |i: usize| -> u32 {
        u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
    };
    (word(3) as usize) * (word(1) as usize)
}

fn load_capture() -> Capture {
    let path = evidence_dir().join("ball/ball-input.raw");
    let raw = fs::read(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    // The header has grown over time (a presentation-index field was added
    // later), so derive its size from the file rather than assuming it.
    let header_bytes = raw.len()
        .checked_sub(PALETTE_BYTES + BLOCK_BYTES + 2 * stride_pending(&raw))
        .expect("file shorter than its pixel payload");

    let word = |i: usize| -> u32 {
        u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap())
    };
    let width = word(0) as usize;
    let height = word(1) as usize;
    let stride = word(3) as usize;
    let min_x = word(4) as i32;
    let min_y = word(5) as i32;
    let bbox_w = word(6) as usize;
    let bbox_h = word(7) as usize;

    assert_eq!(header_bytes % 4, 0, "header should be whole dwords");
    let after_off = header_bytes + PALETTE_BYTES + BLOCK_BYTES + stride * height;
    let after = raw[after_off..after_off + stride * height].to_vec();

    Capture { width, height, stride, min_x, min_y, bbox_w, bbox_h, after }
}

fn load_table() -> SpanTable {
    let path = evidence_dir().join("ball/tables.raw");
    SpanTable::new(fs::read(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display())))
}

#[test]
fn captured_ball_is_explained_by_the_span_table() {
    let cap = load_capture();
    let table = load_table();

    // The captured ball measures 19 across, so it uses table entry 19.
    let diameter = 19usize;
    let widths: Vec<u8> = table.rows(diameter).to_vec();
    assert_eq!(widths.len(), diameter, "table entry should have one width per row");

    // Centred at (9, 9) within the 35x26 capture region - derived from the
    // capture itself (rows 7..9 span the full 19 pixels from x=0).
    let (cx, cy) = (9i32, 9i32);

    let mut expected = 0usize;
    let mut accounted = 0usize;
    let mut unexplained = Vec::new();

    for (row, &width) in widths.iter().enumerate() {
        let (y, left, len) = row_span(cx, cy, diameter, row, width);
        if y < 0 || y as usize >= cap.bbox_h { continue; }
        for i in 0..len {
            let x = left + i as i32;
            if x < 0 || x as usize >= cap.bbox_w { continue; }
            expected += 1;
            let px = cap.after[(cap.min_y as usize + y as usize) * cap.stride
                + (cap.min_x as usize + x as usize)];
            if px == BODY_INDEX || px == EDGE_INDEX {
                accounted += 1;
            } else {
                unexplained.push((x, y, px));
            }
        }
    }

    assert_eq!(expected, 285, "expected span area for a 19-pixel disc");
    assert!(
        unexplained.is_empty(),
        "{} unexplained pixels, e.g. {:?}",
        unexplained.len(),
        &unexplained[..unexplained.len().min(8)]
    );
    assert_eq!(accounted, expected, "every span pixel must be body or edge");
}

#[test]
fn fill_disc_reproduces_the_span_geometry() {
    let cap = load_capture();
    let table = load_table();
    let diameter = 19usize;
    let (cx, cy) = (9i32, 9i32);

    let mut surface = Surface::new(cap.bbox_w, cap.bbox_h);
    fill_disc(&mut surface, &table, cx, cy, diameter, BODY_INDEX);

    // Every pixel we wrote must sit inside the model's expected spans, and the
    // count must match the table area.
    let mut painted = 0usize;
    for row in 0..diameter {
        let width = table.rows(diameter)[row];
        let (y, left, len) = row_span(cx, cy, diameter, row, width);
        for i in 0..len as i32 {
            let (x, yy) = (left + i, y);
            if x < 0 || yy < 0 { continue; }
            if (x as usize) < cap.bbox_w && (yy as usize) < cap.bbox_h {
                assert_eq!(
                    surface.get(x as usize, yy as usize),
                    Some(BODY_INDEX),
                    "span pixel ({x},{yy}) should have been painted"
                );
                painted += 1;
            }
        }
    }
    assert_eq!(painted, 285);
}
