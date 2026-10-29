//! Fur: a pattern that selects material, not light.
//!
//! The original samples a small 8bpp texture and uses the texel to offset a
//! ball's palette index, which is why its fur looks painted on — the texture
//! carries the shading as well as the pattern, so a fur that reads well under
//! one light reads wrong under another, and an artist has to paint the light
//! into the art.
//!
//! Here the texture does one job: it chooses *which ramp* a pixel draws from.
//! Light decides the band within that ramp, exactly as it does for a plain
//! ball. The two stay independent, so the same dapple works at any light angle
//! and the same light works on any pattern.
//!
//! Sampling is integer and wrapping, matching the original's constraints
//! (`docs/BINARY_NOTES.md`: divisor 128, wrap by texture width, no filtering).

use crate::shade::{Light, Ramp};
use crate::spans::SpanTable;
use crate::surface::Surface;

/// An 8bpp pattern. Indices, not RGB: this is a mask that picks between ramps.
#[derive(Clone, Debug)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Texture {
    pub fn new(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        assert_eq!(width * height, pixels.len(), "texture size mismatch");
        Self { width, height, pixels }
    }

    /// A texture that selects nothing, so a patterned ball renders identically
    /// to a plain one.
    pub fn flat(width: usize, height: usize, value: u8) -> Self {
        Self::new(width, height, vec![value; width * height])
    }

    #[inline]
    pub fn sample(&self, u: usize, v: usize) -> u8 {
        let x = u % self.width;
        let y = v % self.height;
        self.pixels[y * self.width + x]
    }
}

/// A material plus a pattern. Pixels where the texel is at or above
/// `threshold` take the alternate ramp.
#[derive(Clone, Debug)]
pub struct Pattern {
    pub base: Ramp,
    pub alt: Ramp,
    pub texture: Texture,
    pub threshold: u8,
    /// Rotation of the pattern around the ball, in texture widths. The original
    /// has a per-ball angle for exactly this.
    pub phase: f32,
}

impl Pattern {
    pub fn plain(ramp: Ramp, texture: Texture) -> Self {
        Self {
            alt: ramp.clone(),
            base: ramp,
            texture,
            threshold: 0,
            phase: 0.0,
        }
    }
}

/// Fill a disc shaded by the light, with a pattern choosing the material.
///
/// Same silhouette as `fill_disc`, same lighting as `shade_disc`; the only new
/// input is the texel.
pub fn shade_disc_pattern(
    surface: &mut Surface,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    light: Light,
    pattern: &Pattern,
) {
    if diameter == 0 {
        return;
    }
    let rows = table.rows(diameter);
    if rows.is_empty() {
        return;
    }
    let r = diameter as f32 / 2.0;
    let inv_r = 1.0 / r;
    let top = cy - (diameter as i32 - 1) / 2;
    let (tw, th) = (pattern.texture.width as f32, pattern.texture.height as f32);
    let inv_pi = 1.0 / std::f32::consts::PI;

    for (row, &width) in rows.iter().enumerate() {
        if width == 0 {
            continue;
        }
        let y = top + row as i32;
        if y < 0 || y as usize >= surface.height() {
            continue;
        }
        let left = cx - (width as i32 - 1) / 2;
        let dy = (y as f32 + 0.5 - (cy as f32 + 0.5)) * inv_r;
        // Latitude: -1 at the top pole, +1 at the bottom.
        let v = ((dy.asin() * inv_pi) + 0.5) * th;

        for i in 0..width as i32 {
            let x = left + i;
            if x < 0 || x as usize >= surface.width() {
                continue;
            }
            let dx = (x as f32 + 0.5 - (cx as f32 + 0.5)) * inv_r;
            let nz = (1.0 - dx * dx - dy * dy).max(0.0).sqrt();

            // Longitude across the visible hemisphere, using the full texture
            // width: atan2 spans -pi/2..pi/2 on the front face.
            let u = ((dx.atan2(nz) * inv_pi) + 0.5 + pattern.phase).rem_euclid(1.0) * tw;

            let texel = pattern.texture.sample(u as usize, v as usize);
            let ramp = if texel >= pattern.threshold { &pattern.base } else { &pattern.alt };

            let lambert = (dx * light.dir.x + dy * light.dir.y + nz * light.dir.z).max(0.0);
            let t = light.ambient + light.diffuse * lambert;
            surface.set(x as usize, y as usize, ramp.pick(t));
        }
    }
}

/// Procedural fur, so nothing in the repository is game art.
///
/// A hash-based dapple: cheap, deterministic, and enough to prove the material
/// mechanism works end to end. Real patterns will be authored later.
pub fn dapple(width: usize, height: usize, seed: u32, cover: u8) -> Texture {
    let mut pixels = vec![0u8; width * height];
    for y in 0..height {
        for x in 0..width {
            // Wrap around both axes so the pattern tiles without a seam.
            let u = x as f32 / width as f32 * std::f32::consts::TAU;
            let v = y as f32 / height as f32 * std::f32::consts::TAU;
            let n = (u.sin() * 1.7 + v.cos() * 1.3
                + (u * 2.0 + v).sin() * 0.8
                + (v * 3.0 - u).cos() * 0.5)
                * 40.0
                + seed as f32;
            pixels[y * width + x] = if n.sin() > 0.15 { cover } else { 0 };
        }
    }
    Texture::new(width, height, pixels)
}

/// Horizontal banding, useful for judging how a pattern wraps around a ball.
pub fn stripes(width: usize, height: usize, period: usize, cover: u8) -> Texture {
    let mut pixels = vec![0u8; width * height];
    for y in 0..height {
        let on = (y / period.max(1)) % 2 == 0;
        for x in 0..width {
            pixels[y * width + x] = if on { cover } else { 0 };
        }
    }
    Texture::new(width, height, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shade::{shade_disc, Vec3};
    use std::fs;
    use std::path::PathBuf;

    fn table() -> SpanTable {
        SpanTable::new(fs::read(repo_root().join("evidence/ball/tables.raw")).unwrap())
    }

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn ramps() -> (Ramp, Ramp) {
        (
            Ramp::new(vec![30, 31, 32, 33, 34, 35, 36, 37]),
            Ramp::new(vec![90, 91, 92, 93, 94, 95, 96, 97]),
        )
    }

    #[test]
    fn sampling_wraps_in_both_axes() {
        let t = Texture::new(2, 2, vec![1, 2, 3, 4]);
        assert_eq!(t.sample(0, 0), 1);
        assert_eq!(t.sample(1, 0), 2);
        assert_eq!(t.sample(2, 0), 1);
        assert_eq!(t.sample(0, 3), 3);
        assert_eq!(t.sample(5, 7), 4);
    }

    #[test]
    fn a_flat_texture_renders_exactly_like_a_plain_ball() {
        let table = table();
        let (base, alt) = ramps();
        let light = Light::default();
        // Both ramps identical, so the pattern cannot matter: any difference
        // would be a bug in the per-pixel maths, not in the material choice.
        let pattern = Pattern {
            base: base.clone(),
            alt: base.clone(),
            texture: Texture::flat(16, 16, 0),
            threshold: 128,
            phase: 0.0,
        };
        let mut plain = Surface::new(64, 64);
        let mut furred = Surface::new(64, 64);
        shade_disc(&mut plain, &table, 32, 32, 40, &base, light);
        shade_disc_pattern(&mut furred, &table, 32, 32, 40, light, &pattern);
        assert_eq!(plain.pixels(), furred.pixels(), "a no-op texture must be a no-op");
    }

    #[test]
    fn a_pattern_uses_both_ramps() {
        let table = table();
        let (base, alt) = ramps();
        let pattern = Pattern {
            base: base.clone(),
            alt: alt.clone(),
            texture: dapple(32, 32, 7, 255),
            threshold: 128,
            phase: 0.0,
        };
        let mut s = Surface::new(64, 64);
        shade_disc_pattern(&mut s, &table, 32, 32, 40, Light::default(), &pattern);
        let used: Vec<u8> = s.pixels().iter().copied().filter(|c| *c != 0).collect();
        assert!(used.iter().any(|c| base.indices.contains(c)), "base ramp unused");
        assert!(used.iter().any(|c| alt.indices.contains(c)), "alt ramp unused");
        assert!(
            used.iter().all(|c| base.indices.contains(c) || alt.indices.contains(c)),
            "a colour escaped both ramps"
        );
    }

    #[test]
    fn the_pattern_does_not_change_the_silhouette() {
        let table = table();
        let (base, alt) = ramps();
        let pattern = Pattern {
            base: base.clone(),
            alt,
            texture: dapple(32, 32, 3, 255),
            threshold: 128,
            phase: 0.0,
        };
        let mut flat = Surface::new(64, 64);
        crate::spans::fill_disc(&mut flat, &table, 32, 32, 40, 99);
        let mut furred = Surface::new(64, 64);
        shade_disc_pattern(&mut furred, &table, 32, 32, 40, Light::default(), &pattern);
        for y in 0..64 {
            for x in 0..64 {
                assert_eq!(
                    flat.get(x, y).unwrap() == 99,
                    furred.get(x, y).unwrap() != 0,
                    "silhouette differs at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn phase_scrolls_the_pattern_without_redrawing_the_ball() {
        let table = table();
        let (base, alt) = ramps();
        let mut a = Surface::new(64, 64);
        let mut b = Surface::new(64, 64);
        for (s, phase) in [(&mut a, 0.0f32), (&mut b, 0.35)] {
            let pattern = Pattern {
                base: base.clone(),
                alt: alt.clone(),
                texture: dapple(32, 32, 11, 255),
                threshold: 128,
                phase,
            };
            shade_disc_pattern(s, &table, 32, 32, 40, Light::default(), &pattern);
        }
        assert_ne!(a.pixels(), b.pixels(), "phase had no effect");
        // Coverage is unaffected by scrolling.
        let ca = a.pixels().iter().filter(|c| **c != 0).count();
        let cb = b.pixels().iter().filter(|c| **c != 0).count();
        assert_eq!(ca, cb);
    }

    #[test]
    fn lighting_still_works_through_a_pattern() {
        // The bright side must follow the light even where the pattern differs.
        let table = table();
        let light = Light {
            dir: Vec3::new(0.0, 0.0, 1.0),
            ..Light::default()
        };
        let (base, alt) = ramps();
        let pattern = Pattern {
            base: base.clone(),
            alt,
            // Threshold 0 sends every texel to the base ramp, isolating the
            // lighting from the pattern.
            texture: Texture::flat(8, 8, 0),
            threshold: 0,
            phase: 0.0,
        };
        let mut s = Surface::new(64, 64);
        shade_disc_pattern(&mut s, &table, 32, 32, 40, light, &pattern);
        let centre = base.indices.iter().position(|&c| c == s.get(32, 32).unwrap()).unwrap();
        let edge = base.indices.iter().position(|&c| c == s.get(50, 32).unwrap()).unwrap();
        assert!(centre > edge);
    }
}
