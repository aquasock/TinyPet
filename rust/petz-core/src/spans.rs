//! The ball rasterizer.
//!
//! A ball's silhouette is not computed geometry - it is a lookup. The engine
//! builds a table at startup holding, for every diameter `d`, the `d` row
//! widths of a disc at offset `(d-1)*d/2`.
//!
//! This was verified against captured pixels: a captured 19-pixel ball is
//! exactly `rows(19)`, centred, with 285 of 285 expected span pixels accounted
//! for. See `tests/ball_silhouette.rs`.

/// The static per-diameter row-width table, read verbatim from the engine.
#[derive(Clone, Debug)]
pub struct SpanTable {
    bytes: Vec<u8>,
}

impl SpanTable {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Offset of the row-width block for `diameter`.
    #[inline]
    pub fn offset(diameter: usize) -> usize {
        if diameter == 0 { 0 } else { (diameter - 1) * diameter / 2 }
    }

    /// Row widths for a disc of `diameter`, top row first.
    pub fn rows(&self, diameter: usize) -> &[u8] {
        if diameter == 0 { return &[]; }
        let start = Self::offset(diameter);
        let end = (start + diameter).min(self.bytes.len());
        if start >= end { return &[]; }
        &self.bytes[start..end]
    }

    /// Largest diameter the table can describe.
    pub fn max_diameter(&self) -> usize {
        let mut d = 0;
        while Self::offset(d + 1) + (d + 1) <= self.bytes.len() {
            d += 1;
        }
        d
    }
}

/// Where a disc of `diameter` centred at `(cx, cy)` puts row `row`.
///
/// Centring is integer: `left = cx - (width-1)/2`. That rule reproduced the
/// captured ball exactly, including rows where the width is odd.
#[inline]
pub fn row_span(cx: i32, cy: i32, diameter: usize, row: usize, width: u8) -> (i32, i32, usize) {
    let top = cy - ((diameter as i32 - 1) / 2);
    let left = cx - ((width as i32 - 1) / 2);
    (top + row as i32, left, width as usize)
}

/// Fill a disc using the table. This is the primitive the renderer is built on.
pub fn fill_disc(
    surface: &mut crate::surface::Surface,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    colour: u8,
) {
    for (row, &width) in table.rows(diameter).iter().enumerate() {
        if width == 0 { continue; }
        let (y, left, len) = row_span(cx, cy, diameter, row, width);
        if y < 0 { continue; }
        surface.fill_span(y as usize, left, len, colour);
    }
}

/// As `fill_disc`, but `keep` is asked about every pixel first. Used to clip a
/// ball against another sphere per pixel, which a painter's algorithm cannot do
/// from centres alone.
pub fn fill_disc_masked(
    surface: &mut crate::surface::Surface,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    colour: u8,
    keep: &dyn Fn(i32, i32) -> bool,
) {
    for (row, &width) in table.rows(diameter).iter().enumerate() {
        if width == 0 {
            continue;
        }
        let (y, left, len) = row_span(cx, cy, diameter, row, width);
        if y < 0 {
            continue;
        }
        for i in 0..len as i32 {
            let x = left + i;
            if keep(x, y) {
                surface.set(x.max(0) as usize, y as usize, colour);
            }
        }
    }
}

/// Draw a ring of `thickness` pixels inside the silhouette of a disc.
///
/// `XFillOutlineCircle` receives the same render block as the body fill - same
/// rect, same centre - so the outline cannot be a disc drawn first or last; it
/// must be the rim. Drawing outer spans and skipping the interior reproduces
/// that.
pub fn fill_ring(
    surface: &mut crate::surface::Surface,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    thickness: usize,
    colour: u8,
) {
    let rows = table.rows(diameter);
    let t = thickness.max(1);
    for (row, &width) in rows.iter().enumerate() {
        if width == 0 { continue; }
        let (y, left, len) = row_span(cx, cy, diameter, row, width);
        if y < 0 { continue; }
        let w = len as i32;
        // Top and bottom rows are solid across the span.
        let solid = row < t || row + t >= rows.len() || (w as usize) <= 2 * t;
        if solid {
            surface.fill_span(y as usize, left, len, colour);
        } else {
            surface.fill_span(y as usize, left, t, colour);
            surface.fill_span(y as usize, left + w - t as i32, t, colour);
        }
    }
}
