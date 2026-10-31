# Handoff - the creature, and the surface it lives on - 2026-10-29

Read this as orientation, not law. Refresh live state (`git status`, `git log`,
`cargo test --workspace`) before acting on it. The living record is
[docs/SANDBOX.md](SANDBOX.md); this note is only where to stand.

## State

- **The animal is a soft body of 48 balls**: spine 0-3, head 4-6, ears 7-10,
  eyes 11-14, four legs 15-42 (hip, knee, foot, four claws each), tail 43-47.
- **Walking works again** - `W`/`S`, `A`/`D`. There is no animation beyond the
  gait and the tail's idle sway: the ears are still rigidly placed.
- **The surface is 320x240**, the animal at half its authored size, in two
  window modes.
- One commit ahead of `origin/main`: the tail's motion, not yet pushed.

## Landed since the last handoff

**The tail's idle sway.** The root is pinned to the butt ball - derived from it
every frame, like a hip from its spine ball - because driven it wandered 6.1 body
units off the rump in a walking step and the base of the tail hovered behind the
cat. The sway now drives the first *free* ball instead (`TAIL_SWAY_X/Y/RATE`,
`TAIL_DRIVE` 0.10, plus one swing per stride while walking), so the joint cannot
open and the four free links carry the motion out: 8.75 body units of tip travel
at gain 1. There is also a lateral glide down the chain (`TAIL_LATERAL`,
`TAIL_LATERAL_LAG`) riding on the rump's own lateral flex, because the solver is
sagittal and the fore-and-aft sway is edge-on when the animal faces the camera. `K`/`L` scale it and `N`/`M` speed it up while it is being judged, and the two
values are drawn on the surface under the facing reading for four seconds after
each press (`GAIN` then `RATE`), with the same numbers in the window title. Measured at gain 1: the tip covers 6.7 body
units in four seconds, 11 px of screen reach in profile, 17 px of diff at the
rear facing the camera; a two-frame diff while standing changes 232 pixels and
231 of them are in the tail's corner. Tuning keys ignore auto-repeat now: holding
`K` used to walk the gain to zero in five steps, which read as "the keys do
nothing".
`the_tail_sways_when_the_animal_stands_still` waits 20 s before measuring,
because the chain rings for ten, and the older liveliness test silences the
driver so it still measures the physics.

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

- `cargo test --workspace`: 70 tests green (39 core, 2 silhouette, 29 in the
  sandbox binary). The counts test expects 48 balls, 11 on the head, sixteen
  claws, five tail balls. The previous handoff said 68 and "2 misc"; there is no
  such pair.
- Tail liveliness, measured as degrees of travel after a turn ends: 75 with the
  tail's own damping against 20 damped like the body.
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
  opens. Driving the whole chain makes a rod; driving the first free ball at 0.10
  does not.
- The spine is a plain chain plus one brace (skull to spine[2]). Its diagonals
  were measured at 2.4 degrees of fold and removed.
- Palette is a diagnostic set: black fur, snout and ear bases blue (240), nose
  pink (70), white on 0, 2, 8, 10, 17, 20, 23, 26, 44, 46, and black elsewhere.
  `PETZ_DIAG` still paints the second ear and every rim, and the hips.

## Next step

The roadmap the user agreed, in order: **tail and ear motion**, then **the face**
- mouth, tongue, and eyelid states for excited, playful, sad, scared.

Tail motion is in, and unjudged by eye. So, in order:

1. Look at it - `./target/release/ragdoll`, `K`/`L` for reach and `N`/`M` for
   rate. Standing and walking are two different judgements: the walk adds a
   swing per stride.
2. The ears, still rigid. They want what the tail got - a little lag on the tip,
   so they are livelier than the skull without the pair drifting apart. After the
   tail settles, not alongside it.
3. Then the face.

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
