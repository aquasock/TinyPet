# Handoff - the face, after the tail was settled - 2026-10-30

Read this as orientation, not law. Refresh live state (`git status`, `git log`,
`cargo test --workspace`) before acting on it. The living record is
[docs/SANDBOX.md](SANDBOX.md); this note is only where to stand.

## State

- **The animal is a soft body of 48 balls**: spine 0-3, head 4-6 (skull, muzzle,
  nose), ears 7-10, eyes 11-14, four legs 15-42 (hip, knee, foot, four claws
  each), tail 43-47.
- **Walking works** - `W`/`S` forward and back, `A`/`D` turn. There is no
  animation beyond the gait: the ears are rigidly placed and the tail is physics
  only.
- **The surface is 320x240**, the animal at half its authored size, in two window
  modes.
- **The face has a nose and a mouth.** The nose is a pink ball on the snout's
  tip, as it always was. The mouth is a *drawing* mapped onto the muzzle's
  the muzzle's **surface** - every pixel asks where it sits on the sphere and
  looks the drawing up by that angle, so the arms curve round the snout. A
  drawing, not a ball. Its **fork** is anchored just under the nose; anchoring the
  bottom edge cannot survive wrapping, because the rim is where the surface turns
  edge-on. The top 10% of the bitmap is blanked - not cropped - to keep its first
  rows out of the pink nose.
- Tree clean, `main` level with `origin/main`. (No SHA here on purpose: the commit
  that adds this note moves it.)
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

**The mouth, in ten passes, two of them reverted.** A small black circle, a thin line, a curve, a
flip, a supplied nose-and-whiskers sheet, a supplied mouth drawing, and finally
that drawing re-mapped from a plate over the disc to a decal *on the surface*.
The sheet was reverted on the owner's call: it duplicated a nose the ball already
drew. Two earlier passes were wrapped and reverted, and both failed for the same reason:
a placement fault dressed up as a mapping fault. One anchored the centre to the
nose's projected position - it tracked the nose exactly, 5.0 px below it at every
heading, but put the mouth on the nose. The other derived the centre from a
body-space angle, which drifted ten pixels against the nose across a quarter turn.
The placement was then fixed separately (the box was being centred when it should
have been anchored by an edge), and only then did the wrap land: angular
(equidistant) mapping, `half_h = asin(wide/2r)` so the approved width is kept,
`half_v` by aspect so nothing stretches, and the **fork** anchored instead of the
bottom edge. `tools/make-marking.py` turns a PNG into a coverage grid compiled into
the binary, cropping to the ink first. `K`/`L` set the span, `N`/`M` the tilt.

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
- The mouth: `cargo test --workspace` is 72 green. The drawing's own shape is
  judged by eye in the window, not by a pixel count. What the dump does confirm
  are the placement and the absence of distortion: under `PETZ_DIAG` at 180, 150,
  120 and 90 degrees the drawing's top row is 1-2 px below the nose's bottom row at
  every heading, and `the_mouth_is_wrapped_without_being_distorted` pins that the
  wrapped mouth keeps its approved width, keeps the image's aspect in *angle*, and
  that the wrap stops short of the rim.
  `the_mouth_is_painted_on_the_half_of_the_snout_that_faces_the_viewer` pins the
  frame, the cull and that a steeper tilt sits lower; `draw_curve_on_sphere`
  keeps its two core tests even though the face no longer calls it.
- Ball count, from the count test: 48, of which 11 on the head.
- Hips, measured against a rigid body through a full-rate turn: 0.2-1.1 px, and
  the hip-to-spine gap fixed at its authored 20 units.
- **Not verified by eye**: `PETZ_MODE=low` since the ear links and the spine
  simplification. It was judged once at 320x240, before those changes.

## Open, not fixed, not this session's fault

- **The feet shimmer while standing.** About thirty pixels on the rims of the two
  white feet change between frames 25 seconds apart, so it is a steady-state
  solver artifact rather than settling. Found while checking that the tail was
  still; untouched.
- **The mouth has almost no snout to sit on.** At the default 25 degree pitch
  the nose ball projects to the *bottom* of the muzzle (y 187 of a 163-190
  snout), because facing the camera its forward offset points at the viewer and
  looking down puts nearer things lower on screen. There are about 2 px of
  visible muzzle under the nose, so the arms sweep around the nose instead of
  sitting under it. Moving the nose up the snout is the fix if that ever matters.
- **The mouth is a stub in profile.** At a 90 degree turn the drawing slides to
  the muzzle's rim and the snout clips most of it away. Correct for a mark on a
  13-unit sphere, but a bigger mouth needs a bigger muzzle. Offered once and
  declined.
- **The mouth is fitted by eye.** `MOUTH_SPAN` and `MOUTH_RIM_GAP` were set by
  rendering and looking at 25 degrees; if the face reads wrong, those two
  constants are where to turn. `PETZ_MOUTH` / `PETZ_MOUTH_GAP` sweep it
  headlessly, and `PETZ_PITCH` frames it at any view angle.

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
- Mouth, live while it is being judged: `K`/`L` span, `N`/`M` tilt down the
  snout. `P` and the window title report both. There is no curl key - the shape
  is the drawing's.
- Instruments: `PETZ_HEADING=<deg>` pins the facing, `PETZ_DUMP_PPM=1` writes
  `/tmp/ours.ppm`, `PETZ_SWEEP=<rad/s>` rotates steadily, `PETZ_DIAG=1` paints the
  diagnostic colours, `PETZ_MOUTH` / `PETZ_MOUTH_GAP` set the mouth headlessly
  for a sweep, and `PETZ_PITCH` frames it at any view angle. Dump pixels rather
  than grabbing the window; the mouth's shape is judged by eye, since it is an
  image and not a curve.
- Watch for stale instances: several ragdolls at once all write `/tmp/ours.ppm`,
  which silently corrupts any measurement. `pgrep -x ragdoll` first.
