//! The ragdoll solver.
//!
//! Petz does not animate its pets. It *simulates* them: the body is a cloud of
//! spheres joined by springs, and every frame the solver moves them and the
//! rasterizer redraws each one. The tell is behavioural, not structural - pick
//! a pet up by the scruff and the balls hang, swing and settle. That is a
//! damped spring network under gravity, not a clip.
//!
//! This is the standard Verlet formulation (Jakobsen, GDC 2001; formalised as
//! position-based dynamics by Müller et al. 2007). Velocity is implicit in the
//! difference between the current and previous position, so there is no
//! integration state to drift; constraints are satisfied by moving positions
//! directly, which is unconditionally stable at the stiffnesses a ragdoll
//! needs.
//!
//! Deliberately no dependencies and no floating-point of the exotic kind: a
//! fixed timestep and plain `f32` keep a replay reproducible, which is what
//! lets us later fit these parameters against captured trajectories.
//!
//! Note the division of duty, because it matters for how exact this has to be:
//! *rendering* must be bit-exact, and is proven elsewhere. Motion only has to
//! be indistinguishable, so the parameters here are meant to be fitted, not
//! guessed.

/// A 2D position / velocity. Small enough to keep by value.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    #[inline]
    pub fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }

    #[inline]
    pub fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

/// One ball of the body. Position is the sphere's centre; `radius` is what the
/// renderer turns into a table lookup later.
#[derive(Clone, Debug)]
pub struct Particle {
    pub pos: Vec2,
    pub prev: Vec2,
    pub radius: f32,
    pub colour: u8,
    /// Pinned particles ignore gravity and constraints. Grabbing a pet pins
    /// the particle under the cursor; that one pin is the whole grab mechanic.
    pub pinned: bool,
    /// Where an animator wants this ball, if anything. A gait drives its feet
    /// and spine this way; the tail and ears are left free so they swing.
    pub target: Option<Vec2>,
    /// How hard to chase `target`, 0..=1 per step.
    pub follow: f32,
    /// Lateral offset from the body's midline, in body units. The solver works
    /// in the animal's sagittal plane, so this is authored rather than
    /// simulated: it is what puts a left leg to the left of a right one.
    pub side: f32,
    /// The authored lateral offset, so a drag can be undone. Side is not
    /// simulated, so nothing else would ever pull a dragged ball back.
    pub rest_side: f32,
    /// Colour of the rim drawn around this ball. The retail data carries a
    /// per-ball outline colour, and the eye uses it for the iris ring.
    pub rim: u8,
    /// Render layer. 0 is the body, drawn in depth order; higher layers are
    /// drawn afterwards. The eye iris needs this: it must sit on its own
    /// sclera whatever the depth says.
    pub layer: u8,
}

impl Particle {
    pub fn new(x: f32, y: f32, radius: f32, colour: u8) -> Self {
        Self {
            pos: Vec2::new(x, y),
            prev: Vec2::new(x, y),
            radius,
            colour,
            pinned: false,
            target: None,
            follow: 0.0,
            side: 0.0,
            rest_side: 0.0,
            rim: 0,
            layer: 0,
        }
    }

    /// Implicit Verlet velocity, for grabbing and for anything that needs to
    /// know how fast a ball is travelling.
    pub fn velocity(&self) -> Vec2 {
        self.pos.sub(self.prev)
    }
}

/// A spring between two particles. `rest` is the length it wants to be;
/// `stiffness` in 0..=1 says how hard it argues for that length per iteration.
#[derive(Clone, Copy, Debug)]
pub struct Constraint {
    pub a: usize,
    pub b: usize,
    pub rest: f32,
    pub stiffness: f32,
}

impl Constraint {
    pub fn new(a: usize, b: usize, rest: f32) -> Self {
        Self { a, b, rest, stiffness: 1.0 }
    }

    pub fn soft(a: usize, b: usize, rest: f32, stiffness: f32) -> Self {
        Self { a, b, rest, stiffness }
    }
}

/// The simulation world. One instance per pet; toys and props get their own.
#[derive(Clone, Debug)]
pub struct Ragdoll {
    pub particles: Vec<Particle>,
    pub constraints: Vec<Constraint>,
    /// Pixels per second squared. The original's world is in 640x480 units.
    pub gravity: f32,
    /// Velocity retained per step. 1.0 is frictionless; 0.98 is visibly damped.
    pub damping: f32,
    /// Constraint passes per step. More passes is stiffer, not more accurate.
    pub iterations: u32,
    /// Walls. Particles are pushed inside this box.
    pub bounds: Option<(f32, f32, f32, f32)>,
    /// How hard two spheres push apart when they overlap.
    pub contact: f32,
}

impl Ragdoll {
    pub fn new() -> Self {
        Self {
            particles: Vec::new(),
            constraints: Vec::new(),
            gravity: 1400.0,
            damping: 0.98,
            iterations: 8,
            bounds: None,
            contact: 1.0,
        }
    }

    pub fn add(&mut self, p: Particle) -> usize {
        self.particles.push(p);
        self.particles.len() - 1
    }

    /// Joins two particles, deriving the rest length from where they are now.
    /// Building a body by placing balls and then linking them is how the pose
    /// is authored; the link holds whatever you laid out.
    pub fn link(&mut self, a: usize, b: usize) -> usize {
        let rest = self.particles[a].pos.sub(self.particles[b].pos).length();
        self.constraints.push(Constraint::new(a, b, rest));
        self.constraints.len() - 1
    }

    /// Advance one fixed step of `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        let dt2 = dt * dt;

        // 1. Integrate. Pinned particles are immovable, so they carry no
        //    velocity and are skipped entirely.
        for p in self.particles.iter_mut() {
            if p.pinned {
                continue;
            }
            let vx = (p.pos.x - p.prev.x) * self.damping;
            let vy = (p.pos.y - p.prev.y) * self.damping;
            p.prev = p.pos;
            p.pos.x += vx;
            p.pos.y += vy + self.gravity * dt2;
        }

        // 2. Satisfy constraints. Relaxation, not a linear solve: each pass
        //    nudges every spring toward its rest length and later passes clean
        //    up the error earlier ones introduced.
        for _ in 0..self.iterations {
            self.solve_constraints();
        }

        // Driven balls move last, so the pose wins over the springs. Driving
        // before relaxation means every constraint pass drags the target back
        // off its mark, and a planted foot ends up sliding.
        self.drive();

        // 3. Resolve contacts and walls after the skeleton is coherent.
        if self.contact > 0.0 {
            self.solve_contacts();
        }
        if let Some((x0, y0, x1, y1)) = self.bounds {
            self.solve_walls(x0, y0, x1, y1);
        }
    }

    /// Move driven particles a fraction of the way toward their targets.
    fn drive(&mut self) {
        for p in self.particles.iter_mut() {
            if p.pinned {
                continue;
            }
            if let Some(t) = p.target {
                p.pos.x += (t.x - p.pos.x) * p.follow;
                p.pos.y += (t.y - p.pos.y) * p.follow;
            }
        }
    }

    /// Point a ball at a place and a stiffness. `follow` of 1 snaps it there.
    pub fn drive_to(&mut self, index: usize, target: Vec2, follow: f32) {
        if let Some(p) = self.particles.get_mut(index) {
            p.target = Some(target);
            p.follow = follow.clamp(0.0, 1.0);
        }
    }

    /// Put every ball back on its authored lateral offset. Dragging changes
    /// side directly, and the solver cannot undo that.
    pub fn restore_sides(&mut self) {
        for p in self.particles.iter_mut() {
            p.side = p.rest_side;
        }
    }

    /// Stop driving everything - what grabbing a pet should do.
    pub fn release_all(&mut self) {
        for p in self.particles.iter_mut() {
            p.target = None;
            p.follow = 0.0;
        }
    }

    fn solve_constraints(&mut self) {
        for c in &self.constraints {
            let (a, b) = (c.a, c.b);
            let pa = self.particles[a].pos;
            let pb = self.particles[b].pos;
            let d = pb.sub(pa);
            let len = d.length();
            if len < f32::EPSILON {
                continue;
            }
            // How far each end must move to reach `rest`, split by mobility.
            let diff = (len - c.rest) / len * 0.5 * c.stiffness;
            let (ma, mb) = (
                !self.particles[a].pinned,
                !self.particles[b].pinned,
            );
            match (ma, mb) {
                (true, true) => {
                    self.particles[a].pos.x += d.x * diff;
                    self.particles[a].pos.y += d.y * diff;
                    self.particles[b].pos.x -= d.x * diff;
                    self.particles[b].pos.y -= d.y * diff;
                }
                // A pinned end takes the whole correction, or the spring
                // would pull against a wall that cannot move and go slack.
                (true, false) => {
                    self.particles[a].pos.x += d.x * diff * 2.0;
                    self.particles[a].pos.y += d.y * diff * 2.0;
                }
                (false, true) => {
                    self.particles[b].pos.x -= d.x * diff * 2.0;
                    self.particles[b].pos.y -= d.y * diff * 2.0;
                }
                (false, false) => {}
            }
        }
    }

    /// True if a spring joins these two. Contacts must ignore such pairs:
    /// a body is *meant* to have its own balls interpenetrate, and letting a
    /// contact push apart a pair that a bone is holding together just makes
    /// the two solvers fight, which is what turns a body into a bead chain.
    fn linked(&self, i: usize, j: usize) -> bool {
        self.constraints
            .iter()
            .any(|c| (c.a == i && c.b == j) || (c.a == j && c.b == i))
    }

    /// Spheres that overlap push each other apart. Deliberately gentle: the
    /// pet's balls are meant to interpenetrate, that is what makes a body read
    /// as one soft mass rather than a bag of marbles.
    fn solve_contacts(&mut self) {
        let n = self.particles.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.linked(i, j) {
                    continue;
                }
                let (pi, pj) = (self.particles[i].pos, self.particles[j].pos);
                let d = pj.sub(pi);
                let dist = d.length();
                let want = (self.particles[i].radius + self.particles[j].radius) * 0.55;
                if dist >= want || dist < f32::EPSILON {
                    continue;
                }
                let push = (want - dist) / dist * 0.5 * self.contact;
                if !self.particles[i].pinned {
                    self.particles[i].pos.x -= d.x * push;
                    self.particles[i].pos.y -= d.y * push;
                }
                if !self.particles[j].pinned {
                    self.particles[j].pos.x += d.x * push;
                    self.particles[j].pos.y += d.y * push;
                }
            }
        }
    }

    fn solve_walls(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        for p in self.particles.iter_mut() {
            if p.pinned {
                continue;
            }
            let r = p.radius;
            if p.pos.x - r < x0 { p.pos.x = x0 + r; }
            if p.pos.x + r > x1 { p.pos.x = x1 - r; }
            if p.pos.y - r < y0 { p.pos.y = y0 + r; }
            if p.pos.y + r > y1 {
                // Landing should bleed energy, or a dropped pet bounces forever.
                p.pos.y = y1 - r;
                p.prev.y = p.pos.y + (p.prev.y - p.pos.y) * 0.4;
            }
        }
    }

    /// Reset the rest length of the spring between two balls. Used when a part
    /// is driven to a new place: a link built at one length would otherwise
    /// drag the part back towards where it was authored.
    pub fn set_rest(&mut self, a: usize, b: usize, rest: f32) {
        for c in self.constraints.iter_mut() {
            if (c.a == a && c.b == b) || (c.a == b && c.b == a) {
                c.rest = rest;
            }
        }
    }

    /// Move a pinned particle to a point, carrying its stored position so the
    /// released velocity is the drag velocity. This is grab-and-throw.
    pub fn drag_to(&mut self, index: usize, target: Vec2) {
        if let Some(p) = self.particles.get_mut(index) {
            p.prev = p.pos;
            p.pos = target;
        }
    }

    /// The particle nearest to a point, within `radius`. What a click picks up.
    pub fn nearest(&self, point: Vec2, radius: f32) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, p) in self.particles.iter().enumerate() {
            let d = p.pos.sub(point).length();
            if d <= radius && best.map_or(true, |(_, bd)| d < bd) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }

    pub fn mean_error(&self) -> f32 {
        if self.constraints.is_empty() {
            return 0.0;
        }
        let total: f32 = self
            .constraints
            .iter()
            .map(|c| {
                let d = self.particles[c.a].pos.sub(self.particles[c.b].pos).length();
                (d - c.rest).abs()
            })
            .sum();
        total / self.constraints.len() as f32
    }
}

impl Default for Ragdoll {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(n: usize) -> Ragdoll {
        let mut r = Ragdoll::new();
        for i in 0..n {
            r.add(Particle::new(100.0, 100.0 + i as f32 * 20.0, 6.0, 10));
        }
        for i in 0..n - 1 {
            r.link(i, i + 1);
        }
        r.particles[0].pinned = true;
        r
    }

    #[test]
    fn a_pinned_particle_does_not_move() {
        let mut r = chain(4);
        for _ in 0..200 {
            r.step(1.0 / 60.0);
        }
        assert_eq!(r.particles[0].pos, Vec2::new(100.0, 100.0));
    }

    #[test]
    fn a_hanging_chain_keeps_its_lengths() {
        let mut r = chain(6);
        for _ in 0..400 {
            r.step(1.0 / 60.0);
        }
        assert!(
            r.mean_error() < 0.5,
            "constraints stretched by {} px on average",
            r.mean_error()
        );
    }

    #[test]
    fn a_chain_hangs_below_its_pin() {
        let mut r = chain(6);
        for _ in 0..400 {
            r.step(1.0 / 60.0);
        }
        let top = r.particles[0].pos;
        let bottom = r.particles[5].pos;
        assert!(bottom.y > top.y + 80.0, "chain did not hang: {bottom:?}");
        // Damping must take the swing out, or a dropped pet never settles.
        let v = r.particles[5].velocity();
        assert!(v.length() < 0.5, "still moving at {:?}", v.length());
    }

    #[test]
    fn a_wall_stops_a_falling_body() {
        let mut r = Ragdoll::new();
        let i = r.add(Particle::new(50.0, 10.0, 5.0, 10));
        r.particles[i].pinned = false;
        r.bounds = Some((0.0, 0.0, 100.0, 100.0));
        for _ in 0..200 {
            r.step(1.0 / 60.0);
        }
        assert!(r.particles[i].pos.y <= 95.5, "fell through the floor");
        assert!(r.particles[i].pos.y > 90.0, "never reached the floor");
    }

    #[test]
    fn dragging_moves_only_the_dragged_particle_first() {
        let mut r = chain(4);
        for _ in 0..30 {
            r.step(1.0 / 60.0);
        }
        let target = Vec2::new(160.0, 90.0);
        r.drag_to(3, target);
        assert_eq!(r.particles[3].pos, target);
        for _ in 0..60 {
            r.step(1.0 / 60.0);
        }
        assert!(r.mean_error() < 0.5, "springs did not recover after a drag");
    }

    #[test]
    fn contacts_do_not_fight_a_bone() {
        // Two spheres far more overlapped than their radius sum, held at that
        // distance by a stiff spring. The contact pass must leave them alone.
        let mut r = Ragdoll::new();
        let a = r.add(Particle::new(100.0, 100.0, 20.0, 10));
        let b = r.add(Particle::new(110.0, 100.0, 20.0, 10));
        r.link(a, b);
        r.gravity = 0.0;
        r.contact = 1.0;
        for _ in 0..30 {
            r.step(1.0 / 60.0);
        }
        let d = r.particles[a].pos.sub(r.particles[b].pos).length();
        assert!((d - 10.0).abs() < 0.5, "contact shoved a linked pair to {d}");
    }

    #[test]
    fn the_solver_is_deterministic() {
        let run = || {
            let mut r = chain(8);
            r.bounds = Some((0.0, 0.0, 320.0, 480.0));
            for _ in 0..300 {
                r.step(1.0 / 60.0);
            }
            r.particles.iter().map(|p| p.pos.x.to_bits()).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;

    /// A stand-in for a pet torso: five balls, heavily overlapping, all linked.
    /// This is the property the sandbox has to satisfy - when one ball is held,
    /// the body must stay one connected mass, not trail apart into beads.
    fn torso() -> Ragdoll {
        let mut r = Ragdoll::new();
        let radii = [24.0, 23.0, 22.0, 21.0, 20.0];
        for (i, rad) in radii.iter().enumerate() {
            r.add(Particle::new(200.0 + i as f32 * 26.0, 150.0, *rad, 61));
        }
        for i in 0..4 {
            r.link(i, i + 1);
        }
        r.particles[2].pinned = true;
        r
    }

    fn max_gap(r: &Ragdoll) -> f32 {
        // Largest distance between neighbouring sphere surfaces along the
        // links. Positive means a visible gap in the silhouette.
        r.constraints
            .iter()
            .map(|c| {
                let d = r.particles[c.a].pos.sub(r.particles[c.b].pos).length();
                d - (r.particles[c.a].radius + r.particles[c.b].radius)
            })
            .fold(f32::MIN, f32::max)
    }

    #[test]
    fn a_held_body_stays_one_piece() {
        let mut r = torso();
        for _ in 0..240 {
            r.step(1.0 / 120.0);
        }
        assert!(
            max_gap(&r) < 0.0,
            "balls separated by {} px while hanging",
            max_gap(&r)
        );
    }

    #[test]
    fn a_held_body_does_not_stretch() {
        let before: f32 = (0..4)
            .map(|i| r_dist(&torso(), i))
            .sum();
        let mut r = torso();
        for _ in 0..240 {
            r.step(1.0 / 120.0);
        }
        let after: f32 = (0..4).map(|i| {
            r.particles[i].pos.sub(r.particles[i + 1].pos).length()
        }).sum();
        assert!(
            (after - before).abs() < 2.0,
            "spine changed length {before} -> {after}"
        );
    }

    fn r_dist(r: &Ragdoll, i: usize) -> f32 {
        r.particles[i].pos.sub(r.particles[i + 1].pos).length()
    }
}
