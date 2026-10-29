# The sandbox: current state

This is a record of where the reimplementation actually stands, not where it is
heading. It exists because the top-level README still describes the M0 oracle,
and because several of the numbers below were measured rather than assumed —
they are worth not re-deriving.

## What changed since the oracle

The original goal was a **bit-exact** port. That is no longer the goal. It was
relaxed deliberately: the requirement is now that the pets **look and act** like
the originals, without reusing game data or reproducing the original engine's
internals. The rendering work below is built on that basis.

Two consequences worth stating plainly:

- Petz's data is now **reference**, not ground truth. We read it to learn what
  the artists had to work with, then build our own thing.
- Physics and animation only have to be *indistinguishable*. Rendering is the
  only layer where exactness still matters, and only because it is cheap to
  prove.

## Running it

```sh
cargo test --workspace                  # 60 tests
cargo run --release -p petz-sdl --bin ragdoll
```

Keys: drag to grab, **`W`/`S` forward and back**, **`A`/`D` turn left and right**
(hold, not toggle; `Q`/`E` are the same), `+`/`-` speed, `V` view pitch, arrows
move the light, `Z`/`X` ambience, `[`/`]` ramp steps, `T` texture, `,`/`.` spine
flex, `Tab` ball numbers, `P` print settings, `R` reset, `Space` pause, `Esc`
quit.

There are no controls for the eyes or the ears any more. Both were tuned from
the keyboard, then the settled numbers were written into the source as defaults
and the keys removed. An adjustment now is a constant in `build()` or `pose()`,
not a live tweak.

The big number in the top left is the facing: **0** away from the viewer, **90**
right, **180** towards the viewer, **270** left. It shows the *actual* facing,
including the turn rate limit and per-part lag, so it eases rather than jumping.

## Layers

| module | what it does |
|---|---|
| `petz-core::spans` | table-driven disc rasterizer (the proven part) |
| `petz-core::surface` | 8bpp indexed surface, spans, palette |
| `petz-core::shade` | analytic sphere normals, Lambert, quantised palette ramps |
| `petz-core::fur` | 8bpp patterns that choose a ramp, independent of light |
| `petz-core::ragdoll` | Verlet solver: springs, gravity, contacts, grab, drive |
| `petz-core::gait` | phase-driven foot cycle, two-bone IK, spine flex |
| `petz-core::project` | orthographic 2.5D projection, body transform, depth |
| `petz-sdl` | window, creature layout, walker, rendering |

## Why the projection is what it is

The captured data settles it: 330,902 rendered balls across 1,740 frames used
only **23 distinct diameters**, and every rect was square. A radius that never
changes with distance means **no perspective divide** — the projection is
orthographic. Depth decides position and occlusion, never size.

That is why a dog walking away is a body transform plus a fixed view pitch, not
a camera:

```
screen_x = world_x
screen_y = -(z cos p) - (y sin p)
depth    = y cos p + z sin p
```

A level view with a side-on heading reproduces the old 2D layout exactly, which
is asserted as a regression test.

## Verified, with the numbers

- **Silhouette**: a captured 19 px ball is reproduced 285/285 pixels.
- **Shading**: a shaded disc covers exactly the same pixels as the flat fill.
- **Walk**: 2 s of simulation travels speed × time within 1 px; feet planted
  >35% of the cycle; front foot sweeps >20 px relative to its hip.
- **Ragdoll**: a held body is one connected mass; its spine keeps its length;
  the solver is bit-identical across two runs.
- **Grab**: nothing ends up stranded more than a body length from the spine
  after a drag and release.
- **Eyes**: one visible in profile, two facing the camera, none facing away;
  the arc where both are up is at least 60 degrees, centred on the front; and the
  radius cannot flip an eye (see below).
- **Turn**: rotation is rate-limited and reaches the head before the tail.

## Known wrong, or unknown

- **The skeleton is hand-authored.** Neck, eyes and limb lengths are guesses.
  The breed files carry real coordinates in `[Add Ball]` (`base x y z colour …`)
  and the neck size lives in the packed `SCP` resource, which we have not
  decoded. Until then, proportions are plausible rather than correct.
- **Ears are two balls a side, not six.** The reference puts six balls in an
  ear, in a shade darker than the coat (colour 35 on a Dalmatian, 55 on a Great
  Dane, 95 on a Bulldog). Ours is a base and a lighter tip, which reads as an
  ear at this fidelity but cannot flop. See "The ears" below.
- **The ear tip is a cap, not a flop.** It follows the base rigidly because it
  is pinned with it. Nothing here gives an ear its own swing.
- **Eyelids are removed.** Petz has no lid geometry — it parameterises one with
  `[Eyelid Color]` plus per-eye "eyelid height" and "eyelid tilt". Our lid-ball
  approximation was visible above the eye and read as a small ear.
- **The head is a single ball again.** A seven-ball cluster was tried as a way
  to give the eye a socket and was removed once the shell model made it
  unnecessary: the eye is now a fixed disc whose contents come and go, so it no
  longer depends on the head being made of several balls.
- **Physics is sagittal-only.** The solver runs in the animal's own plane;
  lateral offsets are authored, not simulated. A turning dog's tail swings
  fore-and-aft, not across the screen.
- **Feet slide during a turn.** They are driven to body-space targets rather
  than stepping to new plants.

## Depth

Rendering is depth-buffered: every ball is rasterized with its sphere's *surface*
depth per pixel, tested against a buffer, and drawn. Balls can be drawn in any
order.

This replaced a painter's sort, and with it a pile of special cases that existed
only to patch the sort's mistakes: a per-ball sort key, a containment list for
"which ball is inside which", a head-ball list for the eye's socket, and
per-eye occlusion closures. Every bug in that area for several sessions was the
same mistake in different clothes - centre-depth ordering cannot express a ball
that bulges towards the viewer by its own radius.

The original engine has no z-buffer and paints far-to-near. This is a deliberate
departure: the goal is "looks and acts like", not bit-exact, and per-pixel depth
is both more correct and less code.

Cost is a `f32` per pixel - 1.2 MB at 640x480, which is nothing on desktop and
the reason a watch port would render at half resolution.

## The eye, and why it was hard

Five separate faults, each of which looked like the same symptom - an eye
vanishing at the wrong angle:

1. **The eye was compared against the head by centre depth.** A head ball of
   radius 26 has its centre a full radius behind its own surface, so the head's
   centre crossed in front of the eye long before the eye was actually behind
   the head.
2. **Sorting the eye in front fixed that, then made it pop** - one binary cut
   instead of a shrinking crescent.
3. **The eyes were geometrically inside the head.** They sat 14 from a skull
   centre of radius 26; under correct occlusion they were simply buried.
4. **Each part of the eye clipped on its own.** The outline was drawn in the
   global rim pass, where nothing masked it, so the brown ring blew out while
   the blue ring stayed.
5. **The lateral placement was wrong.** Two parallel lines pushed sideways
   eventually pass through the head. Placing the eye *by angle round the skull*
   keeps it tangent to the surface, and makes the facing test
   `radius * cos(angle +/- heading)` - radius-independent, so width can no
   longer flip an eye on or off.

The eye is built the way the original builds it: three nested shells, outside in.

1. **Eyelid** - a ring, a line across the eye dividing coat colour from open
   eye, and the rule that ties them together: **when the inner shells are not
   visible the open half fills with fur**, so the eye closes instead of leaving
   a hole in the head. Drawn last, never culled. This is what lets the eye read
   as an eye from every angle.
2. **Sclera** - a white sphere drawn as a *full circle*, clipped only by the
   head. It never rotates, so it never pops.
3. **Pupil and iris** - one texture, sliding inside the sphere at
   `radius * sin(facing)`, clipped by the sphere's own rim.

That third point is the whole trick, and it is why the original's eyes never
popped: the sphere's rim clips the pupil, rather than the head clipping the eye.

Placement is by angle round the head and radius from the skull's centre, with a
height above that centre, a depth bias in the eye's favour, and a lid. The eyes
are **pinned** rather than simulated: left to the solver they drifted five
pixels apart and one sat higher than the other.

The defaults as of this pass, all tuned by eye against the retail game:

  angle 25 degrees   radius 29   lift 12   bias 1.5
  lid drop 0.26      lid tilt 0.23

The nose is a single black ball at the tip with **no rim** - an outline in the
body's brown around a black nose reads as a brown ring - and it sits clear of the
muzzle's own sphere so the muzzle's depth does not swallow it.

The eye is, as far as we can tell, finished. Every number above had a live
control while it was being tuned; the keys are gone and the numbers are
constants now.

## The ears

Two balls a side, an ordinary body segment in everything but placement: base
(colour 95, radius 10) and tip (colour 100, radius 7), both pinned, both placed
by `pose()` every frame from the skull's position:

```
base  sk.x - 8   sk.y - 30   r 10
tip   sk.x - 18  sk.y - 44   r 7
side  +-10       (body-lateral, not screen)
```

The lateral offset is body-lateral and goes through the projection, so the two
ears are mirror images facing the camera and share a screen x in profile. Forcing
that in screen space instead made them the same height at every heading, which is
wrong - one ear really is above the other at 90 degrees.

Depth is one bias, `z_bias = (skull_depth - 1) - ear_depth`, and it does both
jobs at once: the ear is past the skull's rim, drawn half a unit behind the
skull's centre, so the head's outline no longer runs across the ear; and it is
behind the skull's own surface, a full radius in front, so the head still covers
the ear where they overlap. That is exactly how two body segments meet, and the
eyes are the only exemption.

Tried and dropped, so nobody pays for it twice: drawing ears through the eye
renderer as copies of the eye shells; a bespoke z order for the ears; deriving
ear height by angle round the skull the way the eye is placed - the eye orbits
the surface, but an ear is attached at a point. Pinning costs the flop: a pinned
ear is rigid, and giving it swing means giving it its own solver drive.

## Next: the nose

What the breed data says, before any code:

```
;L nostril      244, 244, -1, 0, -3, -1, -1, 1     group -1
;R nostril       15, 244, -1, 0, -3, -1, -1, 1     group -1
;nose (bottom)  244, 244, -1, 0, -1, -5, -1, 1     group -1
```

Three balls, all in group -1 like the eyes, and the nose one is named
"nose (bottom)" - so the visible nose is a ball at the underside of the snout
with two nostril balls on it, not a flat marking. Both nostrils and the nose are
white-outlined (outline colour 244 is black) with a thick outline type of -3 on
the nostrils.

Worth noting the asymmetry in the table: the left nostril is colour 244 (black)
and the right one is colour 15 (a light grey). That is either a quirk of this
breed file or a genuine difference in how the two are drawn, and it is the first
thing to check against a capture.

## Reference findings worth keeping

From `Resource/Dogz/*.dog` (LNZ resource, `[Ballz Info]`):

- An eye is **two balls**: `L eye` colour 245 = white sclera, `L iris` colour
  244 = black pupil. The **iris is the pupil's outline colour** — 6 (teal) on a
  Dalmatian, 25 and 115 on others — at thickness 4 against the sclera's 1.
- Measured on screen: sclera diameter 20, pupil 12 — the pupil is 60% of the eye.
- Three eyebrow balls per eye, colour 19.
- `sizeDif` in `[Ballz Info]` is a delta against a base radius that is not in
  the file, so it cannot give absolute sizes. `[Paint Ballz]` and `[Add Ball]`
  express sizes as a percentage of the base ball, but only for markings and
  add-balls.

## Instruments

- `PETZ_SWEEP=1` rotates the animal and logs the iris pixel count per heading.
  This is how the eye occlusion was found; screenshots were not reliable enough.
- `tools/pair-capture.sh` + `tools/pairdiff.py` capture the retail game's own
  pixels alongside the render blocks. Note the unresolved result in
  `docs/BINARY_NOTES.md`: the replayed blocks do not reconstruct the captured
  frames, so the block stream is not a complete record of what draws the scene.
