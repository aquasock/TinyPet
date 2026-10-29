//! Projection: how the world becomes the screen.
//!
//! Petz is a 2.5D renderer, and the evidence for how it projects is in the
//! captured data: 330,902 rendered balls across 1,740 frames used only 23
//! distinct diameters, and every ball was drawn as a perfect circle. A radius
//! that never changes with distance means there is **no perspective divide** -
//! the projection is orthographic. Depth decides where a ball is drawn and what
//! covers it; it does not decide how big it is.
//!
//! That is much cheaper than a camera, and it is the reason the engine could
//! afford two hundred balls a frame. All a 2.5D projection needs is a fixed
//! view pitch:
//!
//! ```text
//!   screen_x = world_x
//!   screen_y = -(world_z * cos(pitch)) - (world_y * sin(pitch))
//!   depth    = world_y * cos(pitch) + world_z * sin(pitch)
//! ```
//!
//! At `pitch = 0` this is an exact side view: height maps straight to screen
//! up, and depth is just world y. That is the case our existing side-on walk
//! already draws, so it doubles as the regression check - the projection must
//! reduce to the old behaviour before it is trusted with a new one.
//!
//! World axes: **x** right, **y** away from the viewer, **z** up. Ground is
//! `z = 0`.

/// A point or direction in world space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    pub fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    #[inline]
    pub fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }

    #[inline]
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

/// A screen position plus the depth used for draw ordering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projected {
    /// Screen x, before any camera offset.
    pub x: f32,
    /// Screen y, measured from the ground line in `View`; negative is up.
    pub y: f32,
    /// Distance from the viewer. Larger is further away.
    pub depth: f32,
}

/// How the world is viewed. Orthographic, one fixed angle, no camera object.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Downward tilt of the view in radians. 0 is level with the room, which is
    /// the side-on case; around 0.4 (23 degrees) gives the usual Petz room
    /// angle, where walking away also moves up the screen.
    pub pitch: f32,
    /// Screen y of the ground line.
    pub ground: f32,
}

impl Default for View {
    fn default() -> Self {
        Self { pitch: 0.0, ground: 218.0 }
    }
}

impl View {
    pub fn new(pitch_degrees: f32, ground: f32) -> Self {
        Self { pitch: pitch_degrees.to_radians(), ground }
    }

    pub fn project(&self, p: Vec3) -> Projected {
        let (s, c) = (self.pitch.sin(), self.pitch.cos());
        Projected {
            x: p.x,
            y: self.ground - (p.z * c) - (p.y * s),
            depth: p.y * c + p.z * s,
        }
    }
}

impl View {
    /// Recover world y and z from a screen position and a known depth.
    ///
    /// Two equations, two unknowns. The system degenerates at a 45 degree
    /// pitch, where height and distance project to the same thing - it is a
    /// genuine ambiguity of an orthographic view, not a bug here.
    pub fn unproject(&self, screen_y: f32, depth: f32) -> (f32, f32) {
        let (s, c) = (self.pitch.sin(), self.pitch.cos());
        let sy = self.ground - screen_y; // positive is up
        let det = s * s - c * c;
        if det.abs() < 1e-4 {
            return (depth, sy);
        }
        ((sy * s - depth * c) / det, (s * depth - c * sy) / det)
    }
}

/// A place and a facing in the world. Everything on an animal is authored in
/// body coordinates - forward, lateral, up - and placed through this.
#[derive(Clone, Copy, Debug, Default)]
pub struct Body {
    /// World position of the point between the feet, on the ground.
    pub pos: Vec3,
    /// Facing, in radians, measured from "away from the viewer" (world +y).
    /// `PI/2` faces right, which is the old side-on walk.
    pub heading: f32,
}

impl Body {
    /// Place a body-space point in the world.
    ///
    /// `forward` runs out of the animal's nose, `lateral` to its right, and
    /// `up` from the ground.
    pub fn to_world(&self, forward: f32, lateral: f32, up: f32) -> Vec3 {
        let (s, c) = (self.heading.sin(), self.heading.cos());
        Vec3::new(
            self.pos.x + forward * s + lateral * c,
            self.pos.y + forward * c - lateral * s,
            self.pos.z + up,
        )
    }

    /// Unit vector along the animal's forward axis.
    pub fn forward(&self) -> Vec3 {
        let (s, c) = (self.heading.sin(), self.heading.cos());
        Vec3::new(s, c, 0.0)
    }

    /// Unit vector to the animal's right, along the ground.
    pub fn right(&self) -> Vec3 {
        let (s, c) = (self.heading.sin(), self.heading.cos());
        Vec3::new(c, -s, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_view_is_the_old_side_on_geometry() {
        // Side-on: heading right, no view pitch. A point 40 px left and 68 px
        // up must land exactly where the 2D walker put it.
        let view = View { pitch: 0.0, ground: 218.0 };
        let body = Body { pos: Vec3::new(500.0, 0.0, 0.0), heading: std::f32::consts::FRAC_PI_2 };
        let world = body.to_world(40.0, 0.0, 68.0);
        let p = view.project(world);
        assert!((p.x - 540.0).abs() < 0.01, "x is {}", p.x);
        assert!((p.y - 150.0).abs() < 0.01, "y is {} (218 - 68)", p.y);
    }

    #[test]
    fn walking_away_moves_up_the_screen() {
        let view = View::new(25.0, 218.0);
        let body = Body { pos: Vec3::new(320.0, 0.0, 0.0), heading: 0.0 };
        let near = view.project(body.to_world(0.0, 0.0, 0.0));
        let far = view.project(body.to_world(0.0, 0.0, 0.0).add(Vec3::new(0.0, 200.0, 0.0)));
        assert!(far.y < near.y, "far point should be higher: {} vs {}", far.y, near.y);
        assert!(far.depth > near.depth, "far point should be deeper");
        // Orthographic: distance does not shrink anything.
        assert_eq!(near.x, far.x);
    }

    #[test]
    fn a_turning_body_sweeps_a_circle() {
        let view = View::default();
        let mut body = Body { pos: Vec3::new(0.0, 0.0, 0.0), heading: 0.0 };
        let mut xs = Vec::new();
        for i in 0..8 {
            body.heading = i as f32 * std::f32::consts::FRAC_PI_4;
            let p = view.project(body.to_world(100.0, 0.0, 0.0));
            xs.push((p.x, p.y));
        }
        // Heading 0 faces +y (depth), quarter turns go right, back, left.
        assert!(xs[0].0.abs() < 0.01, "facing away should not move x");
        assert!((xs[2].0 - 100.0).abs() < 0.01, "quarter turn should face +x");
        assert!((xs[4].0).abs() < 0.01, "half turn returns to the axis");
        assert!((xs[6].0 + 100.0).abs() < 0.01, "three-quarter turn faces -x");
    }

    #[test]
    fn lateral_offset_is_perpendicular_to_forward() {
        let body = Body { pos: Vec3::default(), heading: 1.0 };
        let f = body.to_world(1.0, 0.0, 0.0);
        let r = body.to_world(0.0, 1.0, 0.0);
        assert!((f.x * r.x + f.y * r.y + f.z * r.z).abs() < 1e-4, "axes are not perpendicular");
    }

    #[test]
    fn unproject_inverts_project() {
        for pitch in [0.0f32, 15.0, 25.0] {
            let view = View::new(pitch, 218.0);
            for (wy, wz) in [(0.0f32, 0.0f32), (100.0, 50.0), (-40.0, 68.0)] {
                let p = view.project(Vec3::new(0.0, wy, wz));
                let (y, z) = view.unproject(p.y, p.depth);
                assert!((y - wy).abs() < 0.01 && (z - wz).abs() < 0.01,
                        "pitch {pitch}: ({y},{z}) != ({wy},{wz})");
            }
        }
    }

    #[test]
    fn the_ground_plane_projects_to_a_line() {
        // Every point whose height is zero must land on the ground line plus a
        // depth term - never above it for a level view.
        let view = View::new(25.0, 218.0);
        for d in [0.0f32, 50.0, 300.0] {
            let p = view.project(Vec3::new(10.0, d, 0.0));
            assert!(p.y <= 218.0 + 0.01);
        }
    }
}
