# Handoff - creature head: eyes and ears - 2026-10-29

Read this as orientation, not law. Refresh live state (`git status`, `git log`,
`cargo test --workspace`) before acting on it. The living record is
[docs/SANDBOX.md](SANDBOX.md); this note is only where to stand.

## State

- **Eyes: settled.** Three nested shells - lid, sclera, pupil+iris - placed by
  angle round the skull. Verified from every heading by the user, then frozen.
- **Ears: settled for now.** Two balls a side, base and lighter tip, ordinary
  pinned body segments. Pushed.
- **Nose: not started.** The retail game was run tonight for reference and was
  not copied; the plan is our own nose and mouth, not retail's.
- Tree clean, `main` level with `origin/main` at `ae1d50d`.

## Landed tonight

57 commits, `78f67d3` (00:11) .. `ae1d50d` (03:14), all on `main`, all pushed.
The shape of it:

- depth buffer replaces the painter's sort; one rasterizer for every ball
- rims drawn as a flat backing disc, not a second sphere
- the eye rebuilt as three shells, lid rule included
- head trimmed back to a single ball
- ears tried as eye copies, then as angle-placed balls, then settled as two
  pinned body balls per side with the depth rule the eyes are exempt from
- `ae1d50d` adds the lighter tip ball

## Verified, and how

- `cargo test --workspace`: 60 tests, all green (39 core, 2 silhouette, 19 in
  the sandbox binary). Counts test expects 31 balls, 11 on the head.
- Ear tips: given a diagnostic colour no body ramp can reach, the framebuffer
  was dumped at 0/90/180/270 and pixels counted - 304 of 308 expected head on,
  and the two tips share a screen x in profile. Colour set back to 100 after.
- Not verified by eye: how the tip reads as a shape, and whether the head's
  outline blends as cleanly at the tip as it does at the base. The user last
  looked at the base-only ear.

## Decisions a fresh session must not re-litigate

- Projection is orthographic with a 25 degree pitch. No perspective divide: the
  captured balls only ever had 23 distinct diameters and every rect was square.
- Depth buffer, not painter's sort. Deliberate departure from the original.
- The eye is three shells with the rule "when the inner shells are not visible,
  the lid's open half fills with fur". That is what stops the eye popping.
- The head is one ball. A seven-ball cluster was tried and removed.
- Ears are placed by `pose()`, pinned, with `z_bias = (skull_depth - 1) - ear_depth`.
  They are not shells, not a bespoke sort key, and not angle-placed like eyes.
- Neither the eyes nor the ears have live keys. The ears were tuned from the
  keyboard and the settled numbers are now `EAR_*` constants with a test that
  pins them; `P` prints them next to the eye numbers.

## Known wrong or unproven

- The ear tip is a cap, not a flop: pinned means rigid. Retail has six balls an
  ear; ours is two, deliberately.
- The snout is the one part that bulges out of the skull on purpose: skull
  radius 26, muzzle radius 13 with its centre 24 from the skull's, so it stands
  about 11 proud. The part inside the head is hidden per pixel by depth, which
  is the point - there is no containment rule left to lean on.
- No mouth, no tongue, no tail detail beyond four balls.
- Physics is sagittal only - lateral motion is authored - and feet slide during
  a turn instead of stepping.

## Next step

Start the nose from the breed data in `docs/SANDBOX.md` ("Next: the nose"): a
nose ball plus two nostrils, with the retail asymmetry in nostril colour checked
against a capture before any of it is trusted.

## Continuation

- Sandbox binary: `rust/petz-sdl/src/bin/ragdoll.rs` (~1700 lines). Build with
  `cargo build --release -p petz-sdl --bin ragdoll`.
- A sandbox may still be running on `:0.0` under the window name
  "petz ragdoll sandbox". It no longer positions itself: pass
  `PETZ_WINDOW=x,y` to place it at creation, or let the window manager pick.
  Do not move it after it appears - that is the jump the user sees.
- Instruments: `PETZ_HEADING=<deg>` pins facing, `PETZ_DUMP_PPM=1` writes
  `/tmp/ours.ppm`, `PETZ_SWEEP=1` rotates and logs. `xwd` on an overlapped
  window captures the composited desktop, so count pixels in the PPM instead.
