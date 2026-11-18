//! Level-up feedback for the voxel pets, as plain data: given what is
//! happening (absorbing growth, a level, a milestone, a form change, the last
//! level) and how far along it is, say where the energy motes, ground rings,
//! light column and voxel confetti are, how brightly the pet glows, and how
//! its body hops, squashes and turns.
//!
//! Five tiers, each clearly stronger than the one before:
//! - absorb: growth flows in, a soft glow and a small gulp; no ring, no burst,
//!   so a partial top-up never looks like a level;
//! - level: motes, a crisp hop with squash and stretch, one ring, a few sparks;
//! - milestone (level 40, same form): a light column, a big hop, two rings,
//!   voxel confetti and a slight camera push;
//! - form change (crack, hatch, grow): the existing transformation, charged
//!   with motes, glow and a light column, then a short landing (rings,
//!   confetti) once the new form has settled;
//! - max (level 60): spiralling gold motes, a column, one slow turn in the air,
//!   a confetti fountain and three gold rings, then stillness.
//!
//! Reduced motion keeps only a slow, steady glow and the real progress.

use std::f64::consts::PI;

use crate::pet_models::smooth;

/// Which tier is playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Absorb,
    Level,
    Milestone,
    Max,
    Crack,
    Hatch,
    Grow,
    /// The landing after a form change: rings, confetti and the glow settling.
    Land,
}

/// The beats of one tier at local time `t` (0..1 of its span).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Beats {
    /// Motes travel from the source to the pet in this window.
    pub motes: Option<(f64, f64)>,
    /// How many motes, and whether they spiral (max level).
    pub mote_count: u32,
    pub spiral: bool,
    /// The moment of impact: rings, sparks and confetti start here.
    pub impact: Option<f64>,
    pub rings: u32,
    pub confetti: u32,
    pub gold: bool,
    /// Body motion, in pet units.
    pub hop: f64,
    pub squash: f64,
    pub spin: f64,
    /// 0..1 glow on the pet (rim light and a lift).
    pub glow: f64,
    /// 0..1 light column.
    pub beam: f64,
    /// Fraction the camera pushes in.
    pub punch: f64,
}

fn bump(t: f64, at: f64, width: f64) -> f64 {
    (-((t - at) / width).powi(2)).exp()
}

fn arc(t: f64, from: f64, to: f64) -> f64 {
    let u = ((t - from) / (to - from)).clamp(0.0, 1.0);
    (u * PI).sin()
}

/// The beats of `tier` at local time `t`.
pub fn beats(tier: Tier, t: f64, reduced: bool) -> Beats {
    let t = t.clamp(0.0, 1.0);
    if reduced {
        // A slow, steady glow and nothing that moves, flashes or flies.
        let peak = match tier {
            Tier::Absorb => 0.25,
            Tier::Level => 0.4,
            _ => 0.6,
        };
        return Beats {
            glow: peak * (t * PI).sin(),
            gold: matches!(tier, Tier::Max | Tier::Milestone),
            squash: 1.0,
            ..Default::default()
        };
    }
    let mut b = Beats {
        squash: 1.0,
        ..Default::default()
    };
    match tier {
        Tier::Absorb => {
            b.motes = Some((0.0, 0.8));
            b.mote_count = 10;
            b.glow = 0.35 * smooth(0.3, 0.8, t) * (1.0 - smooth(0.85, 1.0, t));
            // A small gulp as the last motes arrive.
            b.squash = 1.0 - 0.035 * arc(t, 0.72, 1.0);
        }
        Tier::Level => {
            let i = 0.42;
            b.motes = Some((0.0, i));
            b.mote_count = 12;
            b.impact = Some(i);
            b.rings = 1;
            b.hop = 0.9 * arc(t, i, i + 0.42);
            // Crouch before the hop, stretch in the air, squash on landing.
            b.squash = 1.0 - 0.09 * arc(t, i - 0.12, i) + 0.06 * arc(t, i, i + 0.2)
                - 0.06 * arc(t, i + 0.4, i + 0.55);
            b.glow = 0.25 * smooth(0.1, i, t) * (1.0 - smooth(0.6, 0.95, t))
                + 0.55 * bump(t, i + 0.04, 0.09) * f64::from(t > i - 0.1);
        }
        Tier::Milestone => {
            let i = 0.42;
            b.motes = Some((0.0, 0.32));
            b.mote_count = 18;
            b.impact = Some(i);
            b.rings = 2;
            b.confetti = 36;
            b.gold = true;
            b.hop = 1.5 * arc(t, i, i + 0.4);
            b.squash = 1.0 - 0.12 * arc(t, i - 0.14, i) + 0.08 * arc(t, i, i + 0.2)
                - 0.07 * arc(t, i + 0.38, i + 0.52);
            b.beam = smooth(0.12, 0.38, t) * (1.0 - smooth(0.55, 0.9, t));
            b.glow = 0.4 * smooth(0.1, i, t) * (1.0 - smooth(0.6, 0.95, t))
                + 0.6 * bump(t, i + 0.04, 0.1);
            b.punch = 0.05 * bump(t, i + 0.05, 0.12);
        }
        Tier::Max => {
            let i = 0.62;
            b.motes = Some((0.0, 0.32));
            b.mote_count = 28;
            b.spiral = true;
            b.impact = Some(i);
            b.rings = 3;
            b.confetti = 60;
            b.gold = true;
            // Rise, one slow turn, and a soft landing.
            b.hop = 2.2 * smooth(0.22, 0.45, t) * (1.0 - smooth(0.55, i, t));
            b.spin = 2.0 * PI * smooth(0.3, 0.58, t);
            b.squash = 1.0 - 0.1 * arc(t, i - 0.02, i + 0.14);
            b.beam = smooth(0.15, 0.4, t) * (1.0 - smooth(0.7, 0.95, t));
            b.glow = 0.55 * smooth(0.1, i, t) * (1.0 - smooth(i + 0.05, 0.97, t))
                + 0.4 * bump(t, i, 0.06);
            b.punch = 0.06 * bump(t, i + 0.03, 0.1);
        }
        Tier::Crack => {
            b.motes = Some((0.0, 0.3));
            b.mote_count = 14;
            b.glow = 0.5 * smooth(0.2, 0.74, t) * (1.0 - smooth(0.8, 1.0, t));
        }
        Tier::Hatch => {
            b.motes = Some((0.0, 0.18));
            b.mote_count = 16;
            b.beam = smooth(0.05, 0.3, t) * (1.0 - smooth(0.5, 0.8, t));
            b.glow = 0.5 * smooth(0.1, 0.5, t) * (1.0 - smooth(0.75, 1.0, t));
        }
        Tier::Grow => {
            b.motes = Some((0.0, 0.18));
            b.mote_count = 16;
            b.beam = smooth(0.08, 0.35, t) * (1.0 - smooth(0.7, 0.95, t));
            b.glow = 0.55 * smooth(0.1, 0.5, t) * (1.0 - smooth(0.8, 1.0, t));
        }
        Tier::Land => {
            b.impact = Some(0.0);
            b.rings = 2;
            b.confetti = 30;
            b.glow = 0.6 * bump(t, 0.04, 0.14);
            b.punch = 0.04 * bump(t, 0.06, 0.12);
            b.squash = 1.0 - 0.05 * arc(t, 0.0, 0.25);
        }
    }
    b
}

/// A small deterministic hash for particle layouts.
fn hash(i: u32, salt: u32) -> f64 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f64 / 65535.0
}

/// Where the pet is on screen, in world units.
#[derive(Clone, Copy, Debug)]
pub struct Stage {
    pub centre: [f64; 3],
    pub floor: f64,
    pub height: f64,
    pub width: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mote {
    pub pos: [f64; 3],
    pub size: f64,
    pub alpha: f64,
    /// 0 orange, 1 gold.
    pub gold: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub radius: f64,
    pub alpha: f64,
    pub gold: bool,
}

/// One confetti cube: centre, size, palette slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bit {
    pub pos: [f64; 3],
    pub size: f64,
    pub slot: u8,
}

/// Motes flying from below the front of the stage into the pet, with a short
/// trail behind each (drawn as smaller, fainter copies).
pub fn motes(b: &Beats, t: f64, s: &Stage) -> Vec<Mote> {
    let Some((from, to)) = b.motes else {
        return Vec::new();
    };
    let span = to - from;
    let mut out = Vec::new();
    for i in 0..b.mote_count {
        let delay = hash(i, 3) * 0.45;
        let travel = 0.55;
        let local = ((t - from) / span - delay) / travel;
        if !(0.0..1.0).contains(&local) {
            continue;
        }
        let side = (hash(i, 5) - 0.5) * s.width * 1.6;
        let start = [side, s.floor - s.height * 0.35, 6.0 + hash(i, 7) * 4.0];
        let end = [
            s.centre[0] + (hash(i, 9) - 0.5) * s.width * 0.2,
            s.centre[1],
            s.centre[2] + 2.0,
        ];
        for trail in 0..3 {
            let u = (local - trail as f64 * 0.05).clamp(0.0, 1.0);
            let e = u * u * (3.0 - 2.0 * u);
            let mut p: [f64; 3] = std::array::from_fn(|k| start[k] + (end[k] - start[k]) * e);
            // Bow outwards on the way, or swirl round the pet at max level.
            let bow = (u * PI).sin();
            if b.spiral {
                let a = hash(i, 11) * 2.0 * PI + u * 3.0 * PI;
                p[0] += a.cos() * s.width * 0.6 * bow;
                p[2] += a.sin() * s.width * 0.6 * bow;
            } else {
                p[0] += side.signum() * s.width * 0.3 * bow;
            }
            p[1] += s.height * 0.25 * bow;
            out.push(Mote {
                pos: p,
                size: (0.55 + hash(i, 13) * 0.35) * (1.0 - trail as f64 * 0.28),
                alpha: (1.0 - (local - 0.85).max(0.0) / 0.15) * (1.0 - trail as f64 * 0.35),
                gold: b.gold && i % 3 != 0,
            });
        }
    }
    out
}

/// Ground rings expanding from the pet's feet after the impact.
pub fn rings(b: &Beats, t: f64, s: &Stage) -> Vec<Ring> {
    let Some(i) = b.impact else {
        return Vec::new();
    };
    (0..b.rings)
        .filter_map(|k| {
            let start = i + k as f64 * 0.07;
            let age = (t - start) / 0.45f64.min(1.0 - start);
            (0.0..1.0).contains(&age).then(|| Ring {
                radius: s.width * (0.35 + (0.65 + 0.15 * k as f64) * (1.0 - (1.0 - age).powi(3))),
                alpha: (1.0 - age).powf(1.5),
                gold: b.gold || k == 1,
            })
        })
        .collect()
}

/// Voxel confetti bursting from the pet and settling on the floor.
/// `slots` are the palette slots to colour them with.
pub fn confetti(b: &Beats, t: f64, s: &Stage, slots: [u8; 3]) -> Vec<Bit> {
    let Some(i) = b.impact else {
        return Vec::new();
    };
    let age = (t - i) / (1.0 - i).max(0.2);
    if b.confetti == 0 || !(0.0..1.0).contains(&age) {
        return Vec::new();
    }
    let seconds = age * 1.4;
    (0..b.confetti)
        .map(|k| {
            let a = hash(k, 17) * 2.0 * PI;
            let out = s.width * (0.5 + hash(k, 19) * 0.9);
            let up = s.height * (0.9 + hash(k, 23) * 1.1);
            let mut pos = [
                s.centre[0] + a.cos() * out * seconds,
                s.centre[1] + up * seconds - 0.5 * 2.6 * s.height * seconds * seconds,
                s.centre[2] + a.sin() * out * seconds * 0.6 + 1.0,
            ];
            let size = (0.45 + hash(k, 29) * 0.45) * (1.0 - smooth(0.7, 1.0, age));
            pos[1] = pos[1].max(s.floor + size / 2.0);
            Bit {
                pos,
                size,
                slot: slots[(k % 3) as usize],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Stage = Stage {
        centre: [0.0, 0.0, 0.0],
        floor: -6.0,
        height: 12.0,
        width: 8.0,
    };

    #[test]
    fn a_partial_top_up_never_looks_like_a_level() {
        for i in 0..=100 {
            let b = beats(Tier::Absorb, i as f64 / 100.0, false);
            assert_eq!((b.impact, b.rings, b.confetti, b.beam), (None, 0, 0, 0.0));
            assert!(b.hop == 0.0 && b.glow < 0.4);
        }
    }

    #[test]
    fn each_tier_is_stronger_than_the_one_before() {
        let peak = |tier| {
            (0..=200)
                .map(|i| {
                    let b = beats(tier, i as f64 / 200.0, false);
                    b.hop + b.glow + b.beam + b.rings as f64 + b.confetti as f64 / 20.0
                })
                .fold(0.0, f64::max)
        };
        let order = [Tier::Absorb, Tier::Level, Tier::Milestone, Tier::Max];
        for w in order.windows(2) {
            assert!(
                peak(w[1]) > peak(w[0]),
                "{:?} is not stronger than {:?}",
                w[1],
                w[0]
            );
        }
    }

    #[test]
    fn every_effect_has_ended_when_the_span_does() {
        for tier in [
            Tier::Absorb,
            Tier::Level,
            Tier::Milestone,
            Tier::Max,
            Tier::Crack,
            Tier::Hatch,
            Tier::Grow,
            Tier::Land,
        ] {
            let b = beats(tier, 1.0, false);
            assert!(
                b.hop.abs() < 1e-6 && b.beam < 1e-3 && b.glow < 0.05,
                "{tier:?}: {b:?}"
            );
            assert!(
                (b.squash - 1.0).abs() < 1e-6,
                "{tier:?} squash {}",
                b.squash
            );
            assert!(motes(&b, 1.0, &STAGE).is_empty(), "{tier:?} motes");
            assert!(rings(&b, 1.0, &STAGE).is_empty(), "{tier:?} rings");
            assert!(
                confetti(&b, 1.0, &STAGE, [0, 1, 2]).is_empty(),
                "{tier:?} confetti"
            );
        }
    }

    #[test]
    fn reduced_motion_keeps_a_glow_and_drops_everything_that_moves() {
        for tier in [Tier::Level, Tier::Milestone, Tier::Max, Tier::Hatch] {
            for i in 0..=50 {
                let b = beats(tier, i as f64 / 50.0, true);
                assert_eq!(
                    (b.motes, b.impact, b.hop, b.spin, b.beam),
                    (None, None, 0.0, 0.0, 0.0)
                );
            }
            assert!(beats(tier, 0.5, true).glow > 0.2);
        }
    }

    #[test]
    fn confetti_never_falls_through_the_floor() {
        let b = beats(Tier::Max, 0.0, false);
        for i in 0..=100 {
            for bit in confetti(&b, i as f64 / 100.0, &STAGE, [0, 1, 2]) {
                assert!(bit.pos[1] - bit.size / 2.0 >= STAGE.floor - 1e-9);
            }
        }
    }

    #[test]
    fn motes_end_at_the_pet() {
        let b = beats(Tier::Level, 0.0, false);
        let near_end = motes(&b, 0.41, &STAGE);
        assert!(!near_end.is_empty());
        for m in near_end.iter().step_by(3) {
            assert!((m.pos[1] - STAGE.centre[1]).abs() < STAGE.height * 0.6);
        }
    }
}
