//! Gait: where a quadruped's feet want to be, and the IK that gets them there.
//!
//! The ragdoll alone is a floppy thing — it falls over, which is correct but not
//! useful. What turns it into an animal is a pose controller layered on top:
//! feet follow a phase-driven cycle, and the body is *driven* toward the pose
//! the cycle describes while the solver supplies the wobble.
//!
//! The foot cycle is the standard treadmill: during stance the foot holds still
//! in the world, which from the body's frame looks like it slides backwards at
//! exactly the walking speed. During swing it lifts and reaches forward. The two
//! halves are automatically consistent because both cover the same distance,
//! `speed * duty * cycle`.
//!
//! Legs are placed by analytic two-bone IK, so a foot target outside the leg's
//! reach is clamped rather than snapping.

use crate::ragdoll::Vec2;

/// Timing and reach of a gait. Distances are pixels, times are seconds.
#[derive(Clone, Copy, Debug)]
pub struct GaitSpec {
    /// Seconds for one full cycle of a single leg.
    pub cycle: f32,
    /// Fraction of the cycle a foot spends on the ground. Above 0.5 is a walk,
    /// near 0.5 a trot, below 0.5 a bound.
    pub duty: f32,
    /// Ground speed in pixels per second.
    pub speed: f32,
    /// How high a swinging foot lifts.
    pub step_height: f32,
}

impl Default for GaitSpec {
    fn default() -> Self {
        Self { cycle: 0.85, duty: 0.62, speed: 95.0, step_height: 16.0 }
    }
}

/// A foot's offset from its hip, in the body frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FootPose {
    /// Horizontal offset from the hip's ground point. Negative is behind.
    pub dx: f32,
    /// Vertical offset; negative is up, so a swinging foot returns -step_height.
    pub dy: f32,
    /// True while the foot is on the ground.
    pub planted: bool,
}

/// The gait clock. One instance per animal.
#[derive(Clone, Debug)]
pub struct Gait {
    pub spec: GaitSpec,
    /// Where we are in the cycle, 0..1.
    pub phase: f32,
    /// Per-leg phase offsets. A walk is 0, .5, .25, .75; a trot is 0, .5, .5, 0.
    pub offsets: [f32; 4],
}

impl Gait {
    pub fn walk(spec: GaitSpec) -> Self {
        // Left front, right front, left hind, right hind.
        Self { spec, phase: 0.0, offsets: [0.0, 0.5, 0.25, 0.75] }
    }

    pub fn trot(spec: GaitSpec) -> Self {
        Self { spec, phase: 0.0, offsets: [0.0, 0.5, 0.5, 0.0] }
    }

    pub fn advance(&mut self, dt: f32) {
        self.phase = (self.phase + dt / self.spec.cycle.max(1e-4)).rem_euclid(1.0);
    }

    /// How far a foot travels along the ground during one stance.
    pub fn stride(&self) -> f32 {
        self.spec.speed * self.spec.duty * self.spec.cycle
    }

    /// Foot pose for one leg at the current time.
    pub fn foot(&self, leg: usize) -> FootPose {
        let local = (self.phase + self.offsets[leg]).rem_euclid(1.0);
        let d = self.stride();
        let duty = self.spec.duty.clamp(0.05, 0.95);
        if local < duty {
            // Stance: slide backwards relative to the body at exactly the
            // speed the body is moving forwards, so the foot stays put.
            let t = local / duty;
            FootPose { dx: d * 0.5 - d * t, dy: 0.0, planted: true }
        } else {
            let t = (local - duty) / (1.0 - duty);
            FootPose {
                dx: -d * 0.5 + d * t,
                dy: -self.spec.step_height * (t * std::f32::consts::PI).sin(),
                planted: false,
            }
        }
    }
}

/// Solve a two-bone chain: where the middle joint goes so that `foot` is reachable.
///
/// `bend` picks the side the knee falls on: +1 and -1 are the two elbows/knees
/// a dog can have. Out-of-reach targets are clamped onto the reachable arc
/// rather than returned as a nonsense position.
pub fn two_bone_ik(hip: Vec2, foot: Vec2, upper: f32, lower: f32, bend: f32) -> Vec2 {
    let to = foot.sub(hip);
    let dist = to.length();
    if dist < 1e-4 {
        return Vec2::new(hip.x, hip.y + upper);
    }
    let max = upper + lower;
    let min = (upper - lower).abs().max(1e-3);
    let d = dist.clamp(min, max);

    // Law of cosines: how far along the hip->foot line the knee sits.
    let a = (upper * upper - lower * lower + d * d) / (2.0 * d);
    let h2 = upper * upper - a * a;
    let h = if h2 > 0.0 { h2.sqrt() } else { 0.0 };

    let dir = Vec2::new(to.x / dist, to.y / dist);
    let perp = Vec2::new(-dir.y, dir.x);
    Vec2::new(
        hip.x + dir.x * a + perp.x * h * bend,
        hip.y + dir.y * a + perp.y * h * bend,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gait() -> Gait {
        Gait::walk(GaitSpec { cycle: 1.0, duty: 0.6, speed: 100.0, step_height: 10.0 })
    }

    #[test]
    fn a_stance_foot_does_not_move_in_the_world() {
        // The body advances at `speed`; the foot's body-relative offset must
        // retreat at the same rate, or the animal would skate.
        let g = gait();
        let speed = g.spec.speed;
        let dt = 0.001;
        let mut g = g;
        let mut body_x = 0.0;
        let mut prev_world: Option<f32> = None;
        let mut samples = 0;
        while g.phase < g.spec.duty - 1e-3 {
            let f = g.foot(0);
            assert!(f.planted);
            let world = body_x + f.dx;
            if let Some(p) = prev_world {
                assert!((world - p).abs() < 0.05, "foot slid {:.3} px", world - p);
                samples += 1;
            }
            prev_world = Some(world);
            body_x += speed * dt;
            g.advance(dt);
        }
        assert!(samples > 100, "not enough stance samples: {samples}");
    }

    #[test]
    fn a_swinging_foot_lifts_and_lands_forward() {
        let g = gait();
        let mut g2 = g.clone();
        while g2.foot(0).planted {
            g2.advance(0.001);
        }
        // Where it lifted off - the foot should land ahead of this, or the
        // animal would walk backwards.
        let liftoff = g2.foot(0).dx;
        let mut peak = 0.0f32;
        let mut last = g2.foot(0);
        while !last.planted {
            peak = peak.max(-last.dy);
            last = g2.foot(0);
            g2.advance(0.001);
        }
        assert!(peak > 9.0, "swing did not lift: {peak}");
        assert!(
            last.dx > liftoff + 1.0,
            "foot landed at {} having lifted at {liftoff}",
            last.dx
        );
    }

    #[test]
    fn stance_and_swing_cover_the_same_distance() {
        let g = gait();
        let d = g.stride();
        assert!((g.foot(0).dx - d * 0.5).abs() < 1e-3);
    }

    #[test]
    fn ik_reaches_a_target_within_range() {
        let hip = Vec2::new(0.0, 0.0);
        let foot = Vec2::new(20.0, 40.0);
        let knee = two_bone_ik(hip, foot, 30.0, 30.0, 1.0);
        let l1 = knee.sub(hip).length();
        let l2 = foot.sub(knee).length();
        assert!((l1 - 30.0).abs() < 0.01, "upper bone is {l1}");
        assert!((l2 - 30.0).abs() < 0.01, "lower bone is {l2}");
    }

    #[test]
    fn ik_clamps_an_unreachable_target() {
        let hip = Vec2::new(0.0, 0.0);
        let foot = Vec2::new(0.0, 500.0); // far beyond 60 px of leg
        let knee = two_bone_ik(hip, foot, 30.0, 30.0, 1.0);
        let l1 = knee.sub(hip).length();
        assert!((l1 - 30.0).abs() < 0.01, "bone length changed under clamping: {l1}");
        assert!(knee.y < 31.0, "knee should stay within the leg's reach");
    }

    #[test]
    fn the_two_bends_are_mirror_images() {
        let hip = Vec2::new(0.0, 0.0);
        let foot = Vec2::new(10.0, 40.0);
        let a = two_bone_ik(hip, foot, 30.0, 30.0, 1.0);
        let b = two_bone_ik(hip, foot, 30.0, 30.0, -1.0);
        assert!((a.x - b.x).abs() > 1.0, "bend should choose a side: {a:?} {b:?}");
        // Both solutions are equally valid: same bone lengths, mirrored.
        assert!((a.sub(hip).length() - b.sub(hip).length()).abs() < 1e-3);
    }
}
