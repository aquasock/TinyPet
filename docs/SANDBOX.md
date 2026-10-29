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
cargo test --workspace                  # 37 tests
cargo run --release -p petz-sdl --bin ragdoll
```

Keys: drag to grab, **`W`/`S` forward and back**, **`A`/`D` turn left and right**
(hold, not toggle), `+/-` speed, `V` view pitch, arrows move the light, `Z`/`X`
ambience, `[ ]` ramp steps, `T` texture, `,`/`.` spine flex, `H`/`J` eye radius,
`K`/`L` eye angle, `P` print settings, `R` reset, `Space` pause, `Esc` quit.

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
- **Ears are removed.** They sat where they covered the eye in profile and got
  in the way of judging it. They need rebuilding: the reference says three balls
  a side in a shade darker than the coat (colour 35 on a Dalmatian, 55 on a
  Great Dane, 95 on a Bulldog).
- **Eyelids are removed.** Petz has no lid geometry — it parameterises one with
  `[Eyelid Color]` plus per-eye "eyelid height" and "eyelid tilt". Our lid-ball
  approximation was visible above the eye and read as a small ear.
- **The head is a cluster now, and the eye clips against all of it.** Seven
  balls — cranium, brow, two cheeks, jowl, muzzle, nose — so the eye is occluded
  by whichever ball is actually in front of it rather than by one oversized
  sphere. It still clips a little eagerly at some placements, which is what the
  depth bias (`U`/`I`) is for; that bias is a deliberate fudge, not a more
  correct occluder.
- **Physics is sagittal-only.** The solver runs in the animal's own plane;
  lateral offsets are authored, not simulated. A turning dog's tail swings
  fore-and-aft, not across the screen.
- **Feet slide during a turn.** They are driven to body-space targets rather
  than stepping to new plants.

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

The eye is now one unit drawn in order - outline, sclera, iris ring, pupil -
every layer through the same per-pixel clip, so all four go behind the head
together. Placement is by angle round the head (27 degrees) and radius (22)
against an 18-pixel cranium, both live-tunable, with a depth bias in the eye's
favour on top.

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
