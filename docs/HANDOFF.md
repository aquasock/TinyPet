# Handoff - the face, after the tail was settled - 2026-10-30

Read this as orientation, not law. Refresh live state (`git status`, `git log`,
`cargo test --workspace`) before acting on it. The living record is
[docs/SANDBOX.md](SANDBOX.md); this note is only where to stand.

## State

- **The animal is a soft body of 48 balls**: spine 0-3, head 4-6, ears 7-10,
  eyes 11-14, four legs 15-42 (hip, knee, foot, four claws each), tail 43-47.
- **Walking works** - `W`/`S` forward and back, `A`/`D` turn. There is no
  animation beyond the gait: the ears are rigidly placed and the tail is physics
  only.
- **The surface is 320x240**, the animal at half its authored size, in two window
  modes.
- **The face has a nose and a mouth.** The mouth is a *texture on the muzzle*, not
  a ball: two humps, painted the way the pupil is painted on the sclera.
- Tree clean, `main` level with `origin/main` at `3098b63`.
- A ragdoll may still be running from the owner's session (no dump instrument on
  it, so it cannot corrupt `/tmp/ours.ppm`). `pgrep -x ragdoll` before measuring
  anything anyway.

## What this session did

**The tail root is welded to the rump.** Ball 43 was driven toward a place behind
the cat, so it lagged whatever moved the rump - 6.1 body units off in a walking
step, visible as a tail base hovering behind the animal. It is now placed from the
butt ball every frame, pinned, like a hip from its spine ball, and the chain sits
on the rump's lateral plane so the spine's flex cannot step the joint sideways.
Exact at placement; 0.31 units stale after a step.

**The tail's sway was built, tuned, measured, and then removed.** The owner's line:
the tail is a physical thing that trails the body, not an animation played at it.
Do not rebuild it without asking. `docs/SANDBOX.md` keeps the numbers so it would
not have to be rediscovered.

**The mouth, in three passes.** A small black circle, then a thin line, then a
curve - each one asked for by the owner and judged in the window. The final shape
is two humps painted onto the muzzle's own sphere with a new core primitive,
`draw_curve_on_sphere`, which walks columns so the shape is the caller's and the
depth is still the reference sphere's per pixel. `K`/`L` lengthen it, `H`/`J` curl
it, `N`/`M` move it, all live.

**A readout and an input fix came and went with the sway**: the digit font grew
letters for `GAIN`/`RATE`, and tuning keys were made to ignore auto-repeat after
holding `K` walked a value to its bound. The text path went when the sway did; the
`repeat: false` fix stayed.

## Verified, and how

- `cargo test --workspace`: **70 tests green** (41 core, 2 silhouette, 27 in the
  sandbox binary). The counts test expects 48 balls, 11 on the head, sixteen
  claws, five tail balls.
- The tail root's weld: exact at placement, under 1.5 units of residual after any
  step of turning, walking, both, or standing (`the_tail_root_cannot_come_off_the_rump`).
- Tail liveliness, degrees of travel after a turn ends: 75 with the tail's own
  damping against 20 damped like the body - the chain answering the body with
  nothing driving it.
- The mouth, counted under `PETZ_DIAG` where it paints green: 9-10 px across
  facing the camera, 8 at 135 degrees, 5 at 120 and in profile either way, none
  facing away. `the_mouth_is_painted_on_the_half_of_the_snout_that_faces_the_viewer`
  pins the placement; `draw_curve_on_sphere` has two core tests.
- Hips, measured against a rigid body through a full-rate turn: 0.2-1.1 px, and
  the hip-to-spine gap fixed at its authored 20 units.
- **Not verified by eye**: `PETZ_MODE=low` since the ear links and the spine
  simplification. It was judged once at 320x240, before those changes.

## Open, not fixed, not this session's fault

- **The feet shimmer while standing.** About thirty pixels on the rims of the two
  white feet change between frames 25 seconds apart, so it is a steady-state
  solver artifact rather than settling. Found while checking that the tail was
  still; untouched.
- **The mouth is at the snout's size limit.** 7 px of radius on screen gives 7.5 px
  of width at the mouth's height, and the humps already meet the nose's lower
  corners. A bigger cartoon mouth needs a bigger muzzle - the owner was offered
  that and passed on it.

## Decisions a fresh session must not re-litigate

- Projection is orthographic with a 25 degree pitch. No perspective divide.
- Depth buffer, not painter's sort.
- **Anything drawn in another ball's frame is *derived* from that ball, heading
  included** - not eased separately. This cost three attempts on the hips; the
  failure mode is a part that swings off the body and snaps back.
- **Sway is lag x distance.** A part far from the body's origin needs a
  proportionally shorter lag to match one near it.
- The ears are pinned and placed exactly (the pair must not drift apart), and
  linked to the skull so a pulled ear drags the head. They snap back on release in
  one frame; that is the accepted cost, not a bug.
- The tail is one **pinned** root and four free balls, with soft links and a
  per-ball `drag` multiplier of about 0.998/0.985. The root is derived from the
  butt ball every frame, never driven: driven, it lags the rump and the joint
  opens.
- **The tail is not animated**, and **the face's markings are textures, not
  balls**. The mouth, like the pupil, is a disc or curve whose depth comes from
  the sphere it is painted on - `draw_disc_on_sphere` for round markings, and
  `draw_curve_on_sphere` (added this session, in `petz-core::raster`) for shaped
  ones.
- **A marking's reference sphere has to be the sphere the ball was actually
  drawn from** - centre *and* radius rounded exactly as the rasterizer rounds
  them, or the ball's own surface beats the marking's bias and paints over it
  everywhere but the middle. This cost two rounds of "the mouth draws nothing".
- The spine is a plain chain plus one brace (skull to spine[2]). Its diagonals
  were measured at 2.4 degrees of fold and removed.
- Palette is a diagnostic set: black fur, snout and ear bases blue (240), nose pink
  (70), white on 0, 2, 8, 10, 17, 20, 23, 26, 44, 46, and black elsewhere.
  `PETZ_DIAG` paints the second ear blue, every rim red, and the mouth green (250).

## Next step

The owner's thread is **the face**. The mouth's shape and place are settled; what
is left is expression:

1. **The tongue.** Retail draws it as another marking on the snout - the same
   texture model as the mouth, so it is a second curve or disc with its own
   colour and place, shown in some states and not others.
2. **Eyelid states** - excited, playful, sad, scared. The lid is a *parameter* in
   the reference (colour, height, tilt), not geometry, and the lid shell already
   exists in the eye renderer, drawn last and closing the eye when there is no
   inner shell to see. `Walker` already carries `lid_drop` and `lid_tilt`.
3. When the mouth's numbers settle completely, fold `K`/`L`/`H`/`J`/`N`/`M` into
   constants and drop the keys, the way the ears' and the eyes' keys went.

Before that, two things worth knowing if the owner asks for "better proportions":
the skeleton is hand-authored, and loading a real `.pet` (the `[Add Ball]`
coordinates, and the neck size in the packed `SCP` resource) is the change that
would make the face's scale a fact rather than a guess.

## Continuation

- Build: `cargo build --release -p petz-sdl --bin ragdoll`.
- Dev window: `./target/release/ragdoll`. Low-end: `PETZ_MODE=low`.
- Keys: `W`/`S` forward and back, `A`/`D` (or `Q`/`E`) turn, `+`/`-` speed, `V`
  pitch, arrows light, `Z`/`X` ambience, `[`/`]` ramp steps, `T` texture,
  `,`/`.` spine flex, `Tab` ball numbers, `G` constraint overlay, `P` print
  settings, `R` reset, `Space` pause, `Esc` quit, `Shift`+drag carry the animal,
  plain drag poke one ball.
- Mouth, live while it is being judged: `K`/`L` length, `H`/`J` curl, `N`/`M`
  height on the snout. `P` and the window title report all three.
- Instruments: `PETZ_HEADING=<deg>` pins the facing, `PETZ_DUMP_PPM=1` writes
  `/tmp/ours.ppm`, `PETZ_SWEEP=<rad/s>` rotates steadily, `PETZ_DIAG=1` paints the
  diagnostic colours, `PETZ_MOUTH` / `PETZ_MOUTH_CURL` / `PETZ_MOUTH_DROP` set the
  mouth headlessly for a sweep. Dump pixel counts; do not trust a window grab.
- Watch for stale instances: several ragdolls at once all write `/tmp/ours.ppm`,
  which silently corrupts any measurement. `pgrep -x ragdoll` first.
