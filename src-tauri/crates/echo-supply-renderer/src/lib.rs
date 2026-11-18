//! EchoBird's pet-supply scene: a black foil pack you tear open, five face-down
//! cards, and one card that flips to show the reward the app already saved.
//!
//! The WASM build draws straight into a canvas in the existing Tauri window
//! with WebGL2; there is no per-frame IPC. This module holds the pure,
//! testable logic (timing, springs, the tear path and card layout); `scene`
//! and `gl` hold the WebGL side.

pub mod mat;
pub mod pet_fx;
pub mod pet_mesh;
pub mod pet_models;
pub mod pet_pose;

#[cfg(target_arch = "wasm32")]
mod gl;
#[cfg(target_arch = "wasm32")]
mod pet_scene;
#[cfg(target_arch = "wasm32")]
mod scene;
#[cfg(target_arch = "wasm32")]
mod shaders;
#[cfg(target_arch = "wasm32")]
pub use pet_scene::PetScene;
#[cfg(target_arch = "wasm32")]
pub use scene::SupplyScene;

/// Pack width / height, and the share of the height taken by each heat seal.
pub const PACK_ASPECT: f32 = 0.69;
pub const CRIMP: f32 = 0.068;
/// Samples along the tear line, left to right.
pub const TEAR_SAMPLES: usize = 64;

/// Seconds the automatic tear takes after a click or Enter.
pub const AUTO_TEAR_SECONDS: f32 = 1.15;
/// The tear sound starts once the strip has peeled this far.
pub const TEAR_CUE_PROGRESS: f32 = 0.22;
// The sound must wait for visible peeling, but not for most of the tear.
const _: () = assert!(TEAR_CUE_PROGRESS > 0.1 && TEAR_CUE_PROGRESS < 0.5);
/// Seconds from a finished tear until the cards are ready to pick.
pub const DEAL_SECONDS: f32 = 1.6;
/// Seconds for the picked card to travel to the centre and flip.
pub const REVEAL_SECONDS: f32 = 1.1;

pub fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

pub fn smooth(t: f32) -> f32 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}

pub fn ease_out(t: f32) -> f32 {
    let t = clamp01(t);
    1.0 - (1.0 - t).powi(3)
}

/// Overshoots slightly before settling; used for cards popping into place.
pub fn ease_back(t: f32) -> f32 {
    let t = clamp01(t);
    let c = 1.6;
    1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
}

/// A damped spring used for tilt, lift and hover.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub to: f32,
}

impl Spring {
    pub fn step(&mut self, dt: f32, stiffness: f32, damping: f32) {
        // Semi-implicit Euler in small sub-steps stays stable at low frame rates.
        let steps = ((dt / (1.0 / 240.0)).ceil() as u32).clamp(1, 16);
        let h = dt / steps as f32;
        for _ in 0..steps {
            let a = stiffness * (self.to - self.x) - damping * self.v;
            self.v += a * h;
            self.x += self.v * h;
        }
    }
}

/// Small deterministic RNG so a pack tears the same way for a given seed.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next_f32()
    }
}

/// The tear line, as distance from the top of the pack (fraction of height),
/// sampled left to right. It sits just below the top seal, wanders like real
/// film, and now and then snags into a small step.
pub fn tear_path(seed: u64) -> [f32; TEAR_SAMPLES + 1] {
    let mut rng = Rng::new(seed);
    let base = rng.range(0.112, 0.128);
    let slope = rng.range(-0.012, 0.012);
    let bow = rng.range(-0.004, 0.004);
    let freq = rng.range(1.5, 3.2);
    let phase = rng.range(0.0, std::f32::consts::TAU);
    let mut walk = 0.0f32;
    let mut out = [0.0; TEAR_SAMPLES + 1];
    for (i, y) in out.iter_mut().enumerate() {
        let s = i as f32 / TEAR_SAMPLES as f32 * 2.0 - 1.0;
        walk = (walk
            + rng.range(-0.0012, 0.0012)
            + if rng.next_f32() < 0.05 {
                rng.range(-0.004, 0.004)
            } else {
                0.0
            })
        .clamp(-0.008, 0.008);
        let jag = rng.range(-0.0015, 0.0015);
        *y = base
            + slope * s
            + bow * (1.0 - s * s)
            + 0.0035 * (s * freq * std::f32::consts::PI + phase).sin()
            + walk
            + jag;
        // Never into the seal above, never deep into the print below.
        *y = y.clamp(CRIMP + 0.02, 0.16);
    }
    out
}

/// Card width / height (5:7), in pack heights.
pub const CARD_W: f32 = 0.56;
pub const CARD_H: f32 = CARD_W * 1.4;

/// Centre-to-centre spacing of the five face-down cards, so they fit the
/// visible width (`half_width` in world units) with a small margin.
pub fn fan_spacing(half_width: f32, scale: f32) -> f32 {
    let card = CARD_W * scale;
    ((half_width * 2.0 * 0.94 - card) / 4.0).clamp(card * 0.55, card * 1.08)
}

/// Where each face-down card rests: x offset, height offset and roll.
/// The middle card sits highest, like a hand of cards.
pub fn fan_slot(index: usize, spacing: f32) -> (f32, f32, f32) {
    let i = index as f32 - 2.0;
    (i * spacing, -0.03 * i * i, -0.05 * i)
}

/// Which card is under a pointer at world x, or None between and beyond them.
pub fn slot_at(x: f32, spacing: f32, card_width: f32) -> Option<usize> {
    let i = (x / spacing + 2.0).round();
    if (0.0..=4.0).contains(&i)
        && (x - (i - 2.0) * spacing).abs() <= (card_width * 0.5).min(spacing * 0.5)
    {
        Some(i as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tear_line_stays_between_the_seal_and_the_print() {
        for seed in 0..200 {
            for y in tear_path(seed) {
                assert!(y > CRIMP && y <= 0.16, "seed {seed}: {y}");
            }
        }
    }

    #[test]
    fn same_seed_tears_the_same_way() {
        assert_eq!(tear_path(7), tear_path(7));
        assert_ne!(tear_path(7), tear_path(8));
    }

    #[test]
    fn five_slots_are_distinct_and_hit_testing_matches_them() {
        for half_width in [0.9f32, 1.2, 2.0] {
            let spacing = fan_spacing(half_width, 0.85);
            let xs: Vec<f32> = (0..5).map(|i| fan_slot(i, spacing).0).collect();
            for w in xs.windows(2) {
                assert!(w[1] > w[0]);
            }
            for (i, x) in xs.iter().enumerate() {
                assert_eq!(slot_at(*x, spacing, CARD_W * 0.85), Some(i));
            }
            assert_eq!(slot_at(10.0, spacing, CARD_W * 0.85), None);
        }
    }

    #[test]
    fn fan_stays_inside_the_view() {
        for half_width in [0.9f32, 1.2, 2.0] {
            let spacing = fan_spacing(half_width, 0.85);
            let outer = fan_slot(4, spacing).0 + CARD_W * 0.85 / 2.0;
            assert!(outer <= half_width * 1.0, "{half_width}: {outer}");
        }
    }

    #[test]
    fn spring_settles_on_its_target() {
        let mut s = Spring {
            to: 1.0,
            ..Default::default()
        };
        for _ in 0..240 {
            s.step(1.0 / 60.0, 120.0, 18.0);
        }
        assert!((s.x - 1.0).abs() < 1e-3);
    }

    #[test]
    fn easing_hits_both_ends() {
        for f in [smooth, ease_out, ease_back] {
            assert!(f(0.0).abs() < 1e-6);
            assert!((f(1.0) - 1.0).abs() < 1e-6);
        }
    }
}
