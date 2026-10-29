//! A depth buffer, and the one rasterizer every ball goes through.
//!
//! The original engine has no z-buffer: it paints balls in order, far to near,
//! and lets later balls cover earlier ones. That works, but it puts the whole
//! question of "what is in front" into the *sort*, and the sort is a scalar per
//! ball. A ball is not a plane - it bulges towards the viewer by its own radius
//! - so centre-depth ordering is wrong wherever two balls meet, which is
//! everywhere on a body made of overlapping spheres.
//!
//! Every bug we had in this area was that same mistake wearing a different hat:
//! an eye compared against a head's centre, two eyes sharing one sort key, a
//! muzzle painted flat over the skull it sits inside, a nose swallowed by a
//! containment test written to paper over the first three.
//!
//! Per-pixel depth fixes it once. It also removes the need for any of it: no
//! sort, no containment lists, no per-eye occlusion code. Balls can be drawn in
//! any order.

use crate::fur::Pattern;
use crate::shade::Light;
use crate::spans::{row_span, SpanTable};
use crate::surface::Surface;

/// Depth per pixel. Smaller is nearer the viewer.
#[derive(Clone, Debug)]
pub struct DepthBuffer {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl DepthBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, data: vec![f32::INFINITY; width * height] }
    }

    pub fn clear(&mut self) {
        self.data.fill(f32::INFINITY);
    }

    #[inline]
    pub fn test_and_set(&mut self, x: usize, y: usize, depth: f32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let i = y * self.width + x;
        if depth < self.data[i] {
            self.data[i] = depth;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }
}

/// How a disc is painted, once the depth test says a pixel is visible.
pub enum Paint<'a> {
    Flat(u8),
    Shaded { light: Light, pattern: &'a Pattern },
}

/// Draw one ball: a disc of `diameter`, with its sphere's surface depth, into
/// both the surface and the depth buffer.
///
/// `centre_depth` is the depth of the ball's centre. Every pixel's depth is
/// solved from the sphere rather than taken as the centre's, which is the whole
/// point: it is what lets a small ball sit in front of a large one at the same
/// centre, and what stops a ball from painting over the one containing it.
///
/// `bias` shifts the whole disc nearer (negative) or further (positive). Used
/// to put a pupil on its sclera and a lid on top of both, and to push a rim
/// just behind the ball it outlines.
pub fn draw_disc(
    surface: &mut Surface,
    depth: &mut DepthBuffer,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    centre_depth: f32,
    bias: f32,
    paint: Paint,
    keep: &dyn Fn(i32, i32) -> bool,
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
    let (h, w) = (surface.height(), surface.width());
    let inv_pi = 1.0 / std::f32::consts::PI;
    let (tw, th) = match paint {
        Paint::Shaded { pattern, .. } => {
            (pattern.texture.width as f32, pattern.texture.height as f32)
        }
        Paint::Flat(_) => (0.0, 0.0),
    };

    for (row, &width) in rows.iter().enumerate() {
        if width == 0 {
            continue;
        }
        let y = top + row as i32;
        if y < 0 || y as usize >= h {
            continue;
        }
        let left = cx - (width as i32 - 1) / 2;
        let dy = (y as f32 + 0.5 - (cy as f32 + 0.5)) * inv_r;
        let v = ((dy.asin() * inv_pi) + 0.5) * th;

        for i in 0..width as i32 {
            let x = left + i;
            if x < 0 || x as usize >= w {
                continue;
            }
            if !keep(x, y) {
                continue;
            }
            let dx = (x as f32 + 0.5 - (cx as f32 + 0.5)) * inv_r;
            let nz = (1.0 - dx * dx - dy * dy).max(0.0).sqrt();
            let pixel_depth = centre_depth - nz * r + bias;
            if !depth.test_and_set(x as usize, y as usize, pixel_depth) {
                continue;
            }
            let colour = match paint {
                Paint::Flat(c) => c,
                Paint::Shaded { light, pattern } => {
                    let u = ((dx.atan2(nz) * inv_pi) + 0.5 + pattern.phase).rem_euclid(1.0) * tw;
                    let texel = pattern.texture.sample(u as usize, v as usize);
                    let ramp = if texel >= pattern.threshold { &pattern.base } else { &pattern.alt };
                    let lambert = (dx * light.dir.x + dy * light.dir.y + nz * light.dir.z).max(0.0);
                    ramp.pick(light.ambient + light.diffuse * lambert)
                }
            };
            surface.set(x as usize, y as usize, colour);
        }
    }
}

/// A flat disc at a single depth. Used for rims: a backing plate just behind
/// the ball, rather than a second sphere, because a sphere of a larger radius is
/// *nearer* at the centre and at the rim alike and ends up winning everywhere.
pub fn draw_flat_disc(
    surface: &mut Surface,
    depth: &mut DepthBuffer,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    at_depth: f32,
    colour: u8,
    keep: &dyn Fn(i32, i32) -> bool,
) {
    let rows = table.rows(diameter);
    if rows.is_empty() {
        return;
    }
    let top = cy - (diameter as i32 - 1) / 2;
    let (h, w) = (surface.height(), surface.width());
    for (row, &width) in rows.iter().enumerate() {
        if width == 0 {
            continue;
        }
        let y = top + row as i32;
        if y < 0 || y as usize >= h {
            continue;
        }
        let left = cx - (width as i32 - 1) / 2;
        for i in 0..width as i32 {
            let x = left + i;
            if x < 0 || x as usize >= w || !keep(x, y) {
                continue;
            }
            if depth.test_and_set(x as usize, y as usize, at_depth) {
                surface.set(x as usize, y as usize, colour);
            }
        }
    }
}

/// A ring of `thickness` pixels inside a disc, depth-tested like any other
/// geometry. Rims go through here so they cannot escape the depth buffer the
/// way an unmasked ring escaped every clip we had before.
pub fn draw_ring(
    surface: &mut Surface,
    depth: &mut DepthBuffer,
    table: &SpanTable,
    cx: i32,
    cy: i32,
    diameter: usize,
    thickness: usize,
    centre_depth: f32,
    bias: f32,
    colour: u8,
    keep: &dyn Fn(i32, i32) -> bool,
) {
    let rows = table.rows(diameter);
    if rows.is_empty() {
        return;
    }
    let t = thickness.max(1);
    let r = diameter as f32 / 2.0;
    let top = cy - (diameter as i32 - 1) / 2;
    let (h, w) = (surface.height(), surface.width());

    for (row, &width) in rows.iter().enumerate() {
        if width == 0 {
            continue;
        }
        let y = top + row as i32;
        if y < 0 || y as usize >= h {
            continue;
        }
        let left = cx - (width as i32 - 1) / 2;
        let dy = (y as f32 + 0.5 - (cy as f32 + 0.5)) / r;
        let solid = row < t || row + t >= rows.len() || (width as usize) <= 2 * t;
        for i in 0..width as i32 {
            if !solid && i >= t as i32 && i < width as i32 - t as i32 {
                continue;
            }
            let x = left + i;
            if x < 0 || x as usize >= w {
                continue;
            }
            if !keep(x, y) {
                continue;
            }
            let dx = (x as f32 + 0.5 - (cx as f32 + 0.5)) / r;
            let nz = (1.0 - dx * dx - dy * dy).max(0.0).sqrt();
            if depth.test_and_set(x as usize, y as usize, centre_depth - nz * r + bias) {
                surface.set(x as usize, y as usize, colour);
            }
        }
    }
}

/// Ensure a span helper is reachable for callers that rasterize their own rows.
pub fn _span(cx: i32, cy: i32, diameter: usize, row: usize, width: u8) -> (i32, i32, usize) {
    row_span(cx, cy, diameter, row, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fur::Texture;
    use crate::shade::Ramp;
    use std::fs;
    use std::path::PathBuf;

    fn table() -> SpanTable {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        SpanTable::new(fs::read(root.join("evidence/ball/tables.raw")).unwrap())
    }

    #[test]
    fn the_nearer_ball_wins_whatever_order_they_are_drawn_in() {
        let table = table();
        let (near, far) = (10.0f32, 50.0f32);
        let pattern = Pattern::plain(Ramp::new(vec![7]), Texture::flat(8, 8, 0));
        for order in [true, false] {
            let mut s = Surface::new(64, 64);
            let mut d = DepthBuffer::new(64, 64);
            let paint = Paint::Shaded { light: Light::default(), pattern: &pattern };
            let balls = if order { [(near, 200u8), (far, 201u8)] } else { [(far, 201), (near, 200)] };
            for (depth, colour) in balls {
                draw_disc(&mut s, &mut d, &table, 32, 32, 20, depth, 0.0,
                          Paint::Flat(colour), &|_, _| true);
            }
            // Same centre, so the nearer ball covers the middle; the far one
            // survives only as the ring outside the near ball's silhouette.
            assert_eq!(s.get(32, 32).unwrap(), 200, "order {order}");
            let _ = paint;
        }
    }

    #[test]
    fn depth_is_solved_from_the_sphere_not_the_centre() {
        let table = table();
        let mut s = Surface::new(64, 64);
        let mut d = DepthBuffer::new(64, 64);
        draw_disc(&mut s, &mut d, &table, 32, 32, 40, 0.0, 0.0, Paint::Flat(9), &|_, _| true);
        let centre = d.get(32, 32);
        let edge = d.get(32, 32 + 18);
        assert!((centre + 20.0).abs() < 1.0, "centre depth {centre} should be radius in front");
        assert!(edge > centre, "edge {edge} should be behind centre {centre}");
    }

    #[test]
    fn a_bias_puts_one_ball_in_front_of_another() {
        let table = table();
        let mut s = Surface::new(64, 64);
        let mut d = DepthBuffer::new(64, 64);
        draw_disc(&mut s, &mut d, &table, 32, 32, 40, 0.0, 0.0, Paint::Flat(1), &|_, _| true);
        draw_disc(&mut s, &mut d, &table, 32, 32, 40, 0.0, -1.0, Paint::Flat(2), &|_, _| true);
        assert_eq!(s.get(32, 32).unwrap(), 2, "the biased ball should win");
    }

    #[test]
    fn a_small_ball_inside_a_large_one_is_hidden() {
        // A ball whose centre is inside another, at the same depth, is behind
        // that ball's surface and must not show. This is the muzzle.
        let table = table();
        let mut s = Surface::new(64, 64);
        let mut d = DepthBuffer::new(64, 64);
        draw_disc(&mut s, &mut d, &table, 32, 32, 40, 0.0, 0.0, Paint::Flat(1), &|_, _| true);
        draw_disc(&mut s, &mut d, &table, 36, 32, 10, 0.0, 0.0, Paint::Flat(2), &|_, _| true);
        assert_eq!(s.get(36, 32).unwrap(), 1, "the inner ball must not show");
    }
}
