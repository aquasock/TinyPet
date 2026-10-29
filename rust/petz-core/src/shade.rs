//! Sphere shading: turn a flat disc into something that reads as round.
//!
//! The silhouette was already solved - a ball's outline is a row-width lookup.
//! What was missing is the third dimension, and for a disc it is free: a point
//! at offset `(dx, dy)` inside a circle of radius `r` sits at height
//! `sqrt(r^2 - dx^2 - dy^2)`, so the surface normal is known analytically. No
//! model file, no mesh, no ray marching.
//!
//! The original engine does *not* do this. `docs/BINARY_NOTES.md` records that
//! a ball's RGB colour is a single palette index, and that the on-screen
//! variation is a small texture being sampled as a shade/height map and
//! offsetting that index. Shading and fur are the same signal there, which is
//! why the fur looks painted on: it is.
//!
//! We separate them. Light decides *which band* of a ramp a pixel falls in;
//! the fur texture will later decide *which ramp*. That way lighting is
//! independent of the pattern and vice versa, and neither is baked into art.
//!
//! Output stays in palette indices, so the result is still an 8bpp surface and
//! still renders through the same table-driven primitives.

use crate::spans::SpanTable;
use crate::surface::{Palette, Surface};

/// A direction. Only unit-length direction matters here; there is no position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn normalised(self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        if len < 1e-6 {
            return Self::new(0.0, 0.0, 1.0);
        }
        Self::new(self.x / len, self.y / len, self.z / len)
    }

    /// Build a direction from two angles in degrees: `yaw` turns the light
    /// left/right, `pitch` raises it. Convenient for key-driven tuning.
    pub fn from_angles(yaw_deg: f32, pitch_deg: f32) -> Self {
        let (y, p) = (
            yaw_deg.to_radians(),
            pitch_deg.to_radians(),
        );
        Self::new(p.cos() * y.sin(), -p.sin(), p.cos() * y.cos()).normalised()
    }

    #[inline]
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
}

/// A dark-to-light run of palette indices. Every pixel of a shaded ball lands
/// on one of these, which is what keeps the result looking like the original's
/// banded art rather than a smooth gradient.
#[derive(Clone, Debug)]
pub struct Ramp {
    pub indices: Vec<u8>,
}

impl Ramp {
    pub fn new(indices: Vec<u8>) -> Self {
        assert!(!indices.is_empty(), "a ramp needs at least one index");
        Self { indices }
    }

    /// Pick the band for a shade value in `0..=1`. Quantised on purpose: the
    /// whole point is a small number of visible steps.
    #[inline]
    pub fn pick(&self, t: f32) -> u8 {
        let n = self.indices.len();
        if n == 1 {
            return self.indices[0];
        }
        let t = t.clamp(0.0, 1.0);
        let i = (t * (n - 1) as f32).round() as usize;
        self.indices[i.min(n - 1)]
    }

    /// Derive a ramp from the game palette by taking indices of the same hue
    /// as `base`, ordered by luminance, and sampling `steps` of them evenly.
    ///
    /// This is how we get a plausible material ladder without inventing
    /// colours: the palette already contains graded runs, one per material.
    pub fn from_palette(pal: &Palette, base: u8, steps: usize) -> Self {
        let steps = steps.max(1);
        let (bh, bs, bv) = hsv(pal.rgb(base));
        let bl = luma(pal.rgb(base)).max(1.0);

        let mut candidates: Vec<(f32, u8)> = (0..=255u8)
            .filter_map(|i| {
                let [r, g, b] = pal.rgb(i);
                if r == 0 && g == 0 && b == 0 {
                    return None;
                }
                let (h, s, v) = hsv([r, g, b]);
                // Same family: close in hue, and not a wildly different
                // saturation. Grays have no meaningful hue, so treat them by
                // saturation instead.
                let dh = if bs < 0.12 || s < 0.12 {
                    if s >= 0.12 { return None; }
                    0.0
                } else {
                    let d = (h - bh).abs();
                    d.min(1.0 - d)
                };
                if dh > 0.035 || (s - bs).abs() > 0.45 {
                    return None;
                }
                // Keep the ladder inside a plausible material range around the
                // base. Without this the ramp runs all the way to the palette's
                // near-white highlight of the same hue, which reads as glare on
                // a fur ball rather than as light.
                let l = luma([r, g, b]);
                if l < bl * 0.42 || l > bl * 1.28 {
                    return None;
                }
                let _ = bv;
                Some((l, i))
            })
            .collect();

        if candidates.len() < steps {
            // Not enough of this material in the palette: fall back to the base
            // held flat rather than faking a gradient.
            return Self::new(vec![base; steps]);
        }
        candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let mut picked = Vec::with_capacity(steps);
        for k in 0..steps {
            let pos = k * (candidates.len() - 1) / (steps - 1).max(1);
            picked.push(candidates[pos].1);
        }
        // Keep the artist's chosen colour exactly where it belongs in the
        // ladder, rather than trusting the sampled neighbour.
        let target = luma(pal.rgb(base));
        let closest = picked
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let la = (luma(pal.rgb(**a)) - target).abs();
                let lb = (luma(pal.rgb(**b)) - target).abs();
                la.partial_cmp(&lb).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        picked[closest] = base;
        Self::new(picked)
    }
}

fn luma([r, g, b]: [u8; 3]) -> f32 {
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// Hue in 0..1, saturation 0..1, value 0..1.
fn hsv([r, g, b]: [u8; 3]) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 1e-6 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max < 1e-6 { 0.0 } else { d / max };
    (h, s, max)
}

/// How a ball is lit.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    /// Direction *towards* the light, unit length.
    pub dir: Vec3,
    /// Light present even on the side facing away. Keeps the dark side from
    /// collapsing into a single flat band, which reads as a hole.
    pub ambient: f32,
    /// How hard the light term pushes toward the bright end.
    pub diffuse: f32,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            dir: Vec3::from_angles(-35.0, 40.0),
            ambient: 0.28,
            diffuse: 0.72,
        }
    }
}

/// Fill a disc with ramp indices chosen from its own surface normals.
///
/// The silhouette still comes from the span table - this changes only which
/// index each pixel gets, so a shaded ball is exactly as circular as a flat one.
pub fn shade_disc(
    surface: &mut Surface,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    ramp: &Ramp,
    light: Light,
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

    for (row, &width) in rows.iter().enumerate() {
        if width == 0 {
            continue;
        }
        let y = top + row as i32;
        if y < 0 || y as usize >= surface.height() {
            continue;
        }
        let left = cx - (width as i32 - 1) / 2;
        // Row centre in normalised sphere coordinates.
        let dy = (y as f32 + 0.5 - (cy as f32 + 0.5)) * inv_r;
        for i in 0..width as i32 {
            let x = left + i;
            if x < 0 || x as usize >= surface.width() {
                continue;
            }
            let dx = (x as f32 + 0.5 - (cx as f32 + 0.5)) * inv_r;
            // The table's row widths can reach a fraction of a pixel past the
            // analytic circle, so clamp instead of skipping: coverage must stay
            // identical to the proven flat fill.
            let nz = (1.0 - dx * dx - dy * dy).max(0.0).sqrt();
            let lambert = (dx * light.dir.x + dy * light.dir.y + nz * light.dir.z).max(0.0);
            let t = light.ambient + light.diffuse * lambert;
            surface.set(x as usize, y as usize, ramp.pick(t));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn table() -> SpanTable {
        SpanTable::new(
            fs::read(repo_root().join("evidence/ball/tables.raw")).expect("span table"),
        )
    }

    fn palette() -> Palette {
        let raw = fs::read(repo_root().join("evidence/ball/ball-input.raw")).expect("palette");
        let stride = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as usize;
        let height = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
        let header = raw.len() - (1024 + 512 + 2 * stride * height);
        Palette::from_rgbquad(&raw[header..header + 1024])
    }

    /// A ramp of greys with known luminance order, for checking band placement.
    fn grey_ramp() -> Ramp {
        Ramp::new(vec![30, 31, 32, 33, 34, 35, 36, 37])
    }

    #[test]
    fn a_ramp_derived_from_the_palette_runs_dark_to_light() {
        let pal = palette();
        let ramp = Ramp::from_palette(&pal, 61, 6);
        let lum: Vec<f32> = ramp.indices.iter().map(|i| luma(pal.rgb(*i))).collect();
        assert!(
            lum.windows(2).all(|w| w[0] <= w[1] + 0.01),
            "ramp is not ordered: {lum:?} from {:?}",
            ramp.indices
        );
        assert!(
            ramp.indices.contains(&61),
            "the artist's base colour must survive: {:?}",
            ramp.indices
        );
    }

    #[test]
    fn the_centre_of_a_ball_is_brighter_than_its_edge() {
        let table = table();
        let ramp = grey_ramp();
        let light = Light {
            dir: Vec3::new(0.0, 0.0, 1.0), // straight at the viewer
            ..Light::default()
        };
        let mut s = Surface::new(64, 64);
        shade_disc(&mut s, &table, 32, 32, 40, &ramp, light);
        let centre = s.get(32, 32).unwrap();
        let edge = s.get(32 + 18, 32).unwrap();
        assert!(
            ramp.indices.iter().position(|&c| c == centre)
                > ramp.indices.iter().position(|&c| c == edge),
            "centre {centre} should sit higher in the ramp than edge {edge}"
        );
    }

    #[test]
    fn moving_the_light_moves_the_bright_side() {
        let table = table();
        let ramp = grey_ramp();

        let bright_side = |dx: f32| {
            let light = Light {
                dir: Vec3::new(dx, 0.0, 0.4).normalised(),
                ..Light::default()
            };
            let mut s = Surface::new(64, 64);
            shade_disc(&mut s, &table, 32, 32, 40, &ramp, light);
            let left = ramp.indices.iter().position(|&c| c == s.get(24, 32).unwrap());
            let right = ramp.indices.iter().position(|&c| c == s.get(40, 32).unwrap());
            (left.unwrap(), right.unwrap())
        };

        let (l1, r1) = bright_side(-0.8); // light from the left
        assert!(l1 > r1, "light on the left should brighten the left side");
        let (l2, r2) = bright_side(0.8); // light from the right
        assert!(r2 > l2, "light on the right should brighten the right side");
    }

    #[test]
    fn every_pixel_comes_from_the_ramp() {
        let table = table();
        let pal = palette();
        let ramp = Ramp::from_palette(&pal, 61, 6);
        let mut s = Surface::new(64, 64);
        shade_disc(&mut s, &table, 32, 32, 40, &ramp, Light::default());
        for y in 0..64 {
            for x in 0..64 {
                let c = s.get(x, y).unwrap();
                if c != 0 {
                    assert!(ramp.indices.contains(&c), "index {c} is not in the ramp");
                }
            }
        }
    }

    #[test]
    fn the_shaded_silhouette_is_the_same_as_the_flat_one() {
        // Shading must not change which pixels are covered - that geometry is
        // already proven against a captured ball.
        let table = table();
        let ramp = grey_ramp();
        let mut shaded = Surface::new(64, 64);
        let mut flat = Surface::new(64, 64);
        crate::spans::fill_disc(&mut flat, &table, 32, 32, 40, 99);
        shade_disc(&mut shaded, &table, 32, 32, 40, &ramp, Light::default());
        for y in 0..64 {
            for x in 0..64 {
                assert_eq!(
                    flat.get(x, y).unwrap() == 99,
                    shaded.get(x, y).unwrap() != 0,
                    "silhouette differs at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn shading_is_deterministic() {
        let table = table();
        let ramp = grey_ramp();
        let run = || {
            let mut s = Surface::new(64, 64);
            shade_disc(&mut s, &table, 32, 32, 40, &ramp, Light::default());
            s.pixels().to_vec()
        };
        assert_eq!(run(), run());
    }
}
