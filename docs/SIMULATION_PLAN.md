# Attacking the simulation

The renderer is understood. This is about the other 5,719 exported methods, and
how to avoid spending five years porting things nobody runs.

## The number is smaller than it looks

`Petz 4.exe` exports 5,719 names. Parsing them:

```
exports                              5,719
name-parseable (real C++ symbols)    3,920
excluding template artefacts         2,975   across 689 classes
```

Template instantiations inflate the raw count (`ABV0`, `PAVAlpoSprite`, `H`,
`HJ` are mangled parameters, not classes). The working surface is ~3,000
methods.

Within that, the behaviour:

```
Goal*          241    thin subclasses, data-driven
Plan*          113    thin subclasses, data-driven
*Sprite*     1,090    PetSprite alone is 303
Linez/Brain     69    the LNZ interpreter
Area*           99
```

The behavioural core — goals, plans, sprites, linez — is **~1,500 methods, 51%
of the real surface**. The rest is rendering (done), UI, serialisation, and
plumbing.

## Four principles

### 1. Port by heat, not by inventory

Most of those 3,000 methods will never run in a normal session: the editor, the
screenshot tools, the web/download paths, the screensaver, half the goal
subclasses under a given breed.

We can measure instead of guessing. The executable exports all 5,719 names, so a
symbol map is a one-line transform of `analysis/pe_exports.py` output. With that,
`perf record -p <pid>` on the Wine process gives a ranked hot list with real
names, and `perf --call-graph` gives the call tree.

**Port in heat order.** Anything cold stays unported until something needs it.

### 2. Port the interpreter, not the content

`Goal*` and `Plan*` are mostly subclasses whose behaviour is parameterised from
`LNZ` and the behaviour chromosome. The engine instantiates them from data.

That means the port needs *one* goal interpreter and the data files — not 241
hand-written goals. This is the same shape as the renderer: small engine, large
data, and the data is on the user's disc.

### 3. Gate behaviour on a trace oracle, not on vibes

Frames made the renderer provable. The equivalent for behaviour is a *state
trace*: with the clock pinned (already done), log every goal activation, emotion
delta, action selection and brain tick, then diff a port's trace against the
original's.

We already have the machinery: an in-process DLL, IAT hooks, a pinned clock, and
a snapshot that makes runs start identically. Recording transitions is a
variation on the ball hook, not new infrastructure.

The bar is the same as for pixels: **traces match, or the milestone isn't done.**

### 4. Build one pet, one room, ten goals

A vertical slice that runs, not a subsystem that compiles. Suggested order:

1. load a `.pet`, render it, keep it alive on screen
2. needs → goal selection → action → animation, for 3–5 goals
3. one interaction loop (petting, feeding) with emotion deltas
4. sleep/wake, mood, and the idle behaviours
5. only then: genetics, breeding, adoption

Nothing about stages 1–4 requires the full 1,500.

## What to cut, deliberately

Out of scope for a first port, and honest about it:

- the Playscene Editor, screen saver, wallpaper, web/download paths
- ViaVoice speech, AOL installers, the CD's bundled DirectX
- multiplayer / DirectPlay remnants

These are a meaningful share of the export surface and none of them are why
anyone wants Petz back.

## Compatibility is a hard requirement

A port that can't load a 2003 community `.pet` will be rejected by the people
who care. So:

- read the existing `.pet` / `.LNZ` formats (documented, text-based)
- write them back compatibly
- keep breed `.cat`/`.dog` handling working at the *resource* level (see the
  ABI note in BINARY_NOTES.md — these are DLLs importing the exe by ordinal,
  so we parse their resources rather than loading them)

## Effort, honestly

Of ~1,500 behavioural methods:

- Goals and Plans are largely declarative; the interpreter is a few hundred lines
- Sprites share most machinery through `AlpoSprite`
- The genuinely bespoke logic is the goal arbitration, the emotion model, and
  the LNZ brain

Call it **300–500 functions of real logic** plus data. That's a year-scale
project for one person, or a few months for a small group — *provided* every
stage is oracle-gated. Without the oracle it's unbounded, which is how most
remakes die.

## Next three steps

1. **Symbol map + profile.** Generate `rva -> name` from the export table, run
   `perf` on a live session, and produce the hot list. This turns "5,719" into a
   ranked backlog.
2. **State-trace oracle.** Hook goal/emotion/action transitions and record them
   under the pinned clock. Verify the original produces identical traces across
   runs before trusting it.
3. **Vertical slice skeleton.** A Rust workspace with `petz-core` (no platform)
   and `petz-headless`, loading one `.pet` and rendering it via the decoded
   renderer path.

Step 1 is the one that changes the plan's shape. Until it's done, every
scheduling estimate is a guess.
