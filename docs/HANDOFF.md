# Handoff - the creature, and the surface it lives on - 2026-10-29

Read this as orientation, not law. Refresh live state (`git status`, `git log`,
`cargo test --workspace`) before acting on it. The living record is
[docs/SANDBOX.md](SANDBOX.md); this note is only where to stand.

## State

- **The animal is a soft body of 48 balls**: spine 0-3, head 4-6, ears 7-10,
  eyes 11-14, four legs 15-42 (hip, knee, foot, four claws each), tail 43-47.
- **Walking works again** - `W`/`S`, `A`/`D` - and there is no animation beyond
  the gait: the tail hangs and swings on physics only, and the ears are rigidly
  placed.
- **The surface is 320x240**, the animal at half its authored size, in two
  window modes.
- Tree clean, `main` level with `origin/main` at `fefa649`.

## Landed since the last handoff

`ac092e4`, `bb16cb9`, `4f9e442`, `fefa649`, all pushed:

- the ears locked down: bases at (-2, 28), apart 14, radius 9.5; tips 12 above,
  radius 6.5, sunk 0.2; no live keys
- the surface cut to 320x240 with dev (2x window) and low-end (1:1) modes, and
  `PETZ_SCALE` / `PETZ_ZOOM` / `PETZ_MODE` / `PETZ_WINDOW` overrides
- four claws a foot, hips derived from their spine ball, the tail reworked into
  a held root plus a free chain
- a constraint overlay (`G`), shift-drag to carry the animal, and the spine's
  redundant diagonals removed

## Verified, and how

- `cargo test --workspace`: 68 tests green (39 core, 2 silhouette, 25 in the
  sandbox binary, 2 misc). The counts test expects 48 balls, 11 on the head,
  sixteen claws, five tail balls.
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
- The tail is one held root and four free balls, with soft links and a
  per-ball `drag` multiplier of about 0.998/0.985. Driving the chain makes a rod;
  leaving the root slack drops the tail under the cat.
- The spine is a plain chain plus one brace (skull to spine[2]). Its diagonals
  were measured at 2.4 degrees of fold and removed.
- Palette is a diagnostic set: black fur, snout and ear bases blue (240), nose
  pink (70), white on 0, 2, 8, 10, 17, 20, 23, 26, 44, 46, and black elsewhere.
  `PETZ_DIAG` still paints the second ear and every rim, and the hips.

## Next step

The roadmap the user agreed, in order: **tail and ear motion** (a slow sway on
the tail's root target, then gait coupling; the ears want the same `drag`
treatment so they are livelier than the body), then **the face** - mouth,
tongue, and eyelid states for excited, playful, sad, scared.

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
