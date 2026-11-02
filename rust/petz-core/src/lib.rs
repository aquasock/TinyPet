//! petz-core - the platform-free core of a Petz 4 reimplementation.
//!
//! No windowing, no file dialogs, no SDL. The presentation layer lives in
//! `petz-sdl`; the oracle harness lives in `petz-headless`. Everything here must
//! be runnable on any target, including in a test with no display.

pub mod fur;
pub mod gait;
pub mod lnz;
pub mod project;
pub mod ragdoll;
pub mod raster;
pub mod shade;
pub mod spans;
pub mod surface;

pub use fur::{shade_disc_pattern, Pattern, Texture};
pub use gait::{two_bone_ik, FootPose, Gait, GaitSpec};
pub use lnz::Lnz;
pub use project::{Body, Projected, Vec3, View};
pub use ragdoll::{Constraint, Particle, Ragdoll, Vec2};
pub use raster::{
    draw_curve_on_sphere, draw_disc, draw_disc_on_sphere, draw_flat_disc, draw_ring, DepthBuffer,
    Paint,
};
pub use shade::{shade_disc, Light, Ramp};
pub use spans::{fill_disc, fill_disc_masked, fill_ring, fill_ring_masked, row_span, SpanTable};
pub use surface::{Palette, Surface};
