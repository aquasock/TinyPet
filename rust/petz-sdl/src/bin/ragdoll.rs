//! ragdoll - an interactive sandbox for the sphere-spring creature.
//!
//! Three layers meet here, and the split between them is the design:
//!
//!   solver   `petz-core::ragdoll` - springs, gravity, grab. Always running.
//!   gait     `petz-core::gait`    - where the feet want to be, and the IK that
//!                                   gets them there.
//!   look     `petz-core::shade` / `fur` - light and material per pixel.
//!
//! The body does not walk by physics. Feet are kinematic - planted where the
//! gait says - and the spine and knees are *driven* toward a pose with springs
//! that are deliberately not stiff, so the solver still supplies the wobble.
//! The tail and ears have no target at all, so they swing freely.
//!
//! Grab a ball and the whole drive is cancelled: the animal goes limp in your
//! hand and the ragdoll takes over. Let go and it picks the walk back up.
//!
//! Keys: drag = grab, W = walk/stop, + / - = speed, arrows = move light,
//! A/S = ambience, [ ] = ramp steps, T = texture, P = print settings,
//! R = reset, Space = pause, Esc = quit.

use std::collections::HashMap;

use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::{Point, Rect};
use std::time::Instant;

use petz_core::fur::{dapple, stripes, Pattern, Texture};
use petz_core::gait::{two_bone_ik, Gait, GaitSpec};
use petz_core::ragdoll::{Particle, Ragdoll, Vec2};
use petz_core::project::{Body, Projected, Vec3 as WVec3, View};
use petz_core::shade::{Light, Ramp, Vec3};
use petz_core::{draw_disc, draw_disc_on_sphere, draw_flat_disc, draw_ring, DepthBuffer, Paint, Palette, SpanTable, Surface};
use std::fs;
use std::path::PathBuf;

const W: usize = 320;
const H: usize = 240;
/// How much bigger than authored the animal is drawn. The skeleton is in
/// pixels and stays there, so zooming is a projection-time multiply on its
/// local offsets and radii - the shape is untouched, only its scale.
///
/// Default is a half: at 320x240 the animal at authored size filled half the
/// window's width, which left no room to walk around in. `PETZ_ZOOM` sets it
/// per run.
const DEFAULT_ZOOM: f32 = 0.5;

/// The horizon, in screen pixels. `GROUND` is the skeleton's own floor in body
/// units and does not move with the surface; this is where that floor lands on
/// screen, and it halves with `H` so the animal keeps its footing.
const VIEW_GROUND: f32 = GROUND * 0.5;
const BACKGROUND: u8 = 130;

const OUTLINE: u8 = 89;
/// Diagnostic colours, in place of the real ones when `PETZ_DIAG` is set: the
/// near ear in pure blue and every rim in pure red, so which ball covers which
/// can be read off a screenshot instead of argued about.
const DIAG_EAR: u8 = 252;
const DIAG_OUTLINE: u8 = 249;
// Diagnostic palette, asked for by eye: black fur everywhere, a pink nose,
// white feet. `BODY` is what "fur" resolves to, so the coat, the head, the legs
// and the tail all follow it.
const BODY: u8 = 0;
const LIGHT: u8 = 10;
const DARK: u8 = 0;
/// The nose, for this diagnostic set: a clear pink against the black coat.
const PINK: u8 = 70;
/// Ball 5, the muzzle: a bright blue with enough of a family in the palette to
/// shade. Pure blue (252) has two entries and comes out flat, like the sclera
/// white did.
const MUZZLE_BLUE: u8 = 240;
/// Retail `;L eye` colour: white sclera.
const SCLERA: u8 = 245;
/// Retail `;L iris` colour: black pupil, with the iris ring drawn as its
/// outline colour. Teal is the Dalmatian's; any breed colour works.
const IRIS: u8 = 244;
const IRIS_RING: u8 = 6;

/// One eye per side, at this lateral offset.
const EYE_SIDES: [f32; 2] = [-12.0, 12.0];
/// The eyelid shell. Black for this diagnostic set, like the rest of the fur.
const LID: u8 = 0;

// The ear balls, frozen at the placement that was tuned by eye. The base hangs
// off the skull's centre; the tip hangs off its base and is pulled into it by
// `EAR_TIP_SINK`. Sizes are radii. There are no keys for these any more - an
// adjustment is an edit here.
const EAR_BACK: f32 = -2.0;
const EAR_UP: f32 = 28.0;
const EAR_SEP: f32 = 14.0;
const EAR_SIZE: f32 = 9.5;
const EAR_TIP_BACK: f32 = 0.0;
const EAR_TIP_UP: f32 = 12.0;
const EAR_TIP_SIZE: f32 = 6.5;
const EAR_TIP_SINK: f32 = 0.20;

/// How hard each tail link argues for its length, per solver pass. A hard link
/// is 1.0; the tail is deliberately looser so the chain can stretch and whip.
const TAIL_LINK: f32 = 0.12;

/// The mouth: a thin line painted onto the muzzle's own sphere, the way the
/// pupil is painted onto the sclera - a texture, not a ball.
///
/// `MOUTH_WIDTH` is its length and `MOUTH_THICK` its thickness, both in body
/// units; at the default zoom of 0.5, 14 and 2 come out as a 7 px line one pixel
/// thick. `MOUTH_DROP` is how far below the sphere's centre it sits, as a
/// fraction of the muzzle's radius, and it is at the snout's limit: at 0.85 the
/// line's lower edge is on the muzzle's own rim, and further down the rim is what
/// is clipping it, so the pixels stop changing.
const MOUTH_WIDTH: f32 = 14.0;
const MOUTH_THICK: f32 = 2.0;
const MOUTH_DROP: f32 = 0.85;

/// The mouth's colour: black on this palette, the same entry the retail iris
/// uses for a pupil, and green under `PETZ_DIAG` so it can be found in a dump
/// instead of disappearing into a black coat.
const MOUTH: u8 = 244;
const DIAG_MOUTH: u8 = 250;

/// How far in front of the muzzle's own surface the mouth is drawn, in world
/// units. Small: enough to beat the snout it is painted on, not enough to win
/// against anything genuinely in front of it.
const MOUTH_BIAS: f32 = -0.9;

/// The claw ball's radius. Small enough that four of them across a foot's toe
/// are separate dots at 320x240 rather than one blob.
const CLAW_RADIUS: f32 = 2.0;

/// Where the four claws sit on a foot, forward then lateral from its centre.
///
/// On the arc of radius `foot + claw` about the foot's centre, so each claw is
/// tangent to the foot rather than buried in it, and spread across the toe the
/// way toes are spread. The outer pair sits further back than the inner pair
/// for that reason: a paw's toes wrap round the front of it.
const TOES: [(f32, f32); 4] = [
    (7.9, -9.0),
    (11.6, -3.0),
    (11.6, 3.0),
    (7.9, 9.0),
];

/// How far the front ear is pushed clear of the back one, in depth units.
///
/// Two ears a couple of units apart interleave pixel by pixel, because a single
/// sphere spans its own radius in depth: the back ear's body punches holes
/// through the front ear's, which reads as slivers between texture and outline.
/// Eleven is more than a radius, so the front ear wins everywhere inside its own
/// silhouette, and it is small enough that the ear still passes behind the
/// skull's surface where it sinks into the head.
const EAR_FRONT: f32 = 11.0;

/// Where the floor is. Feet stand on this line.
const GROUND: f32 = 218.0;

/// Centre of the body in the skeleton's own forward coordinates. The bones are
/// authored at absolute x, so this is subtracted before placing them in the
/// world - otherwise the animal stands a body-length behind its own position.
const BODY_ORIGIN: f32 = 366.0;

/// A 3x5 digit font, hand-written. The angle readout is the only text the
/// sandbox needs, and it is not worth a font dependency for that.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// Draw a non-negative integer in window coordinates.
fn draw_number(
    canvas: &mut sdl2::render::Canvas<sdl2::video::Window>,
    x: i32,
    y: i32,
    scale: i32,
    value: i32,
) {
    let text = value.max(0).to_string();
    let width = text.len() as i32 * 4 * scale;
    let _ = canvas.set_draw_color(Color::RGB(0, 0, 0));
    let _ = canvas.fill_rect(Rect::new(x - 5, y - 5, (width + 9) as u32, (5 * scale + 9) as u32));
    let _ = canvas.set_draw_color(Color::RGB(255, 255, 255));
    for (i, ch) in text.chars().enumerate() {
        let d = ch.to_digit(10).unwrap_or(0) as usize;
        for (row, bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3 {
                if bits & (1 << (2 - col)) != 0 {
                    let _ = canvas.fill_rect(Rect::new(
                        x + i as i32 * 4 * scale + col * scale,
                        y + row as i32 * scale,
                        scale as u32,
                        scale as u32,
                    ));
                }
            }
        }
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_table() -> SpanTable {
    SpanTable::new(
        fs::read(repo_root().join("evidence/ball/tables.raw"))
            .expect("evidence/ball/tables.raw"),
    )
}

fn load_palette() -> Palette {
    let raw = fs::read(repo_root().join("evidence/ball/ball-input.raw"))
        .expect("evidence/ball/ball-input.raw");
    let stride = u32::from_le_bytes(raw[12..16].try_into().unwrap()) as usize;
    let height = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let header = raw.len() - (1024 + 512 + 2 * stride * height);
    Palette::from_rgbquad(&raw[header..header + 1024])
}

/// One leg, by particle index.
struct Leg {
    hip: usize,
    /// The spine ball this leg hangs from, and the hip's offset from it. The hip
    /// is pinned to *that ball*, not to a fixed point in body space: it has to
    /// move with the body's own sway, or it sits still while the spine swings
    /// and the leg looks torn off.
    attach: usize,
    offset: Vec2,
    /// The hip's lateral offset from that spine ball. Kept separately because
    /// the side axis is not in `offset`.
    side_offset: f32,
    knee: usize,
    foot: usize,
    /// Four tiny balls across the front of the foot: the claws, side by side
    /// like toes. Each entry is the ball and its offset from the foot's centre,
    /// forward then lateral. Placed by the pose, not solved, so they cannot lag
    /// behind the foot they belong to.
    claws: [(usize, f32, f32); 4],
    /// Which way the knee juts out from the hip-foot line. A dog's front legs
    /// bend backward and its hind legs forward.
    bend: f32,
    /// Rest position of the foot, so a stopped animal stands still.
    rest: Vec2,
}

/// The skeleton, described by indices into the ragdoll.
struct Creature {
    spine: Vec<usize>,
    legs: Vec<Leg>,
    head: Vec<usize>,
    tail: Vec<usize>,
    /// (base, overlay) pairs drawn as one unit: the iris sits on its sclera.
    eye_overlays: Vec<(usize, usize)>,
    /// The skull, used as the reference the eyes are tested against.
    skull: usize,
    /// The muzzle: the sphere the mouth is painted onto.
    muzzle: usize,
    /// The sclera of each eye, for the visibility test.
    eyes: Vec<usize>,
    /// The ear balls: (base, tip, side sign).
    ears: Vec<(usize, usize, f32)>,
    /// (sclera, iris, rest height, side sign) per eye, so the placement can be
    /// tuned while running instead of being baked in.
    eye_parts: Vec<(usize, usize, f32, f32)>,
    /// Every ball that can occlude an eye. The eye is clipped against all of
    /// them, not just the cranium.
    head_balls: Vec<usize>,
}

fn build() -> (Ragdoll, Creature) {
    let diag = std::env::var("PETZ_DIAG").is_ok();
    let mut r = Ragdoll::new();
    r.bounds = Some((-4000.0, 8.0, 4000.0, 460.0));
    r.gravity = 1400.0;
    r.damping = 0.985;
    r.iterations = 16;
    r.contact = 0.35;

    // Everything below is in *body* coordinates: x is forward, y is down from
    // the body's own frame (so `GROUND` is the floor), and `side` is lateral.
    //
    // Four balls, not five: hips, two along the back, and the neck at the
    // front. The neck is a spine ball rather than a separate link, because an
    // extra ball there put the head a body-length away from the shoulders and
    // the head read as detached.
    let spacing = 28.0;
    // The last of these is the neck; 20% down from where it was.
    let spine_r = [24.0, 23.0, 22.0, 16.0];
    let mut spine = Vec::new();
    for i in 0..spine_r.len() {
        let fwd = 276.0 + i as f32 * spacing;
        // Balls 0 and 2 are white, so the numbering can be read off the back -
        // the coat's own white (LIGHT), which shades, not the sclera's flat one.
        let colour = if i == 0 || i == 2 { LIGHT } else { BODY };
        spine.push(r.add(Particle::new(fwd, GROUND - 68.0, spine_r[i], colour)));
    }
    // Lift the neck onto the head's own axis. The skull runs from (386, base-26)
    // to the muzzle at (408, base-16); continuing that line backwards by one
    // head length puts the neck at (364, base-16). So the neck leaves the body
    // at a slight angle and the neck-to-skull run is parallel to skull-to-muzzle,
    // instead of the spine lying flat and the head hopping up off it.
    r.particles[spine[3]].pos = Vec2::new(364.0, GROUND - 84.0);

    for i in 0..spine.len() - 1 {
        r.link(spine[i], spine[i + 1]);
    }
    // No diagonals along the spine: measured against a bare chain they bought
    // 2.4 degrees of fold, because the balls' own contacts do most of the work
    // of keeping the back straight. One wire between neighbours is the whole
    // skeleton, which is also what the overlay should show.

    // Head, hanging directly off the neck ball: a single skull, with a muzzle
    // and nose on the front of it. The cluster of seven was an experiment in
    // making the eye clip against a socket; the shell model made it unnecessary.
    let base_y = GROUND - 68.0;
    let neck = *spine.last().unwrap();
    let skull = r.add(Particle::new(386.0, base_y - 26.0, 26.0, BODY));
    // Out along the axis by 2 px, about 10% of its offset from the skull.
    let muzzle = r.add(Particle::new(408.0, base_y - 16.0, 13.0, MUZZLE_BLUE));
    // Out on the tip, clear of the muzzle's own sphere. Closer in it counts as
    // *inside* the muzzle and the containment rule hides it, which is what
    // swallowed the black tip.
    // The nose rides on the muzzle, so it moves with it.
    let nose = r.add(Particle::new(422.0, base_y - 20.0, 5.0, PINK));
    r.particles[nose].no_rim = true;
    r.link(neck, skull);
    r.link(skull, muzzle);
    r.link(muzzle, nose);
    // One brace the chain does not provide: the skull to the spine ball two
    // back. Without it the head nods up to 10 degrees while walking; with it,
    // 6.3. The spine's own diagonals were removed for the opposite reason.
    r.link(skull, spine[spine.len() - 2]);

    let head_balls = vec![skull, muzzle, nose];

    // Ears: a base and a tip each, an ordinary body segment in everything but
    // placement. Two balls, because the retail ear is six and buys a shape we
    // do not need at this fidelity. Both are pinned and placed by the pose, and
    // both take the head's depth in the render loop, so the head's outline does
    // not run across them. The tip is the lighter ball.
    let mut ear_balls: Vec<(usize, usize, f32)> = Vec::new();
    for side in [-1.0f32, 1.0] {
        // The base takes the snout's blue; PETZ_DIAG paints the second ear
        // (balls 9 and 10) in pure blue, to be read against the red outline
        // main() switches in.
        let (base_col, tip_col) = if diag && side > 0.0 {
            (DIAG_EAR, DIAG_EAR)
        } else {
            // The base carries the snout's blue, with the same ramp behind it.
            (MUZZLE_BLUE, LIGHT)
        };
        let base = r.add(Particle::new(378.0, base_y - 58.0, 10.0, base_col));
        let tip = r.add(Particle::new(368.0, base_y - 72.0, 7.0, tip_col));
        // Pinned: the pair is placed as a billboard every frame, so a spring
        // has nothing to do and would only pull the skull about. Pinned plus
        // linked is also what makes a pulled ear drag the head: the pinned end
        // takes no correction, so the spring moves the skull instead.
        for i in [base, tip] {
            r.particles[i].pinned = true;
            r.particles[i].side = 10.0 * side;
            r.particles[i].rest_side = 10.0 * side;
        }
        // The ears are *part of the skeleton*, not decorations floating beside
        // it: linked to the skull and to each other, so they show up in the
        // constraint overlay and drag the head when pulled, like every other
        // ball. The pose keeps each link's rest length at the distance it
        // actually places them, so the springs never fight the placement.
        r.link(skull, base);
        r.link(base, tip);
        ear_balls.push((base, tip, side));
    }

    // Pull the neck and head back into the body by a fifth. The compression is
    // towards the shoulder ball, so the head keeps its proportions instead of
    // simply sliding sideways through the chest.
    const SHOULDER: f32 = 332.0;
    let mut pulled: Vec<usize> = vec![spine[3], skull, muzzle, nose];
    pulled.extend(ear_balls.iter().map(|&(b, _, _)| b));
    pulled.extend(ear_balls.iter().map(|&(_, t, _)| t));
    for i in pulled {
        let p = &mut r.particles[i];
        p.pos.x = SHOULDER + 0.8 * (p.pos.x - SHOULDER);
    }
    // Every spring was built against the old layout, so re-solve them from the
    // arrangement that actually exists now.
    let pairs: Vec<(usize, usize)> = r.constraints.iter().map(|c| (c.a, c.b)).collect();
    for (a, b) in pairs {
        let rest = r.particles[a].pos.sub(r.particles[b].pos).length();
        r.set_rest(a, b, rest);
    }

    // Eyes. The retail data is two balls per eye: a white sclera and a black
    // pupil whose *outline* is the iris, in a per-breed colour. There is no
    // eyelid ball at all - Petz parameterises the lid instead.
    //
    let mut eyes = Vec::new();
    let mut overlay_pairs: Vec<(usize, usize)> = Vec::new();
    let mut eye_list: Vec<usize> = Vec::new();
    let mut eye_parts: Vec<(usize, usize, f32, f32)> = Vec::new();
    for side in EYE_SIDES {
        let sclera = r.add(Particle::new(394.0, base_y - 34.0, 10.0, SCLERA));
        let iris = r.add(Particle::new(394.0, base_y - 33.0, 6.0, IRIS));
        for i in [sclera, iris] {
            r.particles[i].side = side;
            r.particles[i].rest_side = side;
        }
        r.particles[iris].rim = IRIS_RING;
        r.particles[iris].layer = 1;
        // The eyes are placed exactly, not settled. Left to the solver they
        // drifted apart by five pixels and one sat higher than the other.
        r.particles[sclera].pinned = true;
        r.particles[iris].pinned = true;

        r.link(skull, sclera);
        // The pupil and the lid are children of the eye, not of the skull.
        // Unlinked they were free particles: any grab cleared their drive and
        // they simply fell out of the head.
        r.link(sclera, iris);

        eyes.extend([sclera, iris]);
        overlay_pairs.push((sclera, iris));
        eye_list.push(sclera);
        eye_parts.push((sclera, iris, base_y - 34.0, side.signum()));
    }

    // Legs. The front pair hangs off the ball *behind* the neck - the shoulder -
    // and the hind pair off the hips.
    let legs_spec: [(f32, f32, f32); 4] = [
        (332.0, 9.0, 1.0),   // left front
        (332.0, -9.0, 1.0),  // right front
        (276.0, 9.0, -1.0),  // left hind
        (276.0, -9.0, -1.0), // right hind
    ];
    let mut legs = Vec::new();
    for (fwd, side, bend) in legs_spec {
        // PETZ_DIAG paints the hips, which are otherwise coat colour and vanish
        // into the body while you are trying to watch them turn.
        let hip_col = if std::env::var("PETZ_DIAG").is_ok() { 143 } else { BODY };
        let hip = r.add(Particle::new(fwd, GROUND - 48.0, 15.0, hip_col));
        let knee = r.add(Particle::new(fwd, GROUND - 22.0, 12.0, BODY));
        let foot = r.add(Particle::new(fwd, GROUND, 10.0, LIGHT));
        // The claws: black against the white foot, small enough that an outline
        // would be most of it, so they have none. Each sits on the arc that keeps
        // it clear of the foot's own sphere - any closer and the foot's surface
        // hides it - and the four are spread laterally, across the toe, the way
        // toes are.
        let mut claws = [(0usize, 0.0f32, 0.0f32); 4];
        for (n, &(ahead, across)) in TOES.iter().enumerate() {
            let claw = r.add(Particle::new(fwd + ahead, GROUND - 1.0, CLAW_RADIUS, DARK));
            r.particles[claw].no_rim = true;
            r.particles[claw].side = side + across;
            r.particles[claw].rest_side = side + across;
            claws[n] = (claw, ahead, across);
        }
        // The hip is part of the body, not a thing hanging off it. It is pinned
        // and, at projection time, derived from the spine ball it hangs from -
        // see the projection loop. The sway belongs to the leg below it.
        r.particles[hip].pinned = true;
        for i in [hip, knee, foot] {
            r.particles[i].side = side;
            r.particles[i].rest_side = side;
        }
        let attach = if fwd > 300.0 { spine[spine.len() - 2] } else { spine[0] };
        r.link(attach, hip);
        r.link(hip, knee);
        r.link(knee, foot);
        for &(claw, _, _) in &claws {
            r.link(foot, claw);
        }
        let offset = r.particles[hip].pos.sub(r.particles[attach].pos);
        let side_offset = r.particles[hip].side - r.particles[attach].side;
        legs.push(Leg {
            hip,
            attach,
            offset,
            side_offset,
            knee,
            foot,
            claws,
            bend,
            rest: Vec2::new(fwd, GROUND),
        });
    }

    // Tail: no target, so it swings with the body.
    //
    // Authored on the spine's own line; the pose lifts the root above it, the
    // way a cat's tail leaves the body above the anus. Everything past the root
    // is linked and left to gravity, which is what makes it dangle.
    let mut tail = Vec::new();
    // The tail keeps far more of its velocity than the body does. Damped like
    // the body it swings out and snaps back, which reads as stiff; given a
    // lighter hand it keeps moving, and that is most of what makes a tail look
    // alive.
    let tail_drag = 0.998 / 0.985;
    let mut tail_prev = spine[0];
    for i in 0..5 {
        // Alternating: 43 black, 44 white, 45 black, 46 white, 47 black.
        let tail_col = if i % 2 == 0 { BODY } else { LIGHT };
        let t = r.add(Particle::new(
            252.0 - i as f32 * 15.0,
            base_y,
            10.0 - i as f32 * 1.2,
            tail_col,
        ));
        r.particles[t].drag = tail_drag;
        // Soft links, unlike the spine's: a chain of hard links under gravity
        // hangs dead straight and reads as a rod. Letting each segment stretch a
        // little is what gives the tail its whip - it trails behind a turn and
        // curls instead of pivoting as one piece.
        let rest = r.particles[tail_prev].pos.sub(r.particles[t].pos).length();
        r.constraints
            .push(petz_core::Constraint::soft(tail_prev, t, rest, TAIL_LINK));
        if i == 0 {
            // The root is part of the body, not a thing parked behind it. Held
            // by a drive it lagged the rump whenever the rump moved, and the
            // joint opened; pinned, the pose places it on the butt ball every
            // frame and nothing can pull it off. Same treatment as a hip.
            r.particles[t].pinned = true;
        }
        tail_prev = t;
        tail.push(t);
    }

    let mut head = vec![skull, muzzle, nose];
    head.extend(ear_balls.iter().map(|&(b, _, _)| b));
    head.extend(ear_balls.iter().map(|&(_, t, _)| t));
    head.extend(eyes);
    (r, Creature { spine, legs, head, tail, skull, muzzle, eye_overlays: overlay_pairs, eyes: eye_list, ears: ear_balls, eye_parts, head_balls })
}

/// The walking controller: owns the clock, the world position, and the pose.
struct Walker {
    gait: Gait,
    /// Where the animal is in the world and which way it faces. `heading` is
    /// what the player asked for; `heading_body` is where the body has actually
    /// got to, because a real animal cannot pivot instantly.
    body: Body,
    heading_body: f32,
    /// Per-ball heading, lagging the body by a part-specific time constant.
    /// This is what makes a turn travel down the animal instead of teleporting
    /// the whole skeleton to a new orientation.
    facing: Vec<f32>,
    lag: Vec<f32>,
    /// Where the eyes sit on the head: an angle round the skull from straight
    /// ahead, and a radius from the skull's centre.
    ///
    /// Parameterising by angle rather than by a lateral offset matters. Two
    /// parallel lines pushed sideways eventually pass *through* the head; a
    /// point at a fixed radius stays on its surface, so the eye is always
    /// tangent to the head and can never sink into it.
    eye_angle: f32,
    eye_radius: f32,
    /// Depth bias in the eye's favour, in world units. The occlusion test is
    /// "is the eye's surface nearer than the head's", and this moves the
    /// threshold: higher keeps the eye visible a little longer, at the cost of
    /// it occasionally winning against the head where it should not.
    eye_bias: f32,
    /// Outer shell: how far the eyelid has dropped across the eye, 0 open and
    /// 1 closed, and the tilt of the line that divides lid from open eye.
    lid_drop: f32,
    lid_tilt: f32,
    /// How far above the skull's centre the eyes sit, in body units.
    eye_lift: f32,
    /// Last movement input, so the pose knows whether to animate a gait.
    moving: f32,
    /// The mouth line's length and thickness in body units, and how far below
    /// the muzzle's centre it sits as a fraction of that radius. The length is
    /// live while it is being judged; the thickness is not, because at this
    /// zoom the only choices are one pixel and two.
    mouth_width: f32,
    mouth_thick: f32,
    mouth_drop: f32,
}

/// Fastest the animal's body can turn, in radians per second.
const MAX_TURN_RATE: f32 = 2.4;

/// How fast A/D turn the animal, in radians per second.
const TURN_BUTTON_RATE: f32 = 1.8;

impl Walker {
    fn new(creature: &Creature, particles: usize) -> Self {
        // Half of the old start: the surface is half the size, so the same spot
        // on screen is half the world coordinates. The skeleton itself is
        // untouched, which is what keeps the animal's detail.
        let body = Body { pos: WVec3::new(160.0, -260.0, 0.0), heading: 0.42 };
        // Lag by part. The head leads, the body follows, the tail trails - the
        // same overlapping action an animator would key by hand.
        let mut lag = vec![0.28; particles];
        for &i in &creature.spine {
            lag[i] = 0.34;
        }
        for (n, &i) in creature.spine.iter().enumerate() {
            // Front of the spine answers sooner than the back of it.
            // Tighter than it was (0.42 down to 0.24): the rear of the body
            // swung 30 px on every turn, and the hips welded to it swung with
            // it. The order stays - the head leads, the tail trails - but the
            // whole ladder is shorter, because sway is lag times distance and
            // the rear ball is 90 units out.
            // The ladder the turn travels down. It can be this long again
            // because the hips no longer keep their own heading: each one is
            // derived from the spine ball it hangs from, so the rear can swing
            // without the legs tearing off it.
            lag[i] = 0.42 - 0.06 * n as f32;
        }
        for &i in &creature.head {
            lag[i] = 0.11;
        }
        for leg in &creature.legs {
            // The hip rides on the spine ball it hangs from, so it has to ease
            // at *that ball's* rate - not zero, and not its own. Any difference
            // between the two is the gap that opens while the animal turns: the
            // spine ball swings and the hip does not, and the leg tears away
            // from the body.
            lag[leg.hip] = lag[leg.attach];
            lag[leg.knee] = 0.30;
            lag[leg.foot] = 0.36;
            // The claws are placed in the foot's own frame, so they have to ease
            // at the same rate as it: any difference and the projection swings
            // them out of the paw while the animal turns, then back.
            for &(claw, _, _) in &leg.claws {
                lag[claw] = lag[leg.foot];
            }
        }
        for (n, &i) in creature.tail.iter().enumerate() {
            lag[i] = 0.50 + 0.08 * n as f32;
        }
        Self {
            gait: Gait::walk(GaitSpec::default()),
            body,
            heading_body: body.heading,
            facing: vec![body.heading; particles],
            lag,
            // The placement tuned by eye, expressed on the sphere: 27 degrees
            // round from straight ahead, at radius 31 against a head of 26.
            eye_angle: 25.0_f32.to_radians(),
            eye_radius: 29.0,
            eye_bias: 1.5,
            lid_drop: 0.26,
            lid_tilt: 0.23,
            eye_lift: 12.0,
            moving: 1.0,
            // Headless instruments, like PETZ_ZOOM: the mouth can be swept
            // without a window or a keyboard.
            mouth_width: std::env::var("PETZ_MOUTH")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(MOUTH_WIDTH),
            mouth_thick: std::env::var("PETZ_MOUTH_THICK")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(MOUTH_THICK),
            mouth_drop: std::env::var("PETZ_MOUTH_DROP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(MOUTH_DROP),
        }
    }

    /// Ease every ball's facing toward where the body has got to.
    fn turn(&mut self, dt: f32) {
        let delta = wrap_pi(self.body.heading - self.heading_body);
        let max = MAX_TURN_RATE * dt;
        self.heading_body = wrap_pi(self.heading_body + delta.clamp(-max, max));

        for i in 0..self.facing.len() {
            let want = wrap_pi(self.heading_body - self.facing[i]);
            let lag = self.lag.get(i).copied().unwrap_or(0.25).max(1e-3);
            let k = (dt / lag).min(1.0);
            self.facing[i] = wrap_pi(self.facing[i] + want * k);
        }
    }

    /// The body transform for one ball, using that ball's own facing.
    fn part(&self, i: usize) -> Body {
        Body { pos: self.body.pos, heading: self.facing.get(i).copied().unwrap_or(self.heading_body) }
    }

    /// `moving` is +1 forward, -1 backward, 0 standing. `turning` is -1 left,
    /// +1 right, 0 straight.
    fn advance(&mut self, dt: f32, moving: f32, turning: f32) {
        // Turning happens whether or not the animal is moving - a dog can look
        // round while standing.
        self.body.heading += turning * TURN_BUTTON_RATE * dt;
        self.moving = moving;
        self.turn(dt);

        if moving == 0.0 {
            return;
        }
        let f = Body { pos: self.body.pos, heading: self.heading_body }.forward();
        let nx = self.body.pos.x + f.x * self.gait.spec.speed * moving * dt;
        let ny = self.body.pos.y + f.y * self.gait.spec.speed * moving * dt;
        // Stay on the visible ground by stopping, not by turning. Turning is the
        // player's job: an animal that turns itself round makes any angle you
        // are trying to look at impossible to hold.
        // Bounds of the visible patch of ground at the default view pitch.
        let cx = nx.clamp(35.0, 280.0);
        let cy = ny.clamp(-350.0, 150.0);
        let blocked = (cx - nx).abs() > 1e-3 || (cy - ny).abs() > 1e-3;
        self.body.pos.x = cx;
        self.body.pos.y = cy;
        if !blocked {
            // Running the gait backwards makes the feet plant correctly when
            // the animal reverses, instead of skating.
            self.gait.advance(dt * moving.signum());
        }
    }

    /// Drive the skeleton toward the pose the gait describes.
    fn pose(&self, doll: &mut Ragdoll, creature: &Creature, rest: &Pose) {
        let walking = self.moving != 0.0;
        // A small vertical bob, twice per cycle: the body rises as the legs
        // pass under it.
        let bob = (self.gait.phase * std::f32::consts::TAU * 2.0).sin() * 2.0;
        let lift = if walking { bob } else { 0.0 };

        let n = creature.spine.len().max(1);
        for (i, &p) in creature.spine.iter().enumerate() {
            let t = i as f32 / (n - 1).max(1) as f32;
            let (dv, dl) = if walking { self.gait.spine_flex(t) } else { (0.0, 0.0) };
            // dv is upward, and body y runs down, so it subtracts.
            doll.drive_to(p, Vec2::new(rest.spine[i].x, rest.spine[i].y + lift - dv), 0.45);
            // Lateral flex is authored rather than solved: the spine's sway is
            // a curve through `side`, which the solver does not model.
            if let Some(part) = doll.particles.get_mut(p) {
                part.side = dl;
            }
        }
        for (i, &p) in creature.head.iter().enumerate() {
            doll.drive_to(p, Vec2::new(rest.head[i].x, rest.head[i].y + lift), 0.4);
        }
        let skull_now = doll.particles[creature.skull].pos;
        let skull_rest = rest.head[0];
        // Ears separated along the body's lateral axis, which is what puts them
        // on one vertical axis when the animal is side-on: at 90 degrees the
        // lateral offset is entirely depth, so the two share a screen x and
        // differ only in height.
        {
            let sk = doll.particles[creature.skull].pos;
            let base_at = Vec2::new(sk.x - EAR_BACK, sk.y - EAR_UP);
            // The tip hangs back and up of its base, shortened by the sink so
            // the two balls blend instead of always reading as a cap stuck on
            // a lump.
            let apart = 1.0 - EAR_TIP_SINK;
            let tip_at = Vec2::new(
                base_at.x - EAR_TIP_BACK * apart,
                base_at.y - EAR_TIP_UP * apart,
            );
            for &(base, tip, s) in &creature.ears {
                // Placed exactly, not driven. Driven, the links below would
                // fight them and they lagged their own placement by 4 units.
                // Placed, the pair never drifts apart - and the cost is that a
                // released ear returns in one frame instead of easing.
                doll.drag_to(base, base_at);
                doll.drag_to(tip, tip_at);
                for i in [base, tip] {
                    if let Some(p) = doll.particles.get_mut(i) {
                        p.side = s * EAR_SEP;
                    }
                }
                if let Some(p) = doll.particles.get_mut(base) {
                    p.radius = EAR_SIZE;
                }
                if let Some(p) = doll.particles.get_mut(tip) {
                    p.radius = EAR_TIP_SIZE;
                }
                // The links exist so the ears are part of the skeleton. While
                // the pose is driving them they must not argue with it, so their
                // rest lengths track the distance the pose wants; when a grab
                // suspends the pose, the links are what pull the head along.
                // The *current* distance, not the target's: a spring acting on
                // positions that have not converged yet pulls the ear off its
                // target if it is told the target's length instead.
                let now = doll.particles[base].pos.sub(doll.particles[creature.skull].pos).length();
                doll.set_rest(creature.skull, base, now);
                let now = doll.particles[tip].pos.sub(doll.particles[base].pos).length();
                doll.set_rest(base, tip, now);
            }
        }

        for &(sclera, iris, y, sign) in &creature.eye_parts {
            // On the head's surface at the chosen angle, so widening the eyes
            // walks them round the skull instead of through it.
            let target = Vec2::new(
                skull_now.x + self.eye_radius * self.eye_angle.cos(),
                skull_now.y - self.eye_lift,
            );
            let _ = (y, skull_rest);
            for i in [sclera, iris] {
                // Pinned, so this is exact: the solver cannot pull the two eyes
                // out of symmetry.
                doll.drag_to(i, target);
                if let Some(p) = doll.particles.get_mut(i) {
                    p.side = sign * self.eye_radius * self.eye_angle.sin();
                }
            }
            // Let the link follow the eye. Keeping the length it was built at
            // meant the skeleton pulled the eye back towards the authored
            // position, so it could never be moved far out of the head.
            let sk = doll.particles[creature.skull].pos;
            let want = doll.particles[sclera].pos.sub(sk).length();
            doll.set_rest(creature.skull, sclera, want);
        }

        // Only the root of the tail is driven. Left entirely free the whole
        // tail hangs straight down under the body, because that is what a chain
        // does; one weak drive on the first ball holds the tail out behind, and
        // the rest hang off it and swing.
        // Only the root of the tail is held: it sits high on the rump, above
        // the line of the spine, the way a cat's tail leaves the body. Every
        // ball past it is a free link - no target at all - so the tail dangles
        // like a chain and can swing right round the root. Left unanimated that
        // looks like a broken tail; what it needs is motion, not a shape.
        const ROOT_LIFT: f32 = 13.0;
        if let Some(&root) = creature.tail.first() {
            // The root grows out of the rump, so it is placed on the ball it
            // grows out of: the offset it holds in the rest pose, carried on the
            // butt ball's live position. Derived, not driven - a driven root lags
            // whatever moves the rump and the joint opens, which is the same
            // fault the hips had and fixed the same way.
            let home = Vec2::new(252.0, GROUND - 68.0 - ROOT_LIFT);
            let off = home.sub(rest.spine[0]);
            let butt = doll.particles[creature.spine[0]].pos;
            doll.drag_to(root, Vec2::new(butt.x + off.x, butt.y + off.y));
            if let Some(p) = doll.particles.get_mut(root) {
                // A grab-release clears the pin on whatever was grabbed, so the
                // pose asserts it back: the root is part of the body always.
                p.pinned = true;
            }
            // Keep the link neutral while the pose owns the placement: it is
            // what holds the joint when a grab suspends the pose, and it must
            // not argue with the placement the rest of the time.
            doll.set_rest(creature.spine[0], root, off.length());
        }

        // The chain hangs off the rump, so every ball sits on the rump's own
        // lateral plane. The spine's lateral flex is authored, and without this
        // the joint steps sideways by the whole of it - the same opening as a
        // driven root, in the other axis.
        {
            let base_side = doll.particles[creature.spine[0]].side;
            for &ball in creature.tail.iter() {
                if let Some(p) = doll.particles.get_mut(ball) {
                    p.side = base_side;
                }
            }
        }

        for (i, leg) in creature.legs.iter().enumerate() {
            let foot = if walking {
                let f = self.gait.foot(i);
                Vec2::new(leg.rest.x + f.dx, GROUND + f.dy)
            } else {
                Vec2::new(leg.rest.x, GROUND)
            };

            let hip_target = Vec2::new(rest.legs[i].x, rest.legs[i].y + lift);
            // Bone lengths are the rest distances, so IK matches the skeleton.
            let upper = rest.hip_to_knee[i];
            let lower = rest.knee_to_foot[i];
            let knee = two_bone_ik(hip_target, foot, upper, lower, leg.bend);

            // The foot is exact; the hip and knee lag slightly, which is what
            // keeps the legs from looking like rigid rods.
            doll.drive_to(leg.foot, foot, 1.0);
            doll.drive_to(leg.knee, knee, 0.85);
            // Exact, not driven, and anchored on the spine ball it hangs from:
            // a driven hip lags the body through every turn, and a hip fixed in
            // body space sits still while the spine sways - either way the leg
            // reads as coming adrift.
            let held = doll.particles[leg.attach].pos.add(leg.offset);
            doll.drag_to(leg.hip, held);
            doll.set_rest(leg.attach, leg.hip, leg.offset.length());
            // The claws ride on the foot: forward of it and spread across its
            // toe. Placed rather than solved - a tiny ball on a spring lags and
            // jitters behind a foot that is being driven.
            let heel = doll.particles[leg.foot].pos;
            let s = doll.particles[leg.foot].side;
            for &(claw, ahead, across) in &leg.claws {
                doll.drag_to(claw, Vec2::new(heel.x + ahead, heel.y + 1.0));
                if let Some(p) = doll.particles.get_mut(claw) {
                    p.side = s + across;
                }
            }
        }
    }
}

/// Bone lengths measured once from the rest pose.
struct Pose {
    spine: Vec<Vec2>,
    head: Vec<Vec2>,
    legs: Vec<Vec2>,
    hip_to_knee: Vec<f32>,
    knee_to_foot: Vec<f32>,
}

fn measure(doll: &Ragdoll, c: &Creature) -> Pose {
    let spine = c.spine.iter().map(|&i| doll.particles[i].pos).collect();
    let head = c.head.iter().map(|&i| doll.particles[i].pos).collect();
    let legs: Vec<Vec2> = c.legs.iter().map(|l| doll.particles[l.hip].pos).collect();
    let hip_to_knee = c
        .legs
        .iter()
        .map(|l| doll.particles[l.hip].pos.sub(doll.particles[l.knee].pos).length())
        .collect();
    let knee_to_foot = c
        .legs
        .iter()
        .map(|l| doll.particles[l.knee].pos.sub(doll.particles[l.foot].pos).length())
        .collect();
    Pose { spine, head, legs, hip_to_knee, knee_to_foot }
}

/// Where the mouth's centre sits, in screen units from the muzzle's centre, and
/// whether the marking is on the half of that sphere facing the viewer.
///
/// The mouth is a marking on the front of the snout, so its body-space direction
/// is (forward, 0, -`drop`) and its screen offset is that direction projected.
/// The horizontal part is `sin(heading)`, which is what slides the mouth from the
/// middle of the muzzle when the animal faces the camera out to the rim in
/// profile. Dividing by the direction's length keeps the centre on the sphere
/// whatever the drop is.
///
/// The cull is not an optimisation. A disc drawn on a sphere takes that sphere's
/// *near* surface for its depth, so a marking pointing away from the viewer would
/// paint straight through the snout and appear on the wrong side of the head.
fn mouth_offset(heading: f32, radius: f32, drop: f32) -> Option<(f32, f32)> {
    // The tolerance is not cosmetic: exactly side-on, `cos` is a rounding error
    // either side of zero, so a bare `> 0.0` shows the mouth facing one way and
    // hides it facing the other. At the profile the marking is on the rim and
    // the muzzle's own circle clips it to a sliver, which is correct.
    if heading.cos() > 1e-3 {
        return None;
    }
    let n = (1.0 + drop * drop).sqrt();
    Some((radius * heading.sin() / n, radius * drop / n))
}

/// Shortest signed difference between two angles, in (-pi, pi].
fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn rotate_light(dir: Vec3, degrees: f32) -> Vec3 {
    let yaw = dir.x.atan2(dir.z).to_degrees() + degrees;
    let pitch = dir.y.asin().to_degrees();
    Vec3::from_angles(yaw, pitch)
}

fn elevate_light(dir: Vec3, degrees: f32) -> Vec3 {
    let yaw = dir.x.atan2(dir.z).to_degrees();
    let pitch = (dir.y.asin().to_degrees() + degrees).clamp(-80.0, 80.0);
    Vec3::from_angles(yaw, pitch)
}

/// Is the eye's surface at this pixel in front of the nearest head surface?
/// Shared by the sphere, the pupil and the lid so all three shells are clipped
/// against exactly the same thing.
fn eye_pixel_ok(
    geo: &[(f32, f32, f32, f32)],
    x: f32,
    y: f32,
    eye_surface: f32,
    bias: f32,
) -> bool {
    let mut nearest = f32::INFINITY;
    for &(hx, hy, hr, hd) in geo {
        let d2 = (x - hx).powi(2) + (y - hy).powi(2);
        if d2 <= hr * hr {
            let surface = hd - (hr * hr - d2).max(0.0).sqrt();
            if surface < nearest {
                nearest = surface;
            }
        }
    }
    eye_surface < nearest + bias
}

fn report_eyes(w: &Walker) {
    println!(
        "eyes: angle {:.0} deg  radius {:.1}  lift {:.1}  bias {:.1}  lid {:.2} tilt {:.2}",
        w.eye_angle.to_degrees(),
        w.eye_radius,
        w.eye_lift,
        w.eye_bias,
        w.lid_drop,
        w.lid_tilt
    );
}

fn report_mouth(w: &Walker) {
    println!(
        "mouth line: length {:.1}  thick {:.1}  drop {:.2}",
        w.mouth_width, w.mouth_thick, w.mouth_drop
    );
}

/// Put the mouth's live values in the window title, where they can be read
/// without going back to the terminal that started the app.
fn show_mouth(canvas: &mut sdl2::render::Canvas<sdl2::video::Window>, w: &Walker) {
    let _ = canvas.window_mut().set_title(&format!(
        "petz ragdoll sandbox - mouth length {:.1} drop {:.2}",
        w.mouth_width, w.mouth_drop
    ));
}

fn report_ears() {
    println!(
        "ear bases: back {EAR_BACK:.1}  up {EAR_UP:.1}  apart {EAR_SEP:.1}  size {EAR_SIZE:.1}"
    );
    println!(
        "ear tips:  back {EAR_TIP_BACK:.1}  up {EAR_TIP_UP:.1}  size {EAR_TIP_SIZE:.1}  sink {EAR_TIP_SINK:.2}"
    );
}

fn report(light: Light, steps: usize, tex: usize, texture: &str) {    let yaw = light.dir.x.atan2(light.dir.z).to_degrees();
    let pitch = light.dir.y.asin().to_degrees();
    println!(
        "settings: steps={steps} yaw={yaw:.1} pitch={pitch:.1} ambient={:.2} diffuse={:.2} tex={tex} ({texture})",
        light.ambient, light.diffuse
    );
}

/// Where the *nearest point* of an ear's sphere is drawn, given the skull's
/// depth, the ear's own, and the mean of the ear depths.
///
/// Two jobs at once: the ear has to be in front of the skull's rim, which is
/// drawn half a unit behind its centre, or the head's outline runs across the
/// ear; and behind the skull's own surface, so the head still covers the ear
/// where they overlap, exactly as two body segments meet.
///
/// The one thing it must not do is collapse both ears onto the same plane.
/// It did, and then which ear covered the other was decided by draw order
/// rather than by which was nearer - at 90 degrees the far ear won. The
/// deviation *from the pair's own mean* is scaled by KEEP and kept, so the
/// ears hold their real order without either one drifting out from under the
/// head. Deviating from the skull instead would not work: an ear sits 32 above
/// the skull's centre, which at a 25 degree pitch is 13 of depth that has
/// nothing to do with which ear is nearer, and scaling that lands the far ear
/// outside the rim.
fn ear_plane(sk_depth: f32, ear_depth: f32, ear_mean: f32) -> f32 {
    const KEEP: f32 = 0.15;
    (sk_depth - 1.0) + KEEP * (ear_depth - ear_mean)
}

/// The plane each ear ball's *nearest point* is drawn at, in the order the
/// depths are given: base, tip, base, tip.
///
/// Three rules in one place. The pair keeps its own depth order, scaled so the
/// two ears do not collapse onto one plane. The front ear - the one whose base
/// is nearer - is pushed clear by more than a radius, or the two surfaces
/// interleave pixel by pixel and the back ear shows through the front one's
/// texture. And the whole set is shifted together if the back ear would
/// otherwise fall behind the skull's rim, where the head's outline would run
/// across it; shifting the set rather than clamping each plane keeps the tip's
/// offset from its base intact.
fn ear_planes(sk_depth: f32, d: [f32; 4]) -> [f32; 4] {
    let mean = (d[0] + d[1] + d[2] + d[3]) / 4.0;
    let front_is_second = d[2] < d[0];
    let mut planes = [0.0f32; 4];
    for (k, pair) in [(0usize, [0usize, 1usize]), (1, [2, 3])] {
        let push = if (k == 1) == front_is_second { EAR_FRONT } else { 0.0 };
        for j in pair {
            planes[j] = ear_plane(sk_depth, d[j], mean) - push;
        }
    }
    let furthest = planes.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let shift = (sk_depth - 0.5 - furthest).min(0.0);
    for p in planes.iter_mut() {
        *p += shift;
    }
    planes
}

/// `PETZ_WINDOW=x,y`, if it parses, so a scripted run lands where the script
/// says rather than wherever the window manager picks.
fn window_at() -> Option<(i32, i32)> {
    let raw = std::env::var("PETZ_WINDOW").ok()?;
    let (x, y) = raw.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    // Two presentations of the same 320x240 surface. The rasterizer, the depth
    // buffer and the span table are identical either way - only the window you
    // look through changes, so nothing you see in one mode is untrue in the
    // other.
    //
    //   dev     (default)  the surface doubled to 640x480, for working on it
    //   low                the surface at 1:1, 320x240, the low-end target
    //
    // `PETZ_MODE=low` selects the target; `PETZ_SCALE` overrides the window
    // multiplier outright, including to something bigger than either.
    let low_end = std::env::var("PETZ_MODE")
        .map(|m| m.eq_ignore_ascii_case("low") || m.eq_ignore_ascii_case("lowend"))
        .unwrap_or(false);
    let scale = std::env::var("PETZ_SCALE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(if low_end { 1 } else { 2 });

    let mut builder = video.window("petz ragdoll sandbox", (W * scale) as u32, (H * scale) as u32);
    // Where the window lands is not this program's business. It used to centre
    // itself on launch, which moved the window out from under whatever was
    // already there - the sandbox cannot be put beside anything if it relocates
    // itself on every start. Left alone, the window manager places it; pinned
    // runs say where with PETZ_WINDOW=x,y.
    if let Some(at) = window_at() {
        builder.position(at.0, at.1);
    }
    let window = builder.build().map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().build().map_err(|e| e.to_string())?;
    let tc = canvas.texture_creator();
    let mut tex = tc
        .create_texture_streaming(sdl2::pixels::PixelFormatEnum::RGB24, W as u32, H as u32)
        .map_err(|e| e.to_string())?;
    tex.set_scale_mode(sdl2::render::ScaleMode::Nearest);

    let table = load_table();
    let palette = load_palette();

    let mut steps = 6usize;
    let mut ramps: HashMap<u8, Ramp> = HashMap::new();
    let mut rebuild_ramps = |steps: usize, ramps: &mut HashMap<u8, Ramp>| {
        ramps.clear();
        // Every colour the creature actually paints with needs a ramp, or its
        // ball comes out flat - one value regardless of the light.
        for base in [BODY, LIGHT, DARK, MUZZLE_BLUE] {
            ramps.insert(base, Ramp::from_palette(&palette, base, steps));
        }
    };
    rebuild_ramps(steps, &mut ramps);
    let mut light = Light::default();

    let textures: Vec<(&str, Texture)> = vec![
        ("plain", Texture::flat(64, 64, 255)),
        ("dapple", dapple(64, 64, 17, 255)),
        ("stripes", stripes(64, 64, 5, 255)),
    ];
    let mut tex_idx = 0usize;

    let (mut doll, creature) = build();
    let rest = measure(&doll, &creature);
    let mut walker = Walker::new(&creature, doll.particles.len());
    let mut grabbed: Option<usize> = None;
    // Drawing every constraint, so the skeleton can be read while dragging.
    let mut show_links = false;
    // Shift-drag carries the whole animal: where the mouse started, and where
    // the body was in the world when the drag began.
    let mut carrying: Option<(Vec2, WVec3)> = None;
    let mut grab_point = Vec2::new(0.0, 0.0);
    let mut grab_home = Vec2::new(0.0, 0.0);
    let mut grab_side = 0.0f32;
    let mut view = View::new(25.0, VIEW_GROUND);
    let zoom: f32 = std::env::var("PETZ_ZOOM")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(DEFAULT_ZOOM);
    // Projected positions from the last frame; the picker works in screen space.
    let mut screen: Vec<Projected> = Vec::new();

    let sweep_t = 0.0f32;
    let mut sweep_last = Instant::now();
    let mut frame_no = 0u32;
    let mut held: std::collections::HashSet<Keycode> = std::collections::HashSet::new();
    let mut depth = DepthBuffer::new(W, H);
    // Ball numbers, for referring to them. Tab toggles.
    let mut show_ids = true;
    // Diagnostics: red outlines under PETZ_DIAG, so the ear ordering shows up.
    let outline = if std::env::var("PETZ_DIAG").is_ok() { DIAG_OUTLINE } else { OUTLINE };
    let mouth_col = if std::env::var("PETZ_DIAG").is_ok() { DIAG_MOUTH } else { MOUTH };

    let mut events = sdl.event_pump()?;
    let mut paused = false;
    let step = 1.0 / 120.0;
    let mut acc = 0.0f64;
    let mut last = Instant::now();

    let (mode_name, win_w, win_h) = (
        if low_end { "low-end" } else { "dev" },
        W * scale,
        H * scale,
    );
    println!(
        "mode: {mode_name} - surface {W}x{H}, window {win_w}x{win_h}, zoom {zoom}\n\
         drag: left mouse   W/S: forward/back   A/D: turn left/right   +/-: speed   V: pitch\n\
         light: arrows   ambience: Z/X   steps: [ ]   texture: T   flex: , .\n\
         (the ear balls are frozen: no keys - see EAR_* in the source)\n\
         mouth: K/L length, N/M height on the snout (values in the title bar)\n\
         Shift+drag: carry the whole animal   G: constraint links\n\
         Tab: ball numbers   P: settings   R: reset   Space: pause   Esc: quit"
    );
    report(light, steps, tex_idx, textures[tex_idx].0);

    'outer: loop {
        for e in events.poll_iter() {
            match e {
                sdl2::event::Event::Quit { .. } => break 'outer,
                sdl2::event::Event::KeyDown { keycode: Some(k), repeat: false, .. } => {
                    held.insert(k);
                    match k {
                    sdl2::keyboard::Keycode::Escape => break 'outer,
                    sdl2::keyboard::Keycode::R => {
                        // Rebuilding resets indices as well as pose, so the
                        // walker is rebuilt to match the new skeleton.
                        let (d, c) = build();
                        doll = d;
                        walker = Walker::new(&c, doll.particles.len());
                        grabbed = None;
                    }
                    sdl2::keyboard::Keycode::Space => paused = !paused,

                    sdl2::keyboard::Keycode::Comma => {
                        walker.gait.spec.flex_vertical = (walker.gait.spec.flex_vertical - 0.5).max(0.0);
                        walker.gait.spec.flex_lateral = (walker.gait.spec.flex_lateral - 0.5).max(0.0);
                        println!(
                            "flex v{:.1} l{:.1}",
                            walker.gait.spec.flex_vertical, walker.gait.spec.flex_lateral
                        );
                    }
                    sdl2::keyboard::Keycode::Period => {
                        walker.gait.spec.flex_vertical = (walker.gait.spec.flex_vertical + 0.5).min(16.0);
                        walker.gait.spec.flex_lateral = (walker.gait.spec.flex_lateral + 0.5).min(16.0);
                        println!(
                            "flex v{:.1} l{:.1}",
                            walker.gait.spec.flex_vertical, walker.gait.spec.flex_lateral
                        );
                    }

                    sdl2::keyboard::Keycode::V => {
                        let degrees = [0.0f32, 15.0, 25.0, 35.0];
                        let now = view.pitch.to_degrees();
                        let next = degrees.iter().find(|d| **d > now + 0.5).copied().unwrap_or(0.0);
                        view.pitch = next.to_radians();
                        println!("view pitch {next:.0} deg");
                    }
                    sdl2::keyboard::Keycode::Equals | sdl2::keyboard::Keycode::Plus => {
                        walker.gait.spec.speed += 15.0;
                        println!("speed {:.0} px/s", walker.gait.spec.speed);
                    }
                    sdl2::keyboard::Keycode::Minus => {
                        walker.gait.spec.speed = (walker.gait.spec.speed - 15.0).max(0.0);
                        println!("speed {:.0} px/s", walker.gait.spec.speed);
                    }
                    sdl2::keyboard::Keycode::Left => {
                        light.dir = rotate_light(light.dir, -12.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::Right => {
                        light.dir = rotate_light(light.dir, 12.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::Up => {
                        light.dir = elevate_light(light.dir, 8.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::Down => {
                        light.dir = elevate_light(light.dir, -8.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::LeftBracket => {
                        steps = (steps - 1).max(1);
                        rebuild_ramps(steps, &mut ramps);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::RightBracket => {
                        steps = (steps + 1).min(24);
                        rebuild_ramps(steps, &mut ramps);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::T => {
                        tex_idx = (tex_idx + 1) % textures.len();
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::G => {
                        show_links = !show_links;
                        println!("constraint links {}", if show_links { "on" } else { "off" });
                    }
                    sdl2::keyboard::Keycode::Tab => {
                        show_ids = !show_ids;
                        println!("ball numbers {}", if show_ids { "on" } else { "off" });
                    }
                    sdl2::keyboard::Keycode::P => {
                        report(light, steps, tex_idx, textures[tex_idx].0);
                        report_eyes(&walker);
                        report_ears();
                        report_mouth(&walker);
                    }
                    // The mouth, live while its size and height are judged:
                    // radius first, then how far down the snout it sits. The
                    // values are in the title bar until they are settled and the
                    // keys go.
                    sdl2::keyboard::Keycode::K => {
                        walker.mouth_width = (walker.mouth_width - 2.0).max(2.0);
                        report_mouth(&walker);
                        show_mouth(&mut canvas, &walker);
                    }
                    sdl2::keyboard::Keycode::L => {
                        walker.mouth_width = (walker.mouth_width + 2.0).min(30.0);
                        report_mouth(&walker);
                        show_mouth(&mut canvas, &walker);
                    }
                    sdl2::keyboard::Keycode::N => {
                        walker.mouth_drop = (walker.mouth_drop - 0.05).max(-0.8);
                        report_mouth(&walker);
                        show_mouth(&mut canvas, &walker);
                    }
                    sdl2::keyboard::Keycode::M => {
                        walker.mouth_drop = (walker.mouth_drop + 0.05).min(1.5);
                        report_mouth(&walker);
                        show_mouth(&mut canvas, &walker);
                    }
                    // Height as an angle round the head's centre, not an offset
                    // from the ground: raising an ear walks it up the sphere,
                    // the way its radius and width already do.
                    sdl2::keyboard::Keycode::Z => {
                        light.ambient = (light.ambient - 0.05).max(0.0);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    sdl2::keyboard::Keycode::X => {
                        light.ambient = (light.ambient + 0.05).min(0.9);
                        report(light, steps, tex_idx, textures[tex_idx].0);
                    }
                    _ => {}
                    }
                }
                sdl2::event::Event::KeyUp { keycode: Some(k), .. } => {
                    held.remove(&k);
                }
                sdl2::event::Event::MouseButtonDown {
                    x, y, mouse_btn: sdl2::mouse::MouseButton::Left, ..
                } => {
                    // Pick in screen space: with a projection there is no single
                    // world point under the cursor.
                    let (mx, my) = (x as f32 / scale as f32, y as f32 / scale as f32);
                    let mut best: Option<(usize, f32)> = None;
                    for (i, p) in screen.iter().enumerate() {
                        let d = ((p.x - mx).powi(2) + (p.y - my).powi(2)).sqrt();
                        if d < 30.0 && best.map_or(true, |(_, bd)| d < bd) {
                            best = Some((i, d));
                        }
                    }
                    let shift =
                        held.contains(&Keycode::LShift) || held.contains(&Keycode::RShift);
                    if shift {
                        // Carry the whole animal. The pose keeps driving it, so
                        // it holds its shape; only the body's world position
                        // moves.
                        carrying = Some((Vec2::new(mx, my), walker.body.pos));
                        println!("carrying the animal");
                    } else if let Some((i, _)) = best {
                        // Grabbing hands the animal to physics: the gait stops
                        // driving it and it goes limp.
                        doll.release_all();
                        doll.particles[i].pinned = true;
                        grab_point = Vec2::new(mx, my);
                        grab_home = doll.particles[i].pos;
                        grab_side = doll.particles[i].side;
                        grabbed = Some(i);
                        println!("grab ball {i}");
                    } else {
                        println!("grab: nothing within reach");
                    }
                }
                sdl2::event::Event::MouseButtonUp { mouse_btn: sdl2::mouse::MouseButton::Left, .. } => {
                    if carrying.take().is_some() {
                        println!("put the animal down");
                    }
                    if let Some(i) = grabbed.take() {
                        println!("release ball {i}");
                        doll.particles[i].pinned = false;
                        // Side is authored, not solved: undo any lateral drag.
                        doll.restore_sides();

                        // The grab moved the skeleton *within* body space, and
                        // the pose drive pulls body space back to rest - which
                        // is why letting go rewound the animal to where the
                        // grab began. Re-centre body space on the rest pose and
                        // carry the world position with it.
                        let n = creature.spine.len().max(1) as f32;
                        let mut cur = Vec2::new(0.0, 0.0);
                        let mut rst = Vec2::new(0.0, 0.0);
                        for (k, &idx) in creature.spine.iter().enumerate() {
                            cur.x += doll.particles[idx].pos.x;
                            cur.y += doll.particles[idx].pos.y;
                            rst.x += rest.spine[k].x;
                            rst.y += rest.spine[k].y;
                        }
                        let shift = Vec2::new((cur.x - rst.x) / n, (cur.y - rst.y) / n);
                        for p in doll.particles.iter_mut() {
                            p.pos.x -= shift.x;
                            p.pos.y -= shift.y;
                            p.prev.x -= shift.x;
                            p.prev.y -= shift.y;
                        }
                        // Horizontally, move the origin by the same distance so
                        // the picture does not jump. Vertically we let it fall,
                        // which is what dropping something should look like.
                        let f = walker.body.forward();
                        walker.body.pos.x += shift.x * f.x;
                        walker.body.pos.y += shift.x * f.y;
                    }
                }
                sdl2::event::Event::MouseMotion { x, y, .. } => {
                    let carried = carrying;
                    if let Some((start, home)) = carried {
                        let mouse = Vec2::new(x as f32 / scale as f32, y as f32 / scale as f32);
                        // Screen x is world x. Screen y carries height and
                        // distance together, so a vertical drag moves the animal
                        // along the ground: `sin(pitch)` is how much of a step
                        // away shows up as a step up the screen.
                        let dx = mouse.x - start.x;
                        let pitch = view.pitch.sin().max(0.2);
                        let away = -(mouse.y - start.y) / pitch;
                        walker.body.pos = WVec3::new(home.x + dx, home.y + away, home.z);
                    } else if let Some(i) = grabbed {
                        // Drag by *delta*, never by absolute position. An
                        // absolute unprojection depends on a depth that goes
                        // stale, and a wrong depth throws the ball hundreds of
                        // pixels - which the springs then convert into an
                        // explosion.
                        let mouse = Vec2::new(x as f32 / scale as f32, y as f32 / scale as f32);
                        let (dx, dy) = (mouse.x - grab_point.x, mouse.y - grab_point.y);

                        // Screen x maps onto whichever body axis points across
                        // the view; screen y is mostly height, compressed by
                        // the view pitch.
                        let f = walker.body.forward();
                        let r = walker.body.right();
                        let mut fwd = dx * f.x;
                        let mut lat = dx * r.x;
                        let mut up = -dy / view.pitch.cos().max(0.35);

                        // Keep a single yank from outrunning the springs.
                        const LIMIT: f32 = 90.0;
                        let len = (fwd * fwd + lat * lat + up * up).sqrt();
                        if len > LIMIT {
                            let k = LIMIT / len;
                            fwd *= k;
                            lat *= k;
                            up *= k;
                        }

                        if let Some(p) = doll.particles.get_mut(i) {
                            p.pos = Vec2::new(grab_home.x + fwd, grab_home.y - up);
                            p.prev = p.pos;
                            p.side = grab_side + lat;
                        }
                    }
                }
                _ => {}
            }
        }

        let now = Instant::now();
        let frame_dt = (now - last).as_secs_f32().min(0.1);
        acc += frame_dt as f64;
        last = now;

        // Instrument: rotate steadily and stand still, so a sweep can be read
        // off the frame dump without touching the keyboard.
        if let Ok(rate) = std::env::var("PETZ_SWEEP") {
            let rate = rate.parse::<f32>().unwrap_or(0.35).max(0.02);
            walker.body.heading += rate * frame_dt;
        }
        if let Ok(h) = std::env::var("PETZ_HEADING") {
            if let Ok(deg) = h.parse::<f32>() {
                let rad = deg.to_radians();
                walker.body.heading = rad;
                walker.heading_body = rad;
                for f in walker.facing.iter_mut() {
                    *f = rad;
                }
                walker.moving = 0.0;
            }
        }
        let moving = if held.contains(&Keycode::W) {
            1.0
        } else if held.contains(&Keycode::S) {
            -1.0
        } else {
            0.0
        };
        let turning = if held.contains(&Keycode::A) || held.contains(&Keycode::Q) {
            -1.0
        } else if held.contains(&Keycode::D) || held.contains(&Keycode::E) {
            1.0
        } else {
            0.0
        };

        if !paused {
            while acc >= step {
                if grabbed.is_none() {
                    walker.advance(step as f32, moving, turning);
                    walker.pose(&mut doll, &creature, &rest);
                }
                doll.step(step as f32);
                acc -= step;
            }
        }

        // Place every ball in the world and project it. The z we get back is
        // the draw order: there is no z-buffer, only a painter.
        screen.clear();
        for (i, p) in doll.particles.iter().enumerate() {
            // A hip is welded to its spine ball: derived here from that ball's
            // live position and heading, so the gap cannot move at all. Placing
            // it in the pose is not enough - the spine keeps moving inside the
            // same step, after the placement.
            let (pos, side, heading) = match creature.legs.iter().find(|l| l.hip == i) {
                Some(leg) => (
                    doll.particles[leg.attach].pos.add(leg.offset),
                    doll.particles[leg.attach].side + leg.side_offset,
                    leg.attach,
                ),
                None => (p.pos, p.side, i),
            };
            let world = walker.part(heading).to_world(
                (pos.x - BODY_ORIGIN) * zoom,
                side * zoom,
                (GROUND - pos.y) * zoom,
            );
            screen.push(view.project(world));
        }

        let mut surface = Surface::new(W, H);
        for y in 0..H {
            for x in 0..W {
                surface.set(x, y, BACKGROUND);
            }
        }

        // Every ball goes through one depth-tested rasterizer, in any order.
        // The sort, the containment lists and the per-eye occlusion closures
        // are all gone: per-pixel depth answers "what is in front" once, for
        // every pair of balls, at every angle.
        depth.clear();

        let pattern_for = |colour: u8| {
            let alt = match colour {
                BODY => LIGHT,
                LIGHT => BODY,
                other => other,
            };
            Pattern {
                base: ramps.get(&colour).cloned().unwrap_or_else(|| Ramp::new(vec![colour])),
                alt: ramps.get(&alt).cloned().unwrap_or_else(|| Ramp::new(vec![alt])),
                texture: textures[tex_idx].1.clone(),
                threshold: 128,
                phase: 0.0,
            }
        };

        let eye_set: std::collections::HashSet<usize> = creature
            .eyes
            .iter()
            .copied()
            .chain(creature.eye_overlays.iter().map(|(_, o)| *o))
            .collect();

        // The ears ride just in front of the skull, so the head's outline does
        // not run across them, while staying behind the skull's own surface so
        // the head still covers them where they overlap. The eyes are exempt:
        // their outline is supposed to sit on the head.
        {
            let sk_depth = screen[creature.skull].depth;
            let ear_balls: Vec<usize> = creature
                .ears
                .iter()
                .flat_map(|&(base, tip, _)| [base, tip])
                .collect();
            let mean = ear_balls.iter().map(|&i| screen[i].depth).sum::<f32>()
                / ear_balls.len() as f32;
            // The ear planes, then each ball's bias from its own depth. See
            // `ear_planes` for why they are worked out as a set.
            let order = [
                creature.ears[0].0, creature.ears[0].1,
                creature.ears[1].0, creature.ears[1].1,
            ];
            let depths = [
                screen[order[0]].depth, screen[order[1]].depth,
                screen[order[2]].depth, screen[order[3]].depth,
            ];
            let planes = ear_planes(sk_depth, depths);
            for (n, &ear) in order.iter().enumerate() {
                let r = doll.particles[ear].radius * zoom;
                let d = screen[ear].depth;
                // The rasterizer puts a ball's *surface* at `centre - nz *
                // radius`, so the plane has to be carried by the radius: put
                // the sphere's nearest point on the plane, not its centre.
                if let Some(p) = doll.particles.get_mut(ear) {
                    p.z_bias = (planes[n] + r) - d;
                }
            }
        }

        // ---- ordinary balls --------------------------------------------------
        for i in 0..doll.particles.len() {
            if eye_set.contains(&i) {
                continue;
            }
            let p = &doll.particles[i];
            let r = (p.radius * zoom).round() as i32;
            if r < 1 { continue; }
            let (cx, cy) = (screen[i].x.round() as i32, screen[i].y.round() as i32);
            let cd = screen[i].depth + p.z_bias;
            if !p.no_rim {
                // A filled disc behind the body, not a ring: the body is
                // nearer, covers the middle, and the rim survives as the
                // two-pixel annulus with no rounding gap.
                //
                // The push has to beat the radius difference, not be some flat
                // epsilon. A disc of radius r+2 has its surface 2*nz nearer than
                // one of radius r at the same centre, so anything under 2 lets
                // the rim win across the whole ball - which is exactly what
                // happened: the dog came out entirely in outline colour.
                let rim = if p.rim == 0 { outline } else { p.rim };
                // Outline weight: the body's diameter plus four, so the
                // annulus that survives is two pixels all the way round. An odd
                // offset makes it one pixel on some edges and two on others.
                draw_flat_disc(&mut surface, &mut depth, &table, cx, cy,
                    ((r * 2) + 4) as usize, cd + 0.5, rim, &|_, _| true);
            }
            let pat = pattern_for(p.colour);
            draw_disc(&mut surface, &mut depth, &table, cx, cy, (r * 2) as usize, cd, 0.0,
                Paint::Shaded { light, pattern: &pat }, &|_, _| true);
        }

        // ---- eyes: three shells ---------------------------------------------
        let heading = walker.heading_body;
        let (pc, ps) = (view.pitch.cos(), view.pitch.sin());
        let mut groups: Vec<(usize, usize)> =
            creature.eye_parts.iter().map(|&(s, i, _, _)| (s, i)).collect();
        groups.sort_by(|a, b| screen[b.0].depth.partial_cmp(&screen[a.0].depth).unwrap());
        for &(sclera, iris) in &groups {
            let sp = &doll.particles[sclera];
            let cp = &doll.particles[creature.skull];
            let eh = walker.facing.get(sclera).copied().unwrap_or(heading);
            let fe = Body { pos: walker.body.pos, heading: eh }.forward();

            // Does the white sphere face the viewer? Its normal is the offset
            // from the head's centre.
            let (dx, dl, du) = (sp.pos.x - cp.pos.x, sp.side - cp.side, cp.pos.y - sp.pos.y);
            let n_y = dx * eh.cos() - dl * eh.sin();
            let inner_visible = (-pc * n_y + ps * du) > 0.0;

            let (cx, cy) = (screen[sclera].x, screen[sclera].y);
            let ix = cx.round() as i32;
            let iy = cy.round() as i32;
            let re = (sp.radius * zoom).max(1.0);
            let de = screen[sclera].depth;

            if inner_visible {
                // Shell 2: the white sphere, a full circle.
                let pat = pattern_for(SCLERA);
                draw_disc(&mut surface, &mut depth, &table, ix, iy, (re as i32 * 2) as usize,
                    de, 0.0, Paint::Shaded { light, pattern: &pat }, &|_, _| true);

                // Shell 3: pupil and iris, sliding inside the sphere and
                // clipped by its rim rather than by the head.
                let off = re * fe.x;
                let px = (cx + off).round() as i32;
                let ip = &doll.particles[iris];
                let ri = (ip.radius * zoom).max(1.0);
                let inside = {
                    let (cx0, cy0, re0) = (cx, cy, re);
                    move |x: i32, y: i32| -> bool {
                        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                        (fx - cx0).powi(2) + (fy - cy0).powi(2) <= re0 * re0
                    }
                };
                // Both sit on the sclera's surface, not on a sphere of their
                // own: a pupil with its own depth is inside the eye it is
                // painted on, and the sclera hides it.
                draw_disc_on_sphere(&mut surface, &mut depth, &table, px, iy,
                    ((ri as i32 * 2) + 4) as usize, cx, cy, re, de, -0.6, IRIS_RING, &inside);
                draw_disc_on_sphere(&mut surface, &mut depth, &table, px, iy,
                    (ri as i32 * 2) as usize, cx, cy, re, de, -0.9, IRIS, &inside);
            }

            // Shell 1: the lid. Never culled - it is coat colour, so over the
            // head it is invisible, and where the eye stands against something
            // else it is the outline that should persist. When there is no
            // inside to see, it fills the whole disc and the eye is shut.
            let drop = walker.lid_drop.clamp(0.0, 1.0);
            let tilt = walker.lid_tilt;
            let line = re * (1.0 - 2.0 * drop);
            let outer = re + 2.0;
            let lid_keep = move |x: i32, y: i32| -> bool {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (ux, uy) = (fx - cx, fy - cy);
                if ux * ux + uy * uy > outer * outer {
                    return false;
                }
                if !inner_visible {
                    return true;
                }
                uy < -(line + ux * tilt.tan())
            };
            draw_ring(&mut surface, &mut depth, &table, ix, iy,
                ((outer as i32 * 2) as usize).min(255), 2, de, -2.0, outline, &lid_keep);
            let lpat = pattern_for(LID);
            draw_disc(&mut surface, &mut depth, &table, ix, iy,
                ((outer as i32 * 2) as usize).min(255), de, -2.1,
                Paint::Shaded { light, pattern: &lpat }, &lid_keep);
        }

        // ---- the mouth: a disc painted onto the muzzle ----------------------
        // The pupil's trick applied to the snout. Its depth comes from the
        // muzzle's sphere rather than a sphere of its own, so it lies on the
        // surface instead of sinking into it, and the muzzle's rim clips it, so
        // it goes round the side of the snout instead of floating off it.
        {
            let m = creature.muzzle;
            let mp = &doll.particles[m];
            let mh = walker.facing.get(m).copied().unwrap_or(heading);
            // Round exactly as the ball was drawn, or the mouth's sphere is a
            // fraction smaller than the muzzle's and the muzzle's own surface
            // wins the depth test everywhere but the middle - which is what
            // happened: a 4 px mouth came out as two pixels.
            let re = (mp.radius * zoom).round().max(1.0);
            // The centre the muzzle was *drawn* at, not the projected one: the
            // ball rasterizer rounds its centre to whole pixels and solves its
            // surface from there. A reference sphere half a pixel away has a
            // surface up to a unit off, which is enough to beat the bias and
            // let the muzzle draw over the mouth everywhere but the middle.
            let (cx, cy) = (screen[m].x.round() + 0.5, screen[m].y.round() + 0.5);
            let de = screen[m].depth;
            let off = mouth_offset(mh, re, walker.mouth_drop);
            if let Some((ox, oy)) = off {
                let inside = {
                    let (cx0, cy0, re0) = (cx, cy, re);
                    move |x: i32, y: i32| -> bool {
                        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                        (fx - cx0).powi(2) + (fy - cy0).powi(2) <= re0 * re0
                    }
                };
                // A line is a disc with everything but one band of rows
                // clipped away: the disc gives the length (its middle rows are
                // its widest) and the band gives the thickness, so the line is
                // painted with the same per-pixel sphere depth a disc gets and
                // lies on the snout instead of across it.
                let len = (walker.mouth_width * zoom).round().max(1.0) as i32;
                let thick = (walker.mouth_thick * zoom).round().max(1.0) as i32;
                let ly = (cy + oy) as i32;
                let (lo, hi) = (ly - (thick - 1) / 2, ly + thick / 2);
                let band = move |x: i32, y: i32| -> bool { inside(x, y) && y >= lo && y <= hi };
                draw_disc_on_sphere(
                    &mut surface, &mut depth, &table,
                    (cx + ox) as i32, ly,
                    len as usize,
                    cx, cy, re, de, MOUTH_BIAS, mouth_col, &band,
                );
            }
        }

        // Dump our own framebuffer, bypassing the window system entirely. Screen
        // grabs are unreliable when other windows are about.
        if std::env::var("PETZ_DUMP_PPM").is_ok() {
            use std::io::Write;
            let rgb = palette.to_rgb(&surface);
            let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
            out.extend_from_slice(&rgb);
            if let Ok(mut f) = std::fs::File::create("/tmp/ours.ppm") {
                let _ = f.write_all(&out);
            }
            let dmin = screen.iter().map(|p| p.depth).fold(f32::INFINITY, f32::min);
            let dmax = screen.iter().map(|p| p.depth).fold(f32::NEG_INFINITY, f32::max);
            let nan = screen.iter().filter(|p| !p.depth.is_finite()).count();
            let xmin = screen.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
            let xmax = screen.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
            let ymin = screen.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
            let ymax = screen.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
            println!("dump: {} balls, depth {:.0}..{:.0}, screen x {:.0}..{:.0} y {:.0}..{:.0}",
                doll.particles.len(), dmin, dmax, xmin, xmax, ymin, ymax);
            if std::env::var("PETZ_DIAG").is_ok() {
                for (i, p) in doll.particles.iter().enumerate() {
                    println!("part {i:2} r{:5.1} colour {:3} at ({:.0},{:.0}) side {:5.1} screen ({:.1},{:.1}) depth {:.1}",
                        p.radius, p.colour, p.pos.x, p.pos.y, p.side, screen[i].x, screen[i].y, screen[i].depth);
                }
                for &(base, tip, _) in &creature.ears {
                    for e in [base, tip] {
                        println!("dump ear {e:2} at ({:.1},{:.1}) r{:.1} depth {:.2} bias {:.2} cd {:.2}",
                            screen[e].x, screen[e].y,
                            doll.particles[e].radius,
                            screen[e].depth, doll.particles[e].z_bias,
                            screen[e].depth + doll.particles[e].z_bias);
                    }
                }
            }
        }

        let rgb = palette.to_rgb(&surface);
        tex.update(None, &rgb, W * 3).map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&tex, None, None).map_err(|e| e.to_string())?;

        // Facing, in degrees: 0 is away from the viewer (up the screen), 90 is
        // to the right, 180 is towards the viewer, 270 to the left.
        let facing = walker.heading_body.to_degrees().rem_euclid(360.0).round() as i32;
        draw_number(&mut canvas, 18, 18, 4, facing);

        // A number on every ball, just above it, so one can be named exactly.
        if show_ids {
            for (i, p) in screen.iter().enumerate() {
                let x = (p.x * scale as f32) as i32;
                let y = (p.y * scale as f32) as i32 - doll.particles[i].radius as i32 * 2 - 12;
                if x < -20 || y < -20 || x > (W * scale) as i32 || y > (H * scale) as i32 {
                    continue;
                }
                draw_number(&mut canvas, x, y, 2, i as i32);
            }
        }

        // The skeleton: every spring drawn between the balls it joins, so what
        // is connected to what can be seen while dragging one. Soft constraints
        // are drawn dashed-short by drawing them dimmer.
        if show_links {
            canvas.set_draw_color(Color::RGB(0, 255, 255));
            for c in &doll.constraints {
                let a = screen.get(c.a);
                let b = screen.get(c.b);
                if let (Some(a), Some(b)) = (a, b) {
                    let p1 = Point::new((a.x * scale as f32) as i32, (a.y * scale as f32) as i32);
                    let p2 = Point::new((b.x * scale as f32) as i32, (b.y * scale as f32) as i32);
                    let _ = canvas.draw_line(p1, p2);
                }
            }
        }

        canvas.present();
    }
    Ok(())
}

#[cfg(test)]
mod walk_tests {
    use super::*;

    fn rig() -> (Ragdoll, Creature, Pose, Walker) {
        let (doll, c) = build();
        let rest = measure(&doll, &c);
        let w = Walker::new(&c, doll.particles.len());
        (doll, c, rest, w)
    }

    /// Run the walk loop exactly as `main` does, without a window.
    fn run(seconds: f32) -> (Ragdoll, Creature, Walker) {
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let steps = (seconds / dt) as usize;
        for _ in 0..steps {
            w.advance(dt, 1.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
        }
        (doll, c, w)
    }

    #[test]
    fn walking_moves_the_animal_at_the_gait_speed() {
        let (d0, c0, _r, w0) = rig();
        let start = w0.body.pos;
        let _ = (d0, c0);
        let (_, _, w) = run(2.0);
        let travelled = w.body.pos.sub(start).length();
        let expected = w.gait.spec.speed * 2.0;
        assert!(
            (travelled - expected).abs() < 1.0,
            "travelled {travelled} px, expected {expected}"
        );
    }

    #[test]
    fn walking_away_takes_the_animal_up_the_screen() {
        let view = View::new(25.0, GROUND);
        let (doll, c, w) = run(2.0);
        let midpoint = |d: &Ragdoll| {
            let p = &d.particles[c.spine[2]];
            view.project(w.body.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y))
        };
        let now = midpoint(&doll);
        let (d0, _, _, w0) = rig();
        let p0 = &d0.particles[c.spine[2]];
        let start = View::new(25.0, GROUND).project(
            w0.body.to_world(p0.pos.x - BODY_ORIGIN, p0.side, GROUND - p0.pos.y),
        );
        assert!(
            now.y < start.y - 10.0,
            "walking away did not move up the screen: {:.1} -> {:.1}",
            start.y,
            now.y
        );
        let _ = w;
    }

    #[test]
    fn the_body_turn_is_rate_limited() {
        let (_, _, _, mut w) = rig();
        let before = w.heading_body;
        w.body.heading = before + 1.2; // ask for a hard turn
        w.advance(1.0 / 120.0, 1.0, 0.0);
        let moved = (w.heading_body - before).abs();
        assert!(
            moved <= MAX_TURN_RATE / 120.0 + 1e-4,
            "body turned {moved} rad in one step, limit is {}",
            MAX_TURN_RATE / 120.0
        );
        assert!(moved > 0.0, "the body ignored the turn entirely");
    }

    #[test]
    fn a_turn_reaches_the_head_before_the_tail() {
        let (_, c, _, mut w) = rig();
        let target = w.heading_body + 1.0;
        w.body.heading = target;
        for _ in 0..12 {
            w.advance(1.0 / 120.0, 1.0, 0.0);
        }
        let head_err = wrap_pi(target - w.facing[c.head[1]]).abs();
        let tail_err = wrap_pi(target - w.facing[*c.tail.last().unwrap()]).abs();
        assert!(
            head_err < tail_err,
            "head lagged the tail: head {head_err:.3}, tail {tail_err:.3}"
        );
        // ...and the whole animal must not have arrived at once.
        assert!(tail_err > 0.05, "the tail snapped instantly: {tail_err:.3}");
    }

    #[test]
    fn a_turn_propagates_and_then_settles() {
        let (_, c, _, mut w) = rig();
        let target = w.heading_body - 0.9;
        w.body.heading = target;
        for _ in 0..900 {
            w.advance(1.0 / 120.0, 1.0, 0.0);
        }
        // The head is close to the command almost immediately, the tail last.
        let head_err = wrap_pi(target - w.facing[c.head[1]]).abs();
        let tail_err = wrap_pi(target - w.facing[*c.tail.last().unwrap()]).abs();
        assert!(head_err < 0.05, "head never arrived: {head_err:.3}");
        assert!(tail_err < 0.05, "tail never arrived: {tail_err:.3}");
    }

    #[test]
    fn parts_hold_different_facings_during_a_turn() {
        // The whole point: mid-turn the animal is not rigid.
        let (_, c, _, mut w) = rig();
        w.body.heading = w.heading_body + 1.2;
        for _ in 0..20 {
            w.advance(1.0 / 120.0, 1.0, 0.0);
        }
        let head = w.facing[c.head[1]];
        let tail = w.facing[*c.tail.last().unwrap()];
        let spread = wrap_pi(head - tail).abs();
        assert!(spread > 0.08, "the body is still rigid: {spread:.3} rad between head and tail");
    }

    #[test]
    fn the_spine_bends_while_walking() {
        // Fit a line through the spine and measure how far its points sit off
        // it. A rigid spine keeps them on the line; a flexing one does not.
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let mut worst: f32 = 0.0;
        for _ in 0..240 {
            w.advance(dt, 1.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
            let pts: Vec<(f32, f32)> = c
                .spine
                .iter()
                .map(|&i| (doll.particles[i].pos.x, doll.particles[i].pos.y))
                .collect();
            let (x0, y0) = pts[0];
            let (x1, y1) = pts[pts.len() - 1];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            for &(px, py) in &pts[1..pts.len() - 1] {
                // Perpendicular distance from the root-to-shoulder line.
                let d = ((px - x0) * dy - (py - y0) * dx).abs() / len;
                worst = worst.max(d);
            }
        }
        assert!(worst > 0.4, "spine stayed straight; max deviation {worst:.2} px");
    }

    #[test]
    fn nothing_floats_after_a_grab() {
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        for _ in 0..120 {
            w.advance(dt, 1.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
        }
        // Grab the middle of the back and yank it up and forward.
        let i = c.spine[2];
        doll.release_all();
        doll.particles[i].pinned = true;
        let home = doll.particles[i].pos;
        let side0 = doll.particles[i].side;
        for k in 0..90 {
            let p = &mut doll.particles[i];
            p.pos = Vec2::new(home.x + k as f32 * 0.6, home.y - k as f32 * 0.6);
            p.prev = p.pos;
            doll.step(dt);
        }
        doll.particles[i].pinned = false;
        doll.restore_sides();
        let _ = side0;
        // Same re-centring the release handler does.
        let n = c.spine.len().max(1) as f32;
        let mut cur = Vec2::new(0.0, 0.0);
        let mut rst = Vec2::new(0.0, 0.0);
        for (k, &idx) in c.spine.iter().enumerate() {
            cur.x += doll.particles[idx].pos.x;
            cur.y += doll.particles[idx].pos.y;
            rst.x += rest.spine[k].x;
            rst.y += rest.spine[k].y;
        }
        let shift = Vec2::new((cur.x - rst.x) / n, (cur.y - rst.y) / n);
        for p in doll.particles.iter_mut() {
            p.pos.x -= shift.x;
            p.pos.y -= shift.y;
            p.prev.x -= shift.x;
            p.prev.y -= shift.y;
        }
        // Let it walk on for three seconds.
        for _ in 0..360 {
            w.advance(dt, 1.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
        }
        // Every ball should end up within a body's reach of the spine.
        let mut cx = 0.0;
        let mut cy = 0.0;
        for &idx in &c.spine {
            cx += doll.particles[idx].pos.x;
            cy += doll.particles[idx].pos.y;
        }
        cx /= n;
        cy /= n;
        for (k, p) in doll.particles.iter().enumerate() {
            let d = (p.pos.x - cx).hypot(p.pos.y - cy);
            assert!(d < 260.0, "ball {k} stranded {d:.0} px from the body");
        }
    }

    /// Frame the creature at a heading and report which eyes are visible,
    /// using the same two tests the renderer applies: does the white sphere
    /// face the viewer, and is its centre outside the head.
    fn visible_eyes_at(heading: f32, radius: f32) -> Vec<bool> {
        let (mut doll, c, rest, mut w) = rig();
        w.eye_radius = radius;
        w.body.heading = heading;
        w.heading_body = heading;
        for f in w.facing.iter_mut() {
            *f = heading;
        }
        for _ in 0..90 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let view = View::new(25.0, GROUND);
        let proj = |i: usize| {
            let p = &doll.particles[i];
            let b = Body { pos: w.body.pos, heading };
            view.project(b.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y))
        };
        let heads: Vec<(f32, f32, f32, f32)> = c
            .head_balls
            .iter()
            .map(|&i| (i, proj(i)))
            .map(|(i, p)| (p.x, p.y, doll.particles[i].radius.max(1.0), p.depth))
            .collect();
        let cp = &doll.particles[c.skull];
        let (pc, ps) = (view.pitch.cos(), view.pitch.sin());
        c.eyes
            .iter()
            .map(|&sclera| {
                let sp = &doll.particles[sclera];
                // normal = offset from the head's centre; does it face the camera?
                let (dx, dl, du) = (
                    sp.pos.x - cp.pos.x,
                    sp.side - cp.side,
                    cp.pos.y - sp.pos.y,
                );
                let ny = dx * heading.cos() - dl * heading.sin();
                if -pc * ny + ps * du <= 0.0 {
                    return false;
                }
                let e = proj(sclera);
                let re = sp.radius.max(1.0);
                let eye_surface = e.depth - re;
                let mut nearest = f32::INFINITY;
                for &(hx, hy, hr, hd) in &heads {
                    let d2 = (e.x - hx).powi(2) + (e.y - hy).powi(2);
                    if d2 <= hr * hr {
                        let surf = hd - (hr * hr - d2).max(0.0).sqrt();
                        if surf < nearest {
                            nearest = surf;
                        }
                    }
                }
                eye_surface < nearest + w.eye_bias
            })
            .collect()
    }

    fn visible_eyes(heading: f32) -> Vec<bool> {
        visible_eyes_at(heading, 29.0)
    }

    /// Does this eye face the viewer? On the sphere the eye sits at
    /// radius * (cos angle forward, sin angle sideways), so the facing test
    /// reduces to radius * cos(angle +/- heading) - the radius cancels out of
    /// the sign and cannot flip an eye on its own.
    fn faces_viewer(heading: f32, angle: f32, sign: f32) -> bool {
        let (cx, sx) = (heading.cos(), heading.sin());
        let (ca, sa) = (angle.cos(), angle.sin());
        let forward = ca;
        let lateral = sign * sa;
        let away = forward * cx - lateral * sx;
        away < 0.0
    }

    #[test]
    fn one_eye_in_profile_and_two_from_the_front() {
        use std::f32::consts::{FRAC_PI_2, PI};
        assert_eq!(visible_eyes(0.0), vec![false, false], "facing away should show no eyes");
        let profile = visible_eyes(FRAC_PI_2);
        assert_eq!(profile.iter().filter(|v| **v).count(), 1, "profile shows one eye: {profile:?}");
        let front = visible_eyes(PI);
        assert_eq!(front.iter().filter(|v| **v).count(), 2, "facing the camera shows both: {front:?}");
    }

    #[test]
    fn both_eyes_stay_visible_across_the_front_of_the_turn() {
        let mut both = 0;
        for deg in 0..360 {
            let h = (deg as f32).to_radians();
            if visible_eyes(h).iter().filter(|v| **v).count() == 2 {
                both += 1;
            }
        }
        assert!(both >= 30, "both eyes are only up for {both} degrees of the turn");
    }

    #[test]
    fn the_radius_cannot_flip_which_way_an_eye_faces() {
        // Reported from play: at 225 degrees, tapping the radius keys made the
        // left eye vanish. The facing rule must not depend on the radius; the
        // occlusion rule legitimately does, because a smaller radius buries the
        // eye in the head.
        let heading = 225.0_f32.to_radians();
        let angle = 27.0_f32.to_radians();
        let left = faces_viewer(heading, angle, -1.0);
        let right = faces_viewer(heading, angle, 1.0);
        assert!(left && right, "both eyes should face the viewer at 225 degrees");
    }

    #[test]
    fn the_two_eyes_are_level_when_the_animal_faces_straight_away() {
        // Reported from play: at 360 degrees the right eye sits higher than the
        // left. At exactly 0 both eyes are at the same depth, and the projection
        // only turns depth into height, so they must be level.
        let (mut doll, c, rest, mut w) = rig();
        w.body.heading = 0.0;
        w.heading_body = 0.0;
        for f in w.facing.iter_mut() {
            *f = 0.0;
        }
        for _ in 0..120 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let view = View::new(25.0, GROUND);
        let ys: Vec<f32> = c
            .eyes
            .iter()
            .map(|&i| {
                let p = &doll.particles[i];
                let b = Body { pos: w.body.pos, heading: 0.0 };
                view.project(b.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y)).y
            })
            .collect();
        let d = (ys[0] - ys[1]).abs();
        let info: Vec<(f32, f32, f32)> = c
            .eyes
            .iter()
            .map(|&i| (doll.particles[i].pos.x, doll.particles[i].pos.y, doll.particles[i].side))
            .collect();
        assert!(d < 0.6, "eyes differ in height by {d:.2} px: {ys:?} positions {info:?}");
    }

    #[test]
    fn the_head_has_the_expected_parts() {
        // Counts, because a bad edit once duplicated the eye block and the dog
        // came out with four of everything.
        let (doll, c, _, _) = rig();
        let count = |colour: u8| doll.particles.iter().filter(|p| p.colour == colour).count();
        // Named by index, not by colour: the diagnostic palette paints one of
        // the spine balls white, which is the sclera's colour.
        assert!(
            c.eyes.iter().all(|&i| doll.particles[i].colour == SCLERA),
            "sclerae"
        );
        assert!(
            c.eye_overlays.iter().all(|&(_, i)| doll.particles[i].colour == IRIS),
            "pupils"
        );
        assert!(
            c.ears.iter().all(|&(base, _, _)| doll.particles[base].colour == MUZZLE_BLUE),
            "ear bases carry the snout's blue"
        );
        assert_eq!(c.eyes.len(), 2, "two eyes");
        assert_eq!(c.ears.len(), 2, "two ears, each base plus tip");
        // Counts by colour stopped meaning anything once the palette went
        // diagnostic and every fur ball became the same black.
        for &(base, tip, _) in &c.ears {
            assert!(c.head.contains(&base) && c.head.contains(&tip), "ear balls are on the head");
            assert_ne!(base, tip, "a base is not its own tip");
        }
        assert_eq!(c.eye_overlays.len(), 2, "one pupil per eye");
        assert_eq!(c.head.len(), 11, "skull, muzzle, nose, four ear balls, four eye balls");
        assert_eq!(doll.particles.len(), 48, "total ball count: 32 plus sixteen claws");
    }

    #[test]
    fn the_two_ears_share_a_vertical_axis_in_profile() {
        // Reported from play: at 90 degrees the ears did not line up. Their
        // separation is along world x, which is the screen's horizontal, so both
        // have the same depth and therefore the same height at every heading.
        let view = View::new(25.0, GROUND);
        for deg in [0.0f32, 45.0, 90.0, 135.0, 180.0, 270.0] {
            let (mut doll, c, rest, mut w) = rig();
            w.body.heading = deg.to_radians();
            w.heading_body = w.body.heading;
            for f in w.facing.iter_mut() {
                *f = w.body.heading;
            }
            for _ in 0..120 {
                w.pose(&mut doll, &c, &rest);
                doll.step(1.0 / 120.0);
            }
            let at = |i: usize| {
                let p = &doll.particles[i];
                let b = Body { pos: w.body.pos, heading: w.body.heading };
                let s = view.project(b.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y));
                (s.x, s.y)
            };
            let pts: Vec<((f32, f32), (f32, f32))> = c
                .ears
                .iter()
                .map(|&(base, tip, _)| (at(base), at(tip)))
                .collect();
            if (deg - 90.0).abs() < 0.1 || (deg - 270.0).abs() < 0.1 {
                // Side on: one above the other, sharing a vertical axis. Both
                // balls, or a tip drifts off the base it belongs to.
                for (k, name) in [(0usize, "bases"), (1, "tips")] {
                    let (a, b) = if k == 0 { (pts[0].0, pts[1].0) } else { (pts[0].1, pts[1].1) };
                    assert!(
                        (a.0 - b.0).abs() < 0.5,
                        "at {deg} degrees the ear {name} differ in x by {:.1} px",
                        (a.0 - b.0).abs()
                    );
                }
            }
        }
    }

    #[test]
    fn the_ear_balls_sit_where_they_were_tuned() {
        // The ears are frozen: bases at `EAR_*` off the skull's centre, tips at
        // `EAR_TIP_*` off their bases. This pins the numbers that were tuned by
        // keyboard, so a stray edit to one of them shows up here.
        let (mut doll, c, rest, w) = rig();
        for _ in 0..30 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        // One placement with no step after it, so nothing has drifted since.
        w.pose(&mut doll, &c, &rest);
        let sk = doll.particles[c.skull].pos;
        let apart = 1.0 - EAR_TIP_SINK;
        for &(base, tip, s) in &c.ears {
            let b = doll.particles[base].pos;
            let t = doll.particles[tip].pos;
            assert!((b.x - (sk.x - EAR_BACK)).abs() < 0.01, "base x {:.2}", b.x - sk.x);
            assert!((b.y - (sk.y - EAR_UP)).abs() < 0.01, "base y {:.2}", b.y - sk.y);
            assert!(
                (doll.particles[base].side - s * EAR_SEP).abs() < 0.001,
                "base side {:.2}",
                doll.particles[base].side
            );
            assert!((doll.particles[base].radius - EAR_SIZE).abs() < 0.001, "base size");
            assert!(
                (t.x - (b.x - EAR_TIP_BACK * apart)).abs() < 0.01,
                "tip x {:.2}",
                t.x - b.x
            );
            assert!(
                (t.y - (b.y - EAR_TIP_UP * apart)).abs() < 0.01,
                "tip y {:.2}",
                t.y - b.y
            );
            assert!((doll.particles[tip].radius - EAR_TIP_SIZE).abs() < 0.001, "tip size");
        }
    }

    #[test]
    fn the_nearer_ear_is_drawn_in_front_at_ninety_degrees() {
        // Reported from play: at 90 degrees the far ear covered the near one.
        // Both ears were being drawn at exactly the skull's depth, so the draw
        // order decided it. Two things have to hold: the projection puts the
        // far-side ear nearer at 90 and the other one nearer at 270, and the
        // bias keeps that difference instead of flattening it - while staying
        // in front of the skull's rim.
        let view = View::new(25.0, GROUND);
        for (deg, nearer_is_second) in [(90.0f32, true), (270.0, false)] {
            let (mut doll, c, rest, mut w) = rig();
            let rad = deg.to_radians();
            w.body.heading = rad;
            w.heading_body = rad;
            for f in w.facing.iter_mut() {
                *f = rad;
            }
            for _ in 0..80 {
                w.pose(&mut doll, &c, &rest);
                doll.step(1.0 / 120.0);
            }
            w.pose(&mut doll, &c, &rest);
            let depth = |i: usize| {
                let p = &doll.particles[i];
                let b = Body { pos: w.body.pos, heading: w.body.heading };
                view.project(b.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y)).depth
            };
            let sk = depth(c.skull);
            // The same helper the renderer uses, so the test cannot drift from
            // it. Plane order: base, tip, base, tip.
            let planes = ear_planes(sk, [
                depth(c.ears[0].0), depth(c.ears[0].1),
                depth(c.ears[1].0), depth(c.ears[1].1),
            ]);
            let (first, second) = (planes[0], planes[2]);
            let (near, far) = if nearer_is_second { (second, first) } else { (first, second) };
            assert!(
                near < far - 0.5,
                "at {deg} degrees the ears are {:.2} and {:.2} after the bias - no clear winner",
                first, second
            );
            // Still in front of the skull's rim, drawn at sk + 0.5.
            assert!(
                far < sk + 0.5,
                "at {deg} degrees an ear fell behind the head's rim: skull {sk:.2}, ears {first:.2} and {second:.2}"
            );
            // The front ear is pushed clear by more than a radius, so every
            // pixel of it - its texture and the rim behind that texture - wins
            // against every pixel of the back ear. Without the gap the two
            // surfaces interleave and the back ear's body shows through the
            // front one's texture as slivers.
            assert!(
                near + 10.0 < far,
                "at {deg} degrees the front ear ({near:.2}) is not clear of the back one ({far:.2})"
            );
        }
    }


    #[test]
    fn every_foot_carries_four_claws() {
        // Four claws a foot, side by side across the toe, each tangent to the
        // foot's sphere - any closer and the foot's own surface covers it, any
        // further and it floats. Black, tiny, and without an outline, because on
        // a 3 px ball the outline would be most of the ball.
        let (mut doll, c, rest, w) = rig();
        assert_eq!(c.legs.len(), 4, "four feet");
        for _ in 0..30 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        w.pose(&mut doll, &c, &rest);
        for leg in &c.legs {
            let foot = &doll.particles[leg.foot];
            let mut across: Vec<f32> = Vec::new();
            for &(claw, ahead, side_off) in &leg.claws {
                let claw = &doll.particles[claw];
                assert_eq!(claw.colour, DARK, "claws are black");
                assert!(claw.no_rim, "no outline on a claw");
                assert!((claw.radius - CLAW_RADIUS).abs() < 0.001, "claw radius");
                assert!(
                    (claw.pos.x - (foot.pos.x + ahead)).abs() < 0.01,
                    "claw x {:.2}",
                    claw.pos.x - foot.pos.x
                );
                assert!((claw.pos.y - (foot.pos.y + 1.0)).abs() < 0.01, "claw y");
                assert!(
                    (claw.side - (foot.side + side_off)).abs() < 0.001,
                    "claw stays on its own toe"
                );
                // Tangent in three dimensions, so the lateral offset counts: the
                // outer claws sit further back because they sit further out.
                let (dx, dy, dl) = (
                    claw.pos.x - foot.pos.x,
                    claw.pos.y - foot.pos.y,
                    claw.side - foot.side,
                );
                let gap = (dx * dx + dy * dy + dl * dl).sqrt() - (claw.radius + foot.radius);
                assert!(gap.abs() < 0.1, "claw is {gap:.2} from its foot");
                across.push(claw.side);
            }
            across.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(across.len(), 4, "four claws a foot");
            let spread = across[3] - across[0];
            assert!(spread > 2.0 * CLAW_RADIUS + 6.0, "claws are spread across the toe, not stacked: {spread:.1}");
        }
    }

    #[test]
    fn claws_ease_with_their_foot() {
        // Reported from play: the claws flew off the paws mid-turn and came
        // back. Each ball is projected with its own easing heading, and the
        // claws were left on the default while their feet were slower - so the
        // foot turned and the claw did not. They have to share the foot's rate.
        let (mut doll, c, rest, mut w) = rig();
        for leg in &c.legs {
            for &(claw, _, _) in &leg.claws {
                assert!(
                    (w.lag[claw] - w.lag[leg.foot]).abs() < 1e-6,
                    "claw eases at {:.2}, its foot at {:.2}",
                    w.lag[claw],
                    w.lag[leg.foot]
                );
            }
        }
        // Spin on the spot for a second, then check the headings still agree.
        for _ in 0..120 {
            w.advance(1.0 / 120.0, 0.0, 1.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        for leg in &c.legs {
            for &(claw, _, _) in &leg.claws {
                assert!(
                    (w.facing[claw] - w.facing[leg.foot]).abs() < 1e-6,
                    "claw faces {:.3} while its foot faces {:.3}",
                    w.facing[claw],
                    w.facing[leg.foot]
                );
            }
        }
    }


    #[test]
    fn the_hip_stays_welded_to_its_spine_ball_through_a_turn() {
        // Reported from play, three times, and it had three causes in a row: the
        // hip lagged on its own, then it was pinned to a fixed point in body
        // space while the spine swung away from it, then it kept the body's
        // heading while its spine ball eased. The hip is now *derived* at
        // projection time from the spine ball it hangs from - position and
        // heading both - so the weld is exact rather than approximate.
        let (mut doll, c, rest, mut w) = rig();
        for _ in 0..60 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let mut worst = vec![0.0f32; c.legs.len()];
        for dir in [-1.0f32, 1.0] {
            for _ in 0..180 {
                w.advance(1.0 / 120.0, 0.0, dir);
                w.pose(&mut doll, &c, &rest);
                doll.step(1.0 / 120.0);
                for (n, leg) in c.legs.iter().enumerate() {
                    // What the renderer draws is derived from the attach ball, so
                    // what can still go wrong is the solved position disagreeing
                    // with that derivation - and the leg then tears at the joint.
                    let a = &doll.particles[leg.attach];
                    let h = &doll.particles[leg.hip];
                    let drift = h.pos.sub(a.pos.add(leg.offset)).length();
                    worst[n] = worst[n].max(drift);
                }
            }
        }
        for (n, d) in worst.iter().enumerate() {
            assert!(*d < 2.5, "leg {n}'s hip drifts {d:.2} from its spine ball");
        }
    }




    #[test]
    fn pulling_an_ear_drags_the_head() {
        // The ears are pinned to the skull, so they hold their placement exactly
        // - but they are *linked* to it as well, which only shows when something
        // pulls them: a pinned end takes no correction, so the spring moves the
        // skull instead. Before the links existed an ear could be dragged
        // clean off the model and nothing followed it.
        let (mut doll, c, rest, mut w) = rig();
        for _ in 0..90 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let skull0 = doll.particles[c.skull].pos;
        let base = c.ears[0].0;
        let from = doll.particles[base].pos;
        // A grab suspends the drives; do the same here.
        doll.release_all();
        for _ in 0..30 {
            doll.drag_to(base, from.add(Vec2::new(-25.0, 0.0)));
            doll.step(1.0 / 120.0);
        }
        let moved = doll.particles[c.skull].pos.sub(skull0).length();
        assert!(moved > 3.0, "the head did not follow the ear: {moved:.2}");
        // And the link is in the constraint list, so the overlay shows it.
        let linked = doll
            .constraints
            .iter()
            .any(|k| (k.a == c.skull && k.b == base) || (k.a == base && k.b == c.skull));
        assert!(linked, "the ear is not linked to the skull");
    }


    #[test]
    fn nearer_balls_are_drawn_last() {
        // Painter's order, keyed on depth: the far side of the body must be
        // drawn before the near side, or a dog walking away shows its chest
        // through its back.
        let (doll, c, w) = run(1.0);
        let view = View::new(25.0, GROUND);
        let depths: Vec<f32> = c
            .legs
            .iter()
            .map(|l| {
                let p = &doll.particles[l.foot];
                view.project(w.body.to_world(p.pos.x - BODY_ORIGIN, p.side, GROUND - p.pos.y)).depth
            })
            .collect();
        let mut order: Vec<usize> = (0..4).collect();
        order.sort_by(|&a, &b| depths[b].partial_cmp(&depths[a]).unwrap());
        // Sorted first == drawn first == furthest away.
        assert!(depths[order[0]] >= depths[order[3]]);
        assert!(
            (depths[0] - depths[1]).abs() > 1.0,
            "left and right feet are at the same depth - side offsets are not reaching the projection"
        );
    }

    #[test]
    fn feet_are_on_the_floor_for_most_of_the_cycle() {
        // A foot is above the floor during swing; that is the point of a swing.
        // It must spend most of its time planted, and never fly higher than a
        // step.
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let mut planted = vec![0usize; c.legs.len()];
        let mut highest = vec![f32::MIN; c.legs.len()];
        let samples = 240;
        for _ in 0..samples {
            w.advance(dt, 1.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
            for (i, leg) in c.legs.iter().enumerate() {
                let y = doll.particles[leg.foot].pos.y;
                if (y - GROUND).abs() < 1.5 {
                    planted[i] += 1;
                }
                highest[i] = highest[i].max(GROUND - y);
            }
        }
        for i in 0..c.legs.len() {
            let share = planted[i] as f32 / samples as f32;
            assert!(share > 0.35, "leg {i} planted only {:.0}% of the time", share * 100.0);
            assert!(
                highest[i] < w.gait.spec.step_height + 6.0,
                "leg {i} lifted {:.1} px, step is {:.1}",
                highest[i],
                w.gait.spec.step_height
            );
        }
    }

    #[test]
    fn the_body_stays_up_and_keeps_its_height() {
        let (doll, c, _) = run(2.0);
        let spine_y: Vec<f32> = c.spine.iter().map(|&i| doll.particles[i].pos.y).collect();
        let last = spine_y.len() - 1;
        for (i, y) in spine_y.iter().enumerate() {
            // The neck is lifted onto the head's axis, so it sits higher than
            // the rest of the spine by design.
            let rest = if i == last { GROUND - 84.0 } else { GROUND - 68.0 };
            assert!(
                (*y - rest).abs() < 14.0,
                "spine ball {i} drifted to y={y:.1}, expected about {rest:.0}"
            );
        }
    }

    #[test]
    fn the_legs_actually_cycle() {
        // A foot's horizontal offset from its hip should sweep through a range,
        // not sit still - that is the difference between walking and sliding.
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for _ in 0..240 {
            w.advance(dt, 1.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
            let leg = &c.legs[0];
            let off = doll.particles[leg.foot].pos.x - doll.particles[leg.hip].pos.x;
            lo = lo.min(off);
            hi = hi.max(off);
        }
        assert!(hi - lo > 20.0, "front foot only swept {:.1} px", hi - lo);
    }

    #[test]
    fn the_tail_root_holds_high_on_the_rump_and_the_rest_hangs_free() {
        // Settled shape, after several attempts: one held ball and a free chain.
        // The root sits high on the rump, above the line of the spine, the way a
        // cat's tail leaves the body; every ball past it carries no target, so
        // the chain dangles and can swing right round the root. Guards, in order
        // of how many times each one bit: a driven chain (a rigid rod), a slack
        // root (the tail drops under the cat), and a chain damped like the body,
        // which swings out and snaps back and reads as stiff.
        let (mut doll, c, rest, mut w) = rig();
        for _ in 0..90 {
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let root = c.tail[0];
        assert!(doll.particles[root].pinned, "the tail root is not pinned to the rump");
        assert!(
            doll.particles[root].target.is_none(),
            "the tail root is still driven somewhere"
        );
        for &i in &c.tail[1..] {
            assert!(
                doll.particles[i].target.is_none(),
                "tail ball {i} should hang free"
            );
            assert!(doll.particles[i].pos.y > 0.0 && doll.particles[i].pos.y < 480.0);
        }
        let rear = doll.particles[c.spine[0]].pos;
        let root_pos = doll.particles[root].pos;
        let tip = doll.particles[*c.tail.last().unwrap()].pos;
        assert!(
            root_pos.y < rear.y - 5.0,
            "the tail root is not above the body axis"
        );
        assert!(root_pos.x < rear.x, "the tail root is not behind the rump");
        assert!(
            tip.y > root_pos.y + 30.0,
            "the chain does not hang: root {:.0}, tip {:.0}",
            root_pos.y,
            tip.y
        );
        // It keeps moving after the turn stops. Damped like the body it snapped
        // back and died within a quarter second.
        let angle = |doll: &Ragdoll| {
            let tip = doll.particles[*c.tail.last().unwrap()].pos;
            let d = tip.sub(doll.particles[c.tail[0]].pos);
            d.x.atan2(d.y)
        };
        for _ in 0..120 {
            w.advance(1.0 / 120.0, 0.0, 1.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
        }
        let mut last = angle(&doll);
        let mut travel = 0.0f32;
        for _ in 0..180 {
            w.advance(1.0 / 120.0, 0.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(1.0 / 120.0);
            let a = angle(&doll);
            travel += (a - last).abs();
            last = a;
        }
        assert!(
            travel.to_degrees() > 20.0,
            "the tail stops dead once the turn ends: {:.1} degrees in 1.5 s",
            travel.to_degrees()
        );
    }

    #[test]
    fn the_mouth_is_painted_on_the_half_of_the_snout_that_faces_the_viewer() {
        // The mouth is a marking on the front of the snout, not a ball: it is a
        // disc of the muzzle's own surface. Two things follow, and both are
        // asserted here because both are easy to get wrong silently.
        let (r, d) = (6.5f32, MOUTH_DROP);
        // Facing the camera, it sits under the middle of the muzzle.
        let (x, y) = mouth_offset(std::f32::consts::PI, r, d).expect("no mouth facing the camera");
        assert!(x.abs() < 0.01, "the mouth is off-centre facing the camera: x {x}");
        assert!(y > 0.0 && y < r, "the mouth is not below the centre: y {y}");
        // In profile it has slid to the front of the snout. With no drop that is
        // the rim exactly; with the default drop it is a little short of it,
        // because the marking is on the sphere and not on its equator - which is
        // the point of dividing by the direction's length.
        let (x, y) = mouth_offset(std::f32::consts::FRAC_PI_2, r, 0.0).expect("no mouth in profile");
        assert!((x - r).abs() < 1e-4 && y.abs() < 1e-4, "level profile mouth is at ({x}, {y})");
        let (x, _) = mouth_offset(std::f32::consts::FRAC_PI_2, r, d).expect("no mouth in profile");
        assert!(x > 0.5 * r && x < r, "the mouth has not moved to the front of the snout: x {x}");
        let (x, _) = mouth_offset(-std::f32::consts::FRAC_PI_2, r, d).expect("no mouth in profile");
        assert!(x < -0.5 * r && x > -r, "the mouth has not moved to the front on the other side: x {x}");
        // Facing away, there is nothing to paint: a disc takes the near surface
        // for its depth, so drawing it would put it on the back of the head.
        assert!(mouth_offset(0.0, r, d).is_none(), "the mouth shows through the skull");
        // And the centre stays on the sphere at any drop, rather than floating
        // off it as the marking is moved up or down the snout.
        for drop in [0.0f32, 0.45, 1.0, 2.0, -0.8] {
            let (x, y) = mouth_offset(std::f32::consts::PI, 1.0, drop).unwrap();
            assert!(
                (x * x + y * y).sqrt() <= 1.0 + 1e-5,
                "drop {drop} puts the mouth off the sphere: ({x}, {y})"
            );
        }
    }

    #[test]
    fn the_tail_root_cannot_come_off_the_rump() {
        // Ball 43 grows out of the butt ball. Driven it lagged the rump whenever
        // the rump moved and the joint opened - 6.1 units in a walking step with
        // the sway pulling it about, which is visible as a tail hovering behind
        // the cat. Placed from the butt ball instead, it holds one offset for
        // good, through turning and walking both.
        let (mut doll, c, rest, mut w) = rig();
        let dt = 1.0 / 120.0;
        let (root, butt) = (c.tail[0], c.spine[0]);
        for _ in 0..600 {
            w.advance(dt, 0.0, 0.0);
            w.pose(&mut doll, &c, &rest);
            doll.step(dt);
        }
        let off = doll.particles[root].pos.sub(doll.particles[butt].pos);
        // Two different claims, tested apart. The pose puts the root exactly on
        // the butt plus its offset, every time - no easing, no lag. After the
        // step the butt can have moved by up to one step's worth of its own
        // motion, and the root is corrected at the next pose; that residual is
        // real but sub-pixel, so it is bounded rather than required to be zero.
        let mut placed_err = 0.0f32;
        let mut step_err = 0.0f32;
        for (moving, turning) in [(0.0, 1.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)] {
            for _ in 0..240 {
                w.advance(dt, moving, turning);
                w.pose(&mut doll, &c, &rest);
                let now = doll.particles[root].pos.sub(doll.particles[butt].pos);
                placed_err = placed_err.max(now.sub(off).length());
                doll.step(dt);
                let after = doll.particles[root].pos.sub(doll.particles[butt].pos);
                step_err = step_err.max(after.sub(off).length());
            }
        }
        assert!(
            placed_err < 1e-3,
            "the pose does not place the root on the rump: {placed_err:.3} units off"
        );
        // 1.5 body units is three quarters of a surface pixel at the default
        // zoom: past the eye, and it is corrected on the next frame.
        assert!(
            step_err < 1.5,
            "the root lags the rump by {step_err:.2} units after a step"
        );
    }


}
