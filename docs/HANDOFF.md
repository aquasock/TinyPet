# Handoff - the creature, and the surface it lives on - 2026-10-29

Read this as orientation, not law. Refresh live state (`git status`, `git log`,
`cargo test --workspace`) before acting on it. The living record is
[docs/SANDBOX.md](SANDBOX.md); this note is only where to stand.

## State

- **The animal is a soft body of 48 balls**: spine 0-3, head 4-6, ears 7-10,
  eyes 11-14, four legs 15-42 (hip, knee, foot, four claws each), tail 43-47.
- **Walking works again** - `W`/`S`, `A`/`D`. There is no animation beyond the
  gait: the ears are rigidly placed and the tail is not animated at all.
- **The surface is 320x240**, the animal at half its authored size, in two
  window modes.
- Four commits ahead of `origin/main`, none pushed.

## Landed since the last handoff

**The tail's root is welded to the rump.** Ball 43 was driven toward a place
behind the cat, so it lagged whatever moved the rump: 6.1 body units off in a
walking step, visible as a tail base hovering behind the animal. It is now
placed from the butt ball every frame, pinned, like a hip from its spine ball,
and the chain sits on the rump's lateral plane so the spine's flex cannot step
the joint sideways. Exact at placement; 0.31 units stale after a step.

**The tail's sway was built and then dropped.** An animated sway was tuned in the
window and measured - an oval traced by the ball one link in from the root, a
lateral glide down the chain, live keys, an on-screen readout - and then removed
at the owner's request. The tail is physics only: it trails the rump, and hangs
when the animal stands. The readout's text path went with it. `docs/SANDBOX.md`
has the numbers and the reasons, so it need not be rebuilt blind.

**The mouth, first cut.** A black disc painted onto the muzzle's own sphere with
the same primitive as the pupil, so it lies on the snout and the muzzle's circle
clips it as the head turns. Radius 4.5 body units, 0.85 of the muzzle's radius
below its centre - the snout's limit, where the dot's lower edge meets the rim; `K`/`L` and `N`/`M` tune both live. Facing the camera it is 12
pixels under the nose; in profile a sliver; facing away, culled. Two faults were
fixed on the way, both about the reference sphere having to match the drawn ball
exactly - see `docs/SANDBOX.md`.

Previously landed and pushed - `ac092e4`, `bb16cb9`, `4f9e442`, `fefa649`:

- the ears locked down: bases at (-2, 28), apart 14, radius 9.5; tips 12 above,
  radius 6.5, sunk 0.2; no live keys
- the surface cut to 320x240 with dev (2x window) and low-end (1:1) modes, and
  `PETZ_SCALE` / `PETZ_ZOOM` / `PETZ_MODE` / `PETZ_WINDOW` overrides
- four claws a foot, hips derived from their spine ball, the tail reworked into
  a held root plus a free chain
- a constraint overlay (`G`), shift-drag to carry the animal, and the spine's
  redundant diagonals removed

## Verified, and how

- `cargo test --workspace`: 68 tests green (39 core, 2 silhouette, 27 in the
  sandbox binary). The counts test expects 48 balls, 11 on the head, sixteen
  claws, five tail balls. The previous handoff said 68 and "2 misc"; there is no
  such pair.
- Tail liveliness, measured as degrees of travel after a turn ends: 75 with the
  tail's own damping against 20 damped like the body. That is the chain answering
  the body, with nothing driving it.
- The tail root's weld: exact at placement, and under 1.5 units of residual after
  any step of turning, walking, both, or standing.
- Hips, measured against a rigid body through a full-rate turn: 0.2-1.1 px, and
  the hip-to-spine gap fixed at its authored 20 units.
- Outline weight, counted from a frame: 2 px in 52% of runs, 3 px in 20% (where
  two rims meet).
- Not verified by eye: the animal in `PETZ_MODE=low` after the last few changes.
  It was looked at once at 320x240 and judged good, before the ear links and the
  spine simplification.

## Decisions a fresh session must not re-litigate

- Projection is orthographic with a 25 degree pitch. No perspective divide.
- Depth buffer, not painter's sort.
- **Anything drawn in another ball's frame is *derived* from that ball, heading
  included** - not eased separately. This cost three attempts on the hips; the
  failure mode is a part that swings off the body and snaps back.
- **Sway is lag x distance.** A part far from the body's origin needs a
  proportionally shorter lag to match one near it.
- The ears are pinned and placed exactly (the pair must not drift apart), and
  linked to the skull so a pulled ear drags the head. They snap back on release
  in one frame; that is the accepted cost, not a bug.
- The tail is one **pinned** root and four free balls, with soft links and a
  per-ball `drag` multiplier of about 0.998/0.985. The root is derived from the
  butt ball every frame, never driven: driven, it lags the rump and the joint
  opens.
- **The tail is not animated.** A sway was built, measured and removed. Do not
  add one back without asking: the owner's line is that the tail is a physical
  thing that trails the body.
- The spine is a plain chain plus one brace (skull to spine[2]). Its diagonals
  were measured at 2.4 degrees of fold and removed.
- Palette is a diagnostic set: black fur, snout and ear bases blue (240), nose
  pink (70), white on 0, 2, 8, 10, 17, 20, 23, 26, 44, 46, and black elsewhere.
  `PETZ_DIAG` still paints the second ear and every rim, and the hips.

## Next step

The roadmap the user agreed: **the face** - mouth, tongue, and eyelid states for
excited, playful, sad, scared. Tail animation is off the list by decision, and
ear motion was never asked for after that.

The mouth has a first cut: a black disc on the snout, right place, no shape. The
next questions are what a mouth should *be* - the reference draws a marking with
a shape, and expression on top of it.

1. Settle the mouth's shape and place by eye (`K`/`L`, `N`/`M`), then make its
   numbers constants and drop the keys.
2. Tongue and eyelid states: excited, playful, sad, scared. The eyelid is a
   parameter in the reference - colour, height, tilt - not geometry, and the lid
   shell already exists in the eye renderer, closed by default.

## Continuation

- Build: `cargo build --release -p petz-sdl --bin ragdoll`.
- Dev window: `./target/release/ragdoll`. Low-end: `PETZ_MODE=low`.
- `G` toggles the constraint overlay; `Tab` numbers the balls; `P` prints the
  setup; `Shift`+drag carries the animal; plain drag pokes one ball.
- Instruments: `PETZ_HEADING=<deg>` pins the facing, `PETZ_DUMP_PPM=1` writes
  `/tmp/ours.ppm`, `PETZ_SWEEP=<rad/s>` rotates steadily, `PETZ_DIAG=1` paints
  the diagnostic colours. Dump pixel counts, do not trust a window grab.
- Watch for stale instances: several ragdolls at once all write `/tmp/ours.ppm`,
  which silently corrupts any measurement. `pgrep -x ragdoll` first.
