//! petz-core - the platform-free core of a Petz 4 reimplementation.
//!
//! No windowing, no file dialogs, no SDL. The presentation layer lives in
//! `petz-sdl`; the oracle harness lives in `petz-headless`. Everything here must
//! be runnable on any target, including in a test with no display.

pub mod lnz;
pub mod spans;
pub mod surface;

pub use lnz::Lnz;
pub use spans::{fill_disc, fill_ring, row_span, SpanTable};
pub use surface::{Palette, Surface};
