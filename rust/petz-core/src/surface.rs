//! An 8bpp indexed drawing surface and its palette.
//!
//! The retail renderer writes palette indices into a DIB and resolves the
//! palette only at presentation. We keep the same split so that every
//! comparison against captured frames is made on indices, not on RGB - that is
//! where bit-exactness lives.

/// An 8bpp indexed surface.
#[derive(Clone, Debug)]
pub struct Surface {
    width: usize,
    height: usize,
    stride: usize,
    pixels: Vec<u8>,
}

impl Surface {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            stride: width,
            pixels: vec![0; width * height],
        }
    }

    pub fn from_parts(width: usize, height: usize, stride: usize, pixels: Vec<u8>) -> Self {
        assert!(pixels.len() >= stride * height, "pixel buffer too small");
        Self { width, height, stride, pixels }
    }

    pub fn width(&self) -> usize { self.width }
    pub fn height(&self) -> usize { self.height }
    pub fn stride(&self) -> usize { self.stride }
    pub fn pixels(&self) -> &[u8] { &self.pixels }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        if x >= self.width || y >= self.height { return None; }
        Some(self.pixels[y * self.stride + x])
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, value: u8) {
        if x < self.width && y < self.height {
            self.pixels[y * self.stride + x] = value;
        }
    }

    /// Fill `[x, x + len)` on row `y` with `value`. Clipped.
    #[inline]
    pub fn fill_span(&mut self, y: usize, x: i32, len: usize, value: u8) {
        if y >= self.height || len == 0 { return; }
        let start = x.max(0) as usize;
        let end = ((x + len as i32).max(0) as usize).min(self.width);
        if start >= end { return; }
        let row = y * self.stride;
        for px in &mut self.pixels[row + start..row + end] {
            *px = value;
        }
    }
}

/// A 256-entry palette, stored the way Windows stores it (RGBQUAD: B, G, R, 0).
#[derive(Clone, Debug)]
pub struct Palette {
    entries: [[u8; 3]; 256],
}

impl Palette {
    pub fn from_rgbquad(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 1024, "palette needs 256 RGBQUADs");
        let mut entries = [[0u8; 3]; 256];
        for (i, e) in entries.iter_mut().enumerate() {
            let q = &bytes[i * 4..i * 4 + 4];
            *e = [q[2], q[1], q[0]];
        }
        Self { entries }
    }

    pub fn rgb(&self, index: u8) -> [u8; 3] {
        self.entries[index as usize]
    }

    /// Resolve a surface to packed RGB. Nearest-neighbour, integer only - this
    /// is what goes to the screen, and it must not alter the indices' meaning.
    pub fn to_rgb(&self, surface: &Surface) -> Vec<u8> {
        let mut out = Vec::with_capacity(surface.width() * surface.height() * 3);
        for y in 0..surface.height() {
            for x in 0..surface.width() {
                let rgb = self.rgb(surface.get(x, y).unwrap_or(0));
                out.extend_from_slice(&rgb);
            }
        }
        out
    }
}
