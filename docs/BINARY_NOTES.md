# Petz 4 — binary findings

Everything here was read out of the retail binaries on the supplied discs. Each
claim is tagged:

- **[verified]** — observed directly in the file or in a running process
- **[inferred]** — a reading of the code that I have not yet proven by experiment

Keep that distinction. The whole project rests on not believing things we have
not checked.

## Provenance

| item | value |
|---|---|
| retail media | Catz 4 + Dogz 4 CDs, disc date 1999-09-09, InstallShield 6 |
| `catz4.iso` | `bf4106e8fcc572947155fbe2021e78a645dfc48713e2b23cf28337b1da71ec4f` |
| `dogs4.iso` | `86987cb1d19cc3f5f256149343f1e2f17df12451e5074639c7d9167437f58f12` |
| `Petz 4.exe` | `6f11d4ffd630694096fe95b31d988a1c7a0c1a0c1a921e78577278a4408d3b79` |

Both discs install into one directory; each drops a `Catz4.pfm` / `Dogz4.pfm`
marker so the second install merges rather than replaces. [verified]

Install payload sits in `data1.cab` (InstallShield 6). `cabextract` and `7z`
both refuse it; `unshield` works. [verified]

## The executable

`Petz 4.exe` — PE32, i386, MSVC 6 (linker 6.0), **unpacked**, 5,582,096 bytes.

```
.text    0x1952da     .rdata  0x07f13e     .data  0x01d000
PFM_univ 0x000004     .rsrc   0x1ba898     .reloc 0x026aca
```

Leaked build tree (`__FILE__` strings): `C:\Petz\Alpo\Source\Engine\*.cpp`
(`WDraw.cpp`, `Xdib.cpp`, `XBallz.cpp`, `LinezCache.cpp`, `DxSound.cpp`,
`XScrSprt.cpp`, `Xflmstrp.cpp`, `XSex.cpp`, `XPlatGlobals.cpp`, `WSystem.cpp`,
`Wave.cpp`, `xdownload.cpp`, `x_jerror.cpp`).

Version string: `PFM_[V4.015] (c) 1999 The Learning Company Properties, Inc.
(Hi Mom, indeed!)` [verified]

Engine codename is **Alpo**; the sprite base class is `AlpoSprite`.

## The export table is a free architecture map

**[verified]** `Petz 4.exe` exports **5,719 named C++ methods** (7,060 function
entries, ordinal base 7). Not a stub table — real MSVC mangled names:

```
rva      ordinal  symbol
002ddf0   6054   ?SetBallTextureInfo@XBallz@@QAEXAAVCircleRenderBlock@@...
002e810   3352   ?DisplayBallzFrame@XBallz@@QAEXPAVXDrawPort@@PBU?$XTRect@HJ@@...
003ad50   6578   ?XFillRotatedCircle@XDrawPort@@QAE_NPBVCircleRenderBlock@@@Z
```

Roughly 700 distinct class names appear. Method counts (by mangled-name suffix,
so template types inflate a few):

```
PetzSprite       303      XDrawPort        53      XBallzDataBase    7
AlpoSprite       101      Linez            44      XDib              5
CharacterSprite   73      XBallzData       35      Vertex4List       5
ScriptSprite      66      XBallz           28
Area              47      XSex             26
                          Filmstrip        26      GoalManager      24
```

Use `analysis/pe_exports.py` to regenerate.

## Rendering: what actually produces the look

### It is a software rasterizer, not a 3D pipeline

**[verified]** No `ddraw.dll`, `d3d*.dll`, `opengl32.dll` or `glide*.dll`
imports. Drawing goes through `XDrawPort` into DIBs. DirectSound is reached
dynamically via `CoCreateInstance(CLSID_DirectSound, ...)`.

### The per-pixel loop is integer and table-driven

**[verified]** `XDrawPort::XFillRotatedCircle` (`0x43ad50`):

```
43ae48:  movsx  edi, WORD PTR [ecx]     ; int16 per-row edge delta
43ae4f:  add    ebp, edi                ; accumulate span start
...
43ae71:  mov    al, BYTE PTR [ebx]      ; per-row run length / coverage
43ae8b:  shl    eax, 0x10
43ae8e:  mov    ax, bx
43ae98:  rep stos DWORD PTR es:[edi],eax ; fill span with one colour
```

That is a Bresenham-style scanline circle fill driven by a static table. The
tables are built once at startup by `_initStaticCircleLookup` (`0x3a390`) /
`_initStaticCircleTables`; the row table lives at `0x63b458`. [verified]

The dispatch at the top of the function switches on a `XDrawPort` field with
cases `0xff`, `0`, `8`, `9` — almost certainly bits-per-pixel selection for the
destination surface. [inferred]

### Floating point exists, but not in the inner loops

**[verified]** Across the whole executable, 6,854 of 589,742 disassembled
instructions are x87 (1.16%). In the ball path it is concentrated in setup:

```
fpop  insns  function
  19    203  XBallz::SetBallTextureInfo
  97    765  XBallz::DrawEyeball
  37    442  XDrawPort::XFillRotatedCircle
  58    295  XDrawPort::_initStaticCircleLookup
```

**This is the single most important fact in the document.** MSVC 6 keeps
`double` intermediates in 80-bit x87 registers. Any port that computes the same
thing in SSE2 doubles will round differently, land a ball ±1 pixel off, and
differ in a handful of pixels per frame. That is exactly the "kinda sorta"
failure mode. The fix is to keep the setup maths at 80-bit precision
(`long double` on x86-64 is genuinely 80-bit) and match the store/load
rounding pattern.

### The ball pipeline

**[verified]** by export table plus disassembly of `DisplayBallzFrame`:

1. `XBallzData` holds raw per-frame ball data (`BDT`/`BHD` resources).
2. Deformation: `ProjectBallz`, `MoveBallz`, `ModifyRawBallzData`,
   `DoSpeciesSpecificMods`, and per-species shape hacks —
   `CatzExtendLegLength`, `CatzEnlargeFeet`, `DogzExtendLegLength`,
   `DogzEnlargeFeet`.
3. `SetBallTextureInfo(CircleRenderBlock&, BallState&, pfvector<XPointRot3_16>&, ...)`
   builds a render block from **16-bit fixed-point** rotated points.
4. `XFillRotatedCircle` fills the shape from tables.
5. Colour passes: `DrawPaintBallz`, `DrawEyeball`, `DoDrawLine`,
   `DrawAllLines`, `DrawAllWhiskers`.
6. Fur: `XFurInfo` (patterns, ramps, `SetFurColor`, `ChangeFurColorArea`) with
   lookup tables `s_fuzzTables@Vertex4List` (rva `0x23b3c8`).
7. Depth ordering: `GetZDepthBall`, `HandleCaseZSort`.

### Colour: the renderer writes 8bpp palette indices

**[verified]** Breed art is 8bpp indexed DIB. Example: `COTTON.BMP` inside
`Bunny.cat` is 105x89, 1 plane, 8 bpp, uncompressed, 256-entry palette.

**[verified]** The drawing surface the game renders into is **8bpp**: the shim
records `DIB 640x480 8bpp stride=640`, and captured frames use ~170 distinct
palette indices out of 256.

**This contradicts an earlier note in this document, which was wrong.** I had
concluded the surface was true colour because a screenshot of the window showed
~970 distinct colours. That screenshot was the *composited* window: Wine maps
the 8bpp surface through the palette into a 24-bit visual. The renderer's own
buffer is indexed, which is the far better news for bit-exact reproduction —
there is palette-index arithmetic to match, not blend rounding to guess at.

Other surfaces the game creates alongside it (32bpp at the same size) are blit
targets and helpers, not the scene buffer.

### Determinism: achieved

**[verified]** With the clock pinned to a query counter, two cold runs produce
**byte-identical** 640x480 8bpp framebuffers at the same present indices:

```
dib-f1000-8bpp.raw  IDENTICAL  f0a18ba45c4c1a46
dib-f2000-8bpp.raw  IDENTICAL  08a01b15d10368b1
dib-f3000-8bpp.raw  IDENTICAL  c680379ab4fe0705
dib-f4000-8bpp.raw  IDENTICAL  53536de2a8f4ec83
```

Different indices give different hashes, so these are real distinct frames and
not a blank surface repeated. Reproduce with `tools/pin-run.sh NAME`.

Two things this cost, both worth recording because they were silent failures:

- Keying capture on the clock starves: the title screen can stop querying time
  entirely. Capture is keyed to drawn presents instead, while time is driven by
  a separate query counter.
- "Largest surface" is the wrong heuristic. It picked a 1024x768 canvas the game
  had already deleted, and `WriteFile` with a stale pointer writes 0 bytes
  without failing loudly. The dump now prefers the surface matching the screen
  size and logs any short write.

### Native resolution is 640x480

**[verified]** With no window manager present, Wine reserves 8x34 px for
decorations that nobody is drawing, and the game surface came out 632x446 (or
1016x734 on a 1024x768 display) with the framebuffer cropped. Setting
`HKCU\Software\Wine\X11 Driver` → `Decorated` = `N` yields an exact 640x480
surface. That is the only reliable capture geometry found so far.

Earlier 1024x768 captures on a large desktop were **not** native: they were
the game fullscreening to the display. Do not treat 1024x768 output as
authoritative.

## Assets

**[verified]** Resources are ordinary PE resources with **string type names**:

```
BDT  ball data table        BHD  ball header
BMP  8bpp palettized DIB    FLM  filmstrip (sprite frames)
LNZ  "linez" brain          FLH  filmstrip header
SCP  script                 WAV  sound      TXT/LOG  text
```

`Petz 4 Rez.dll` alone: 25 `BDT`, 9 `BHD`, 27 `BMP`, plus `FLM`/`LNZ`/`SCP`/`WAV`.
Use `analysis/pe_resources.py --list` or `--extract BMP --out DIR`.

### Breeds are DLLs, and they link against the exe by ordinal

**[verified]** `Bunny.cat` is a 540 KB PE32 DLL:

```
exports:  GetDLLVersion, GetFilmstrip, GetLoadInfo, GetSprite
imports:  KERNEL32, MSVCRT, verbassert.dll, Petz 4.exe (by ordinal)
          e.g. ordinals 2764, 3323, 2898, 5443, 2752, 6530, ...
```

**[verified]** `GetSprite` allocates a ~19 KB object and installs vtables that
point into the host executable.

This is the hardest constraint in the project. 25 years of community breeds are
binary plugins against a specific engine ABI. A reimplementation cannot
`LoadLibrary` them; it can either reproduce the ABI (ordinals shift between
builds, so this is fragile) or parse their resources and reimplement the
semantics. That decision belongs in M1, before the content pipeline is designed.

## Content layout

**[verified]** The exe contains hardcoded path literals:

```
\Resource\Area\*.env          \Resource\Catz\*.cat
\Resource\Dogz\*.dog          \Resource\Clothes\*.clo
\Resource\Toyz\*.toy          \Resource\Wallpaper\*.bmp
\Resource\Petz 4 Rez.dll      \Resource\Petz 4 Snd.dll
\Adopted Petz\                \PetzPix\
```

So the install root holds the exe, and everything else lives under `Resource\`.

## Preferences

**[verified]** Settings live in `HKEY_CURRENT_USER\Software\PF.Magic\Petz 4`
(read in `CShlGlobals::LoadGlobals`, references at `0x4553xx`).

The reader is a small helper at `0x452b70`:

```
RegOpenKeyExA(HKEY_CURRENT_USER, key, 0, KEY_ALL_ACCESS, &hk)
RegQueryValueExA(hk, name, NULL, NULL, dest, &size)   ; size = 1, lpType = NULL
```

The game does not check the value type, only that it gets one byte back. So the
values are 1-byte `REG_BINARY`. [verified for the call shape; the on-disk type
is [inferred] until we read back a key the game itself wrote]

Value names, as literals at `0x61e3fc`–`0x61e518`:

```
Warn About No Voice Rec       Warn About No Family Model
Warn About No Breed           Warn About Workspace
Warn About Performance        Warn About Making Pet
Warn About 8Bit Sound         Warn About Bad ACM
Warn About No ACM             Warn About Sound Init
Warn About Monitor            Warn Used Nametag
Confirm Web Launch
```

Set to `00` to suppress. See `prefs/petz4-prefs.reg` and
`tools/seed-prefs.sh`. This is what unblocks unattended capture: the speech
stack (IBM ViaVoice via `smapi.dll` / `UVocab.dll` / `vtcommon.dll`) has no
hardware under Wine and raises a modal on startup.

Other knobs seen in the same table: `Sound Effects`, `Max Number Movies`,
`Camera Movie Mode`, `Photo Has Background`, `Auto Save Photos`,
`Number Times Run`, `Time First Launched`, `First Time Run`,
`Serial Number Catz`, `Serial Number Dogz`.

## What this means for a port

Bit-exactness is plausible *because* the pixel work is integer and table-driven,
and the palette is fixed. It is not guaranteed, and these are the things that
will break it, in order of expected damage:

1. **80-bit x87 intermediates vs SSE2 doubles** in ball setup maths.
2. **Z-sort tie-breaking** (`GetZDepthBall`, `HandleCaseZSort`).
3. **Ambient state** — anything seeded from `timeGetTime`, `rand()`, or system
   metrics.
4. **GDI text** (`AdoptFonts`, `PetzText`) — FreeType will not match GDI.
5. **Audio mixing** — not bit-exact by nature; define the bar as identical
   samples pre-mix.

## Inline hook on the ball rasterizer

**[verified]** `XDrawPort::XFillRotatedCircle` (RVA `0x3ad50`) is now hooked by
patching its first five bytes with a rel32 jump; the stolen bytes
(`83 EC 24 / 53 / 55` = `sub esp,0x24; push ebx; push ebp`) form a clean
instruction boundary and are replayed from a trampoline. The prologue is
verified against expected bytes at install time and the hook is skipped if it
does not match.

**[verified]** The hook fires and real call arguments are captured:

```
fill call 5
drawport=0x08682d88  block=0x0012f508
block head: 00 00 00 00 | 1c 00 00 00 | f4 00 00 00 | 00 00 00 00
```

`block` sits on the stack (`0x0012f508`), so `CircleRenderBlock` is a stack
local, rebuilt per ball per frame. Its first fields are constant across the
observed call, so a representative sample is meaningful.

**[verified]** Replacing the return address on the stack with a post-call stub
works in principle — the stub is entered after the callee returns — but it does
not reliably coincide with the armed sample call. Two candidate causes, not yet
distinguished: the callee's SEH epilogue restoring a saved frame rather than
popping the patched return address, or nested fill calls clobbering the single
saved-return slot.

**[verified]** Fill calls are rare early: roughly 5-8 calls in the first minute
while the game is on splash and title screens, so each experiment iteration
costs ~90 s. Sampling should be keyed to a scene-arrival condition rather than
a fixed call number.

Simpler alternative not yet tried: snapshot the surface at fill *N* and again at
fill *N+1*. With fills this sparse, the diff between consecutive snapshots is
essentially one ball's worth of pixels, and it avoids the whole return-address
problem.

### The shim cannot run on a real desktop

**[verified]** With the proxy installed, the game raises
`Verbassert.dll: Unhandled Exception / c0000005 ACCESS_VIOLATION` on a real X
desktop. With the retail `UVocab.dll` restored it runs normally on that same
desktop. The fault is not yet localised; candidates are the clock hooks
(the game may derive geometry or divisors from elapsed time) or the
`BitBlt`/`StretchBlt` return values, which are stubs.

Consequence: `tools/shim.sh on|off` exists, the shim is **off** by default, and
capture runs are headless only. Do not hand the shim to someone who just wants
to play.

### A measurement bug worth remembering

**[verified]** Fill activity was logged only every 5th call, so a run with one
to four fills looked identical to a run with none, and the retry loop treated
it as "scene never reached". That produced a confident but wrong diagnosis.
Every fill call is now logged individually; fills are single-digit per minute,
so the volume is trivial.

The general lesson: the instrument was lying, not the thing being measured.

### The first real ball capture

**[verified]** With an adopted pet in the prefix (`Snowball.pet`) the ball hook
fires reliably, and consecutive-snapshot differencing isolates a single
rendered patch:

```
surface : 896x736 8bpp stride=896
ball    : 35x26 at (250,276)  changed=556
block   : 00 00 00 00 | 12 00 00 00 | f4 00 00 00 | 00 ... 00 b6 43 00 fc ff ff ff
```

Note the surface is **896x736**, not the 640x480 window: the game draws pets
into a larger buffer and then composites. Capturing the window was never going
to see this.

**[verified] The capture is byte-identical across independent cold runs**
(`sha256 2e2d1b473b290ea760583a87…`), from a restored prefix snapshot, twice.
That is the first end-to-end determinism result that covers *what the game
renders*, not just the framebuffer it ends up presenting.

**[verified] A ball is not a flat fill.** Inside the 35x26 region:

- 31 distinct palette indices
- all 26 rows contain multiple colour runs

So `XFillRotatedCircle`'s `rep stos` spans are only part of the story; the
per-ball result is textured, with per-pixel index selection happening in the
same or a tightly coupled pass. Reproducing a ball means reproducing that index
selection, not just an ellipse.

Changed-mask for the capture (`#` = differs after the fill):

```
......#######......................
....###########....................
...#############...................
..###############..................
..###############..................
.#################.................
.#################.................
###################...#######......
###################.###########....
################################...
##################.##############..
##################.##############..
.################.################.
.################.################.
..##############.##################
..##############.##################
...################################
....###########.###################
......#######...###################
.................#################.
.................################..
..................##############...
..................###############..
...................#############...
....................###########....
```

Two overlapping lobes with a gap between them: either one ball drawn in several
passes or two adjacent balls captured in the same interval. Consecutive-snapshot
differencing cannot yet tell those apart, which is the next thing to pin down.

### What a ball actually is

**[verified]** The captured 35x26 patch contains **two** balls, not one. They
separate cleanly by palette index:

| index | pixels | bbox | notes |
|---|---|---|---|
| 18 | 279 | 19x19 | full disc; pi*r^2 for r=9.5 is 283 |
| 201 | 213 | 17x19 | occluded by the other ball |
| 247 | 34 | - | sits at radius 15-27 from ball A's centre |
| 2 | 30 | - | same region |

So the earlier "31 distinct indices" was two balls plus their surroundings, not
texture. **A single ball is a filled disc in one palette index.** The 247/2
pixels belong to the second ball's region, not to a fuzz rim on the first.

### The disc is not an analytic ellipse

**[verified]** Ball A's row spans are asymmetric about its centre:

```
y=0..3   : 7, 11, 13, 15
y=4..9   : 15, 17, 17, 19, 19, 19
y=10..15 : 18, 18, 16, 16, 14, 14
y=16..18 : 13, 11, 7
```

Top half and bottom half differ by one pixel in rows 3-11. A brute-force fit of
a rotated ellipse (theta 0-180 deg, axes 8.5-10.0) leaves **8 of 19 rows
wrong**; the best fit is an axis-aligned r=9.5 circle and it still misses.

Conclusion: the silhouette is produced by the game's own fixed-point
projection (`XPointRot3_16`) and static circle tables, whose discretisation is
not any smooth analytic shape. Reproducing a ball bit-exactly therefore means
porting that integer pipeline - not drawing a better circle.

That is good news for feasibility (integer, table-driven, deterministic) and
bad news for anyone hoping to approximate it convincingly.

### Fill activity is bursty

**[verified]** A static scene re-fills the same ball identically, so
consecutive-snapshot differencing sees zero change and yields nothing after the
first interval. Three fills produced one capture. To get many samples the pet
has to actually move, or capture must move to a per-call method.

### The span table, decoded

**[verified]** The static table at VA `0x63b458` is a per-radius row-width
lookup. Radius `r` occupies exactly `r` bytes at offset `(r-1)*r/2` (the
triangular indexing seen in the disassembly):

```
r=4 : 2,4,4,2
r=5 : 3,5,5,5,3
r=9 : 5,7,7,9,9,9,7,7,5
r=18: 6,10,12,14,16,16,16,18,18,18,18,16,16,16,14,12,10,6
r=19: 7,11,13,15,15,17,17,19,19,19,19,19,17,17,15,15,13,11,7
```

**[verified]** Ball A's measured row widths match table entry `r=19` exactly for
**13 of 19 rows**:

```
observed : 7, 11, 13, 15, 15, 17, 17, 19, 19, 19 | 18, 18, 16, 16, 14, 14 | 13, 11, 7
table 19 : 7, 11, 13, 15, 15, 17, 17, 19, 19, 19 | 19, 19, 17, 17, 15, 15 | 13, 11, 7
```

**[verified] The six deviating rows are not explained by the second ball.** Each
is exactly one pixel narrower, and no foreign-index pixel occupies the missing
column (checked explicitly). So there is a systematic one-pixel reduction on
rows 10-15 that is something else: a per-ball scale/squash applied after the
table lookup, or a second clamping stage in the fill.

This is the sharpest statement of the reproduction problem available so far:
the silhouette is table-driven, and the remaining difference is a small integer
effect in a specific row range rather than any analytic shape question.

### The LNZ file is the whole colour and shape spec

**[verified]** Breed `.cat`/`.dog` files carry two `LNZ` resources, and they are
**plain text** with commented column headers. `B+W Shorthair.cat` -> `BW.LNZ`
(15,139 bytes, 28 sections). This is the system the community has been editing
for 25 years, and it is the authoritative description of how a pet is drawn.

Geometry sections: `[Head Enlargement]`, `[Face Extension]`, `[Body Extension]`,
`[Leg Extension]`, `[Feet Enlargement]`, `[Ear Extension]`, `[Project Ball]`,
`[Ball Size Override]`, `[Default Scales]` (pet scale 87, ball scale 96).

Colour and surface, with the file's own help text:

```
[Ballz Info]        ;col outCol spklCl fuzz otlntTyp sizeDif group texture
  35, 244, 244, 1, -1, 3, 4, 1    eBall_ankleL,
  35,  25, 244, 0, -2, 5, 5, 1    eBall_elbowL,

[Paint Ballz]       ;base ball diameter(% of baseball) direction colour
                    ;outlinecolor fuzz outline group texture
  16, 50, 0,1,.3, 47, -1, 0, -1, -1, -1

[Texture List]      ;FUZZ -1 = no outline, 0 = half outline, >0 = thickness
  \art\textures\Plush.bmp    1
  \art\textures\hair10.bmp   1

[Fur Color Areas]   ;FurColorTrait  ballWithBaseColor  colorAreas
[Color Info Override]  ;individual variation, per ball
[Outline Color Override]
[Fur Markings]      ;groupType 0=Normal 1=BodyArea 2=Primary 3=Paw
```

**[verified]** The texture paths resolve to `BMP` resources inside
`Petz 4 Rez.dll`, not to loose files: `PLUSH.BMP` (128x64), `HAIR10.BMP`
(128x64), `HAIR6.BMP`, all 8bpp. `Catz 4 Rez.dll` separately carries fur
pattern textures (`CALI2`, `MOTTLED2`, `MOTTED5`, `HAIR4`, `CATZCORNER`).

**[inferred]** So a ball's colour is a **single palette index** from the LNZ,
and the per-pixel variation (~19 indices clustered around a base) comes from
sampling a texture as a shade/height map and offsetting that index. That fits the
observed data exactly: a ball with base colour N renders as a narrow band of
indices around N with texture-driven variation, and the texture's own 0-255
indices never appear on screen.

**Consequence for the port:** the ballz system is a *data interpreter*, not a
renderer with hardcoded shading. Shape comes from tables and LNZ overrides;
colour comes from an index plus a texture-driven offset. That is the most
portable possible answer - the engine is small and the content is data.

### Ball data and fur art

**[verified]** `Catz 4 Rez.dll` holds **493 `BDT` entries** totalling 11.2 MB
(`CAT0`, `CAT1`, ...), plus one 67 KB `BHD`. Each `BDT` entry begins:

```
offset 0 : uint32  total size of the entry (matches the file length exactly)
offset 4 : uint16  version (15)
offset 6 : "PFM_[V4.015] (c) 1999 The Learning Company Properties, Inc.
            (Hi Mom, indeed!)"
then     : per-frame ball data
```

**[verified]** Each breed carries its own art: one `BMP` texture plus two `LNZ`
resources. The texture is small - `POUNCER` in `B+W Shorthair.cat` is 105x89
8bpp, the same dimensions as `COTTON` in `Bunny.cat`.

So the fur is a single ~105x89 indexed image per breed, mapped across the body.
That is consistent with the on-screen measurement (smooth shade field plus
per-pixel index detail) and it bounds the problem: the texture is small enough
to be examined exhaustively.

**[verified] Texture indices do not survive to the screen.** A breed texture is
8bpp with ~225 distinct indices spanning 0-255 (index 253 dominant and
transparent). The same breed rendered on screen shows **19** distinct indices in
2..247, dominated by 14..19. Direct pixel correlation between texture and screen
peaks at r=+0.22 over all cat breeds, i.e. no relationship.

So the texture supplies *structure* and the colour is recomputed onto a ramp.
That kills the naive "blit the fur texture" model and matches the exported fur
API: `XFurInfo::SetRampBaseIndex`, `SetFurColor`, `ChangeFurColorArea`,
`SetFurPatternAddball`.

**[inferred]** The `BDT` payload is per-frame ball geometry - positions, sizes
and deformations - while colour comes from the breed `BMP` plus `LNZ` fur
info. If so, reproducing a ball needs three things: the span table (known), the
per-frame geometry (BDT, not yet decoded), and the texture mapping (unknown).

## Oracle harness: what it took to get a stable frame
**[verified]** These are hard-won operating facts. Each one cost a failed run.

1. **Private display.** The game idles its animation when it loses activation
   (0.7% CPU, blocked in the message loop, frames byte-identical). Sharing a
   desktop with a human means every alt-tab freezes the capture subject.
   `tools/xvfb.sh` runs an Xvfb display with no window manager and no competing
   focus.
2. **`Decorated=N`.** Without it the fullscreen surface is cropped by 8x34 px.
3. **Four startup dialogs, in order:** `Voice Recognition Error`,
   `Sound Error` (only once audio is disabled), `Welcome to Petz 4`,
   `Dogz 4 and Catz 4 are Great Gifts!`. All are `_NET_WM_WINDOW_TYPE_DIALOG`.
   Dismiss by setting X input focus then sending Return via XTEST — XSendEvent
   to the window is ignored by Wine's dialog loop.
4. **Audio must be off** for unattended runs: `HKCU\Software\Wine\Drivers` →
   `Audio` = `""`. Otherwise the game plays through the host's speakers during a
   headless capture (yes, really), and DirectSound timing adds nondeterminism.
5. **~60 s of intro** before the scene renders. Readiness must be polled on
   actual content (non-black fraction / colour count), not on elapsed time.

Working sequence:

```sh
./tools/oracle-run.sh build/frames/run-X 6 250 :97 640x480x24
```

### In-process execution: the important capability

**[verified]** We can run our own code inside the retail process. `Petz 4.exe`
imports `UVocab.dll` by name (`VtGeneratePronunciations`) — one symbol — so a
proxy DLL can sit in its place, forward that export to the renamed original,
and execute during `DllMain`:

```
proxy attach pid=32 self=78630000 exe=00400000
```

The exe loads at its preferred base `0x400000`, and its IAT is already resolved
by the time our `DllMain` runs, so import patching is available at load time.
Build with `i686-w64-mingw32-gcc-win32` (bootstrapped into `build/tools/mingw`
by unpacking .debs; no root, no system install).

Why this matters: it removes the need to replay the simulation at all. To prove
the renderer, load the game, let it initialise its own CRT and globals, then
call `XFillRotatedCircle` / `SetBallTextureInfo` with a synthetic `BallState`
and diff the resulting DIB against our reimplementation. Same idea for pinning
the clock and hooking the present.

Loading `Petz 4.exe` as a module from a *separate* harness process does **not**
work: `LoadLibraryA` succeeds and exports resolve correctly
(`?InitMath@@YAXXZ` → base + `0x6bab0`, matching the export table), but calling
in page-faults because the CRT initialisers never ran. The DLL proxy is the
working route.

### Still not deterministic:

**[verified]** Two cold runs do not produce the same frame. Run A captured a
continuously animating scene (3k-9k pixels changing per 250 ms); run B sat on a
near-static screen and diverged from A by 2.56% of pixels. The animation is
time-based, and wall-clock offsets from an unpinned start do not line up.

Until that is fixed, captured corpora are good for *studying* the renderer and
for single-frame work, but not yet for passing "identical" verdicts.

## Open questions

- [ ] Pin the simulation timeline: a shim over `timeGetTime` /
      `QueryPerformanceCounter` so two runs advance identically, or an input
      script that drives the game to a known screen and a known frame boundary.
- [ ] Does the sim run on a fixed timestep or wall clock? Determines whether a
      clock shim is sufficient or frame stepping is needed.
- [ ] Capture the DIB from inside the process (GDI `SetDIBitsToDevice` hook)
      rather than the X surface, to remove compositing from the measurement.
- [ ] `XFurInfo` ramp/pattern maths - the actual fur look.
- [ ] Whether breed DLLs can be loaded for *resource extraction only* without
      running their code (they call into the exe, so no - must parse .rsrc).
