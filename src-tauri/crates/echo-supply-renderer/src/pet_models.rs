//! EchoBird's approved voxel pets: six species in three forms, the shared egg
//! and its three-piece shell, and the cell-by-cell growth tracks between forms.
//!
//! This is a faithful port of the original `voxelModels.js`. The test at the
//! bottom pins every voxel, part, pivot and growth track to a fingerprint of
//! the JavaScript output, so the pets look exactly as approved.
//!
//! Arithmetic follows JavaScript on purpose: f64 throughout, `Math.round`
//! rounds halves up, and maps keep insertion order (a re-claimed cell moves
//! to the end), because the growth matching depends on that order.

use std::collections::HashMap;

/// Species in the order the original families were declared.
pub const KINDS: [&str; 6] = ["unicorn", "mecha", "kaiju", "fox", "alien", "kangaroo"];

/// The shared palette, by name and sRGB hex.
pub const PALETTE: [(&str, &str); 28] = [
    ("ink", "#293945"),
    ("white", "#eee8d6"),
    ("pink", "#dba2a5"),
    ("cream", "#e9cba0"),
    ("red", "#cc7f7b"),
    ("orange", "#dfaa70"),
    ("yellow", "#dfca78"),
    ("mint", "#91bfa5"),
    ("blue", "#88b5cf"),
    ("purple", "#b7a4cb"),
    ("navy", "#4c6380"),
    ("steel", "#8e9fa8"),
    ("cyan", "#7fc8cd"),
    ("gold", "#d3b675"),
    ("metal", "#dce1d9"),
    ("beast", "#627e80"),
    ("beastDark", "#425e65"),
    ("spine", "#a5c3c8"),
    ("fox", "#cc8752"),
    ("foxDark", "#945f40"),
    ("green", "#99b99a"),
    ("greenDark", "#6e917e"),
    ("roo", "#b58d65"),
    ("rooDark", "#886a51"),
    ("vest", "#6b8390"),
    ("shell", "#dfdfcc"),
    ("speckle", "#99b8ac"),
    ("crack", "#667f78"),
];

pub mod c {
    pub const INK: u8 = 0;
    pub const WHITE: u8 = 1;
    pub const PINK: u8 = 2;
    pub const CREAM: u8 = 3;
    pub const RED: u8 = 4;
    pub const ORANGE: u8 = 5;
    pub const YELLOW: u8 = 6;
    pub const MINT: u8 = 7;
    pub const BLUE: u8 = 8;
    pub const PURPLE: u8 = 9;
    pub const NAVY: u8 = 10;
    pub const CYAN: u8 = 12;
    pub const GOLD: u8 = 13;
    pub const METAL: u8 = 14;
    pub const BEAST: u8 = 15;
    pub const BEAST_DARK: u8 = 16;
    pub const SPINE: u8 = 17;
    pub const FOX: u8 = 18;
    pub const FOX_DARK: u8 = 19;
    pub const GREEN: u8 = 20;
    pub const GREEN_DARK: u8 = 21;
    pub const ROO: u8 = 22;
    pub const ROO_DARK: u8 = 23;
    pub const VEST: u8 = 24;
    pub const SHELL: u8 = 25;
    pub const SPECKLE: u8 = 26;
    pub const CRACK: u8 = 27;
}

const RAINBOW: [u8; 6] = [c::RED, c::ORANGE, c::YELLOW, c::MINT, c::BLUE, c::PURPLE];

/// JavaScript's `Math.round`: halves round toward positive infinity.
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// The quintic smoothstep the original animations use, over [a, b].
pub fn smooth(a: f64, b: f64, t: f64) -> f64 {
    let u = ((t - a) / (b - a)).clamp(0.0, 1.0);
    u * u * u * (u * (u * 6.0 - 15.0) + 10.0)
}

pub type Cell = (i32, i32, i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Voxel {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub color: u8,
}

impl Voxel {
    pub fn cell(&self) -> Cell {
        (self.x, self.y, self.z)
    }
}

/// A map of voxels that keeps JavaScript `Map` insertion order.
#[derive(Clone, Debug, Default)]
pub struct VoxelMap {
    slots: Vec<Option<Voxel>>,
    index: HashMap<Cell, usize>,
}

impl VoxelMap {
    pub fn set(&mut self, v: Voxel) {
        if let Some(&i) = self.index.get(&v.cell()) {
            self.slots[i] = Some(v);
        } else {
            self.index.insert(v.cell(), self.slots.len());
            self.slots.push(Some(v));
        }
    }
    pub fn remove(&mut self, cell: Cell) {
        if let Some(i) = self.index.remove(&cell) {
            self.slots[i] = None;
        }
    }
    pub fn get(&self, cell: Cell) -> Option<&Voxel> {
        self.index.get(&cell).and_then(|&i| self.slots[i].as_ref())
    }
    pub fn contains(&self, cell: Cell) -> bool {
        self.index.contains_key(&cell)
    }
    pub fn len(&self) -> usize {
        self.index.len()
    }
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Voxel> {
        self.slots.iter().flatten()
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub pivot: [f64; 3],
    pub parent: Option<String>,
    pub voxels: VoxelMap,
}

#[derive(Clone, Debug)]
pub struct Definition {
    pub kind: &'static str,
    pub age: usize,
    pub parts: Vec<Part>,
    pub tail_count: usize,
    pub voxels: VoxelMap,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub center: [f64; 3],
    pub height: f64,
}

impl Definition {
    pub fn part(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.name == name)
    }
    pub fn part_index(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.name == name)
    }
}

struct Builder {
    parts: Vec<Part>,
    owners: HashMap<Cell, usize>,
    current: usize,
}

impl Builder {
    fn part(&mut self, name: &str, pivot: [f64; 3], parent: Option<&str>) {
        self.parts.push(Part {
            name: name.to_string(),
            pivot,
            parent: parent.map(str::to_string),
            voxels: VoxelMap::default(),
        });
        self.current = self.parts.len() - 1;
    }

    #[allow(clippy::too_many_arguments)]
    fn cube(&mut self, x: i32, y: i32, z: i32, w: i32, h: i32, d: i32, color: u8) {
        for i in 0..w {
            for j in 0..h {
                for k in 0..d {
                    let v = Voxel {
                        x: x + i,
                        y: y + j,
                        z: z + k,
                        color,
                    };
                    if let Some(&previous) = self.owners.get(&v.cell()) {
                        self.parts[previous].voxels.remove(v.cell());
                    }
                    self.parts[self.current].voxels.set(v);
                    self.owners.insert(v.cell(), self.current);
                }
            }
        }
    }

    /// A box and its mirror image across x = 0.
    #[allow(clippy::too_many_arguments)]
    fn pair(&mut self, x: i32, y: i32, z: i32, w: i32, h: i32, d: i32, color: u8) {
        self.cube(x, y, z, w, h, d, color);
        self.cube(-x - w, y, z, w, h, d, color);
    }

    /// A chunky voxel line from a to b, stepping one axis at a time.
    fn line(&mut self, a: [i32; 3], b: [i32; 3], size: i32, color: u8) {
        let steps = (0..3).map(|i| (a[i] - b[i]).abs()).max().unwrap_or(0) * 2;
        let mut previous: Option<[i32; 3]> = None;
        for t in 0..=steps {
            let f = if steps != 0 {
                t as f64 / steps as f64
            } else {
                0.0
            };
            let p: [i32; 3] = std::array::from_fn(|i| {
                let v = a[i] as f64;
                js_round(v + (b[i] as f64 - v) * f) as i32 - size.div_euclid(2)
            });
            if let Some(prev) = previous.as_mut() {
                for axis in 0..3 {
                    while prev[axis] != p[axis] {
                        prev[axis] += (p[axis] - prev[axis]).signum();
                        let q = *prev;
                        self.cube(q[0], q[1], q[2], size, size, size, color);
                    }
                }
            }
            self.cube(p[0], p[1], p[2], size, size, size, color);
            previous = Some(p);
        }
    }
}

fn half_floor(v: i32) -> i32 {
    v.div_euclid(2)
}

fn build_definition(kind: &'static str, s: usize) -> Definition {
    let mut b = Builder {
        parts: Vec::new(),
        owners: HashMap::new(),
        current: 0,
    };
    let si = s as i32;
    let pick = |v: [i32; 3]| v[s];
    let f = |v: i32| v as f64;
    let mut tail_count = 0;
    match kind {
        "unicorn" => {
            let leg = pick([2, 3, 4]);
            let bw = pick([4, 5, 6]);
            let bl = pick([6, 8, 10]);
            let by = leg;
            let bh = pick([3, 3, 4]);
            let bz = -half_floor(bl);
            let front = bz + bl - 2;
            let hy = by + bh - 1;
            let hh = pick([3, 4, 4]);
            b.part("body", [0.0, f(by + 1), 0.0], None);
            b.cube(-half_floor(bw), by, bz, bw, bh, bl, c::WHITE);
            for side in [-1, 1] {
                for end in ["front", "back"] {
                    let w = if s == 2 { 2 } else { 1 };
                    let x = if side > 0 {
                        half_floor(bw) - w
                    } else {
                        -half_floor(bw)
                    };
                    let z = if end == "front" { front } else { bz + 1 };
                    let name = format!("{}-{}", if side > 0 { "right" } else { "left" }, end);
                    b.part(&name, [f(x) + f(w) / 2.0, f(leg), f(z + 1)], None);
                    b.cube(x, 0, z, w, leg + 1, 2, c::WHITE);
                    b.cube(x, 0, z, w, 1, 2, c::PINK);
                }
            }
            b.part("head", [0.0, f(hy), f(front)], Some("body"));
            b.cube(-2, hy, front, 4, hh, 4, c::WHITE);
            b.cube(-2, hy, front + 3, 4, 2, 2, c::CREAM);
            b.pair(1, hy + hh, front + 1, 1, pick([1, 2, 2]), 1, c::WHITE);
            b.cube(-1, hy + hh, front + 2, 1, si + 1, 1, c::GOLD);
            for i in 0..3 + si {
                b.cube(-1, hy + hh - i, front - 1, 2, 1, 1, RAINBOW[i as usize]);
            }
            b.part(
                "eyes",
                [0.0, f(hy + hh) - 1.5, f(front) + 4.5],
                Some("head"),
            );
            b.pair(1, hy + hh - 2, front + 4, 1, 1, 1, c::INK);
            b.part("tail", [0.0, f(by + 1), f(bz)], Some("body"));
            for i in 0..3 + si {
                b.cube(
                    -1,
                    by + 1 - half_floor(i),
                    bz - i - 1,
                    2,
                    2,
                    1,
                    RAINBOW[(i % 6) as usize],
                );
            }
        }
        "mecha" => {
            let leg = 2 + si;
            let by = leg + 1;
            let bh = 3 + i32::from(s > 0);
            let bw = 4 + si;
            let bd = 3 + i32::from(s == 2);
            let bz = -half_floor(bd);
            let front = bz + bd;
            let hy = by + bh + 1;
            b.part("body", [0.0, f(by + 1), 0.0], None);
            b.cube(-half_floor(bw), by, bz, bw, bh, bd, c::METAL);
            b.cube(-2, by - 1, -1, 4, 2, 3, c::NAVY);
            b.cube(-1, by + bh, -1, 2, 2, 2, c::NAVY);
            b.cube(-1, by + 1, front, 2, 2, 1, c::NAVY);
            b.cube(0, by + 1, front + 1, 1, 1, 1, c::CYAN);
            for side in [-1, 1] {
                let x = if side > 0 { 1 } else { -3 };
                let leg_name = if side > 0 { "right-leg" } else { "left-leg" };
                b.part(leg_name, [f(x + 1), f(leg + 1), 0.0], None);
                b.cube(x, 1, -1, 2, leg + 1, 2, c::NAVY);
                b.cube(x, 0, -1, 2, 1, 4, c::METAL);
                if s > 0 {
                    b.cube(x, leg - 1, 1, 2, 2, 1, c::BLUE);
                }
                let ax = if side > 0 {
                    half_floor(bw)
                } else {
                    -half_floor(bw) - 2
                };
                let ay = by + bh - 1;
                let arm = if side > 0 { "right-arm" } else { "left-arm" };
                b.part(arm, [f(ax + 1), f(ay), 0.0], Some("body"));
                b.cube(ax, ay - 2 - si, -1, 2, 4 + si, 2, c::NAVY);
                b.cube(ax, ay, -1, 2, 2, 3, c::BLUE);
                b.cube(ax, ay - 2 - si, 1, 2, 2, 1, c::METAL);
                if s == 2 {
                    b.cube(ax, ay, 2, 2, 2, 1, c::GOLD);
                }
            }
            b.part("head", [0.0, f(hy), 0.0], Some("body"));
            b.cube(-2, hy, -1, 4, 3, 3, c::METAL);
            b.cube(-2, hy + 2, -1, 4, 1, 3, c::BLUE);
            if s > 0 {
                b.pair(2, hy + 2, 0, 1, 2, 1, c::GOLD);
                if s == 2 {
                    b.pair(3, hy + 3, 0, 1, 2, 1, c::GOLD);
                }
            }
            b.part("eyes", [0.0, f(hy) + 1.5, 2.5], Some("head"));
            b.cube(-1, hy + 1, 2, 2, 1, 1, c::CYAN);
        }
        "kaiju" => {
            let leg = 2 + si;
            let by = leg - 1;
            let bh = 5 + si;
            let bw = 4 + si;
            let bz = -2;
            let bd = 4 + si;
            let hy = by + bh - 1;
            let hd = 4;
            let hw = 4 + i32::from(s == 2);
            let hx = -half_floor(hw);
            let head_z = 1;
            b.part("body", [0.0, f(by + 2), 0.0], None);
            b.cube(-half_floor(bw), by, bz, bw, bh, bd, c::BEAST);
            b.cube(-1, by + 1, bz + bd, 2, bh - 2, 1, c::CREAM);
            for side in [-1, 1] {
                let x = if side > 0 { 1 } else { -3 };
                let leg_name = if side > 0 { "right-leg" } else { "left-leg" };
                b.part(leg_name, [f(x + 1), f(leg + 1), 0.0], None);
                b.cube(x, 0, -1, 2, leg + 2, 3, c::BEAST_DARK);
                b.cube(x, 0, 1, 2, 1, 3, c::BEAST_DARK);
                let ax = if side > 0 {
                    half_floor(bw)
                } else {
                    -half_floor(bw) - 1
                };
                let ay = by + bh - 2;
                let arm = if side > 0 { "right-arm" } else { "left-arm" };
                b.part(arm, [f(ax) + 0.5, f(ay), 2.0], Some("body"));
                b.cube(ax, ay - 2, 2, 1, 3 + si, 2, c::BEAST_DARK);
                b.cube(ax, ay - 2, 4, 1, 1, 1, c::CREAM);
            }
            b.part("head", [0.0, f(hy), f(head_z)], Some("body"));
            b.cube(hx, hy, head_z, hw, 3 + i32::from(s == 2), hd, c::BEAST);
            b.cube(hx, hy, head_z + 3, hw, 2, 2, c::BEAST);
            b.part("jaw", [0.0, f(hy), f(head_z + 3)], Some("head"));
            b.cube(hx, hy - 1, head_z + 2, hw, 1, 3, c::BEAST_DARK);
            b.pair(1, hy, head_z + 4, 1, 1, 1, c::CREAM);
            b.part("eyes", [0.0, f(hy) + 1.5, f(head_z) + 4.5], Some("head"));
            b.pair(1, hy + 1, head_z + 4, 1, 1, 1, c::GOLD);
            b.part("spines", [0.0, f(by + 2), f(bz)], Some("body"));
            for i in 0..3 + si {
                b.cube(-1, by + i * 2, bz - 1 - (i % 2), 2, 2, 2, c::SPINE);
            }
            b.part("tail", [0.0, f(by + 1), f(bz)], Some("body"));
            b.line([0, by + 1, bz], [0, by, bz - 3 - si], 2, c::BEAST_DARK);
            b.line(
                [0, by, bz - 3 - si],
                [2, by + 1, bz - 5 - si * 2],
                if s == 0 { 1 } else { 2 },
                c::BEAST,
            );
        }
        "fox" => {
            let leg = 1 + si;
            let by = leg;
            let bw = 4 + i32::from(s == 2);
            let bl = 5 + si;
            let bz = -half_floor(bl);
            let front = bz + bl - 1;
            let hy = by + 2;
            let hh = 3;
            b.part("body", [0.0, f(by + 1), 0.0], None);
            b.cube(-half_floor(bw), by, bz, bw, 3, bl, c::FOX);
            b.cube(-1, by, front, 2, 2, 1, c::CREAM);
            for side in [-1, 1] {
                for end in ["front", "back"] {
                    let x = if side > 0 { 1 } else { -2 };
                    let z = if end == "front" { front - 1 } else { bz };
                    let name = format!("{}-{}", if side > 0 { "right" } else { "left" }, end);
                    b.part(&name, [f(x) + 0.5, f(leg), f(z) + 0.5], None);
                    b.cube(x, 0, z, 1, leg + 1, 2, c::FOX_DARK);
                }
            }
            b.part("head", [0.0, f(hy), f(front)], Some("body"));
            b.cube(-2, hy, front, 4, hh, 3, c::FOX);
            b.cube(-2, hy, front + 2, 4, 2, 2, c::CREAM);
            b.cube(-1, hy + 1, front + 4, 2, 1, 1, c::INK);
            b.pair(1, hy + hh, front, 1, 2 + i32::from(s == 2), 1, c::FOX_DARK);
            b.part("eyes", [0.0, f(hy) + 2.5, f(front) + 3.5], Some("head"));
            b.pair(1, hy + 2, front + 3, 1, 1, 1, c::INK);
            let count = pick([3, 6, 9]);
            tail_count = count as usize;
            for i in 0..count {
                let x = js_round((f(i) - f(count - 1) / 2.0) * 2.0) as i32;
                let ty = by + 5 + si * 2 - js_round(f(x.abs()) * 0.32) as i32;
                let tz = bz - 4 - si - (i % 2);
                let base = [0, by + 1, bz];
                b.part(&format!("tail-{i}"), [0.0, f(by + 1), f(bz)], Some("body"));
                let mid = [
                    js_round(f(x) * 0.5) as i32,
                    by + 3,
                    js_round(f(bz + tz) / 2.0) as i32,
                ];
                b.line(base, mid, 1, c::FOX);
                b.line(mid, [x, ty, tz], 1, c::FOX);
                b.cube(x, ty - 1, tz, 1, 3, 1, c::CREAM);
            }
        }
        "alien" => {
            let leg = 2 + si;
            let by = leg + 1;
            let bh = 2 + si;
            let hy = by + bh + 1;
            let hw = 6 + if s == 2 { 2 } else { 0 };
            let hx = -hw / 2;
            let hh = 4 + i32::from(s == 2);
            let hd = 4;
            b.part("body", [0.0, f(by + 1), 0.0], None);
            b.cube(-1, by, -1, 2, bh, 3, c::GREEN_DARK);
            b.cube(-1, by + 1, 2, 2, 1, 1, c::GOLD);
            b.cube(-1, by + bh, -1, 2, 2, 2, c::GREEN_DARK);
            for side in [-1, 1] {
                let x = if side > 0 { 1 } else { -2 };
                let leg_name = if side > 0 { "right-leg" } else { "left-leg" };
                b.part(leg_name, [f(x) + 0.5, f(leg + 1), 0.0], None);
                b.cube(x, 0, 0, 1, leg + 2, 1, c::GREEN_DARK);
                b.cube(x, 0, 0, 1, 1, 2, c::GREEN);
                let ax = if side > 0 { 1 } else { -2 };
                let ay = by + bh - 1;
                let arm = if side > 0 { "right-arm" } else { "left-arm" };
                b.part(arm, [f(ax) + 0.5, f(ay), 0.0], Some("body"));
                b.cube(ax, ay - 2 - si, 0, 1, 3 + si, 1, c::GREEN);
                b.cube(ax, ay - 2 - si, 1, 1, 2, 1, c::GREEN);
            }
            b.part("head", [0.0, f(hy), 0.0], Some("body"));
            b.cube(hx, hy, -1, hw, hh, hd, c::GREEN);
            b.pair(hw / 2, hy + 1, 0, 1, 2, 1, c::GREEN_DARK);
            b.pair(1, hy + hh, 0, 1, si, 1, c::GREEN_DARK);
            if s > 0 {
                b.pair(1, hy + hh + si, 0, 1, 1, 1, c::GOLD);
            }
            b.part("eyes", [0.0, f(hy) + f(hh) / 2.0, 3.5], Some("head"));
            b.pair(1, hy + 1, 3, 2, 2, 1, c::INK);
        }
        "kangaroo" => {
            let leg = 2 + si;
            let by = leg;
            let bh = 4 + si;
            let bw = 3 + si;
            let bz = -2;
            let bd = 4;
            let hy = by + bh - 1;
            let front = 2;
            let hh = 3;
            b.part("body", [0.0, f(by + 1), 0.0], None);
            b.cube(-half_floor(bw), by, bz, bw, bh, bd, c::ROO);
            b.cube(-1, by + 1, front, 2, 1 + si, 1, c::CREAM);
            if s > 0 {
                b.pair(1, by + 1, front, 1, bh - 1, 1, c::VEST);
                if s == 2 {
                    b.pair(0, by + bh - 1, front + 1, 1, 1, 1, c::INK);
                }
            }
            for side in [-1, 1] {
                let x = if side > 0 { 1 } else { -3 };
                let leg_name = if side > 0 { "right-leg" } else { "left-leg" };
                b.part(leg_name, [f(x + 1), f(leg + 1), 0.0], None);
                b.cube(x, 0, -1, 2, leg + 2, 2, c::ROO_DARK);
                b.cube(x, 0, 0, 2, 1, 3 + si, c::ROO_DARK);
                let ax = if side > 0 {
                    (bw + 1).div_euclid(2)
                } else {
                    -half_floor(bw) - 1
                };
                let ay = by + bh - 2;
                let arm = if side > 0 { "right-arm" } else { "left-arm" };
                b.part(arm, [f(ax) + 0.5, f(ay), 1.0], Some("body"));
                b.cube(ax, ay - 2, 1, 1, 3, 1, c::ROO_DARK);
                b.cube(ax, ay - 2, 2, 1, 1, 1, c::CREAM);
            }
            b.part("head", [0.0, f(hy), f(front)], Some("body"));
            b.cube(-2, hy, front, 4, hh, 3, c::ROO);
            b.cube(-2, hy, front + 2, 4, 2, 2, c::CREAM);
            b.cube(-1, hy + 1, front + 4, 2, 1, 1, c::ROO_DARK);
            b.pair(1, hy + hh, front, 1, 2 + si, 1, c::ROO_DARK);
            b.part("eyes", [0.0, f(hy) + 2.5, f(front) + 3.5], Some("head"));
            b.pair(1, hy + 2, front + 3, 1, 1, 1, c::INK);
            if s == 2 {
                b.part("hat", [0.0, f(hy + hh), f(front + 1)], Some("head"));
                b.cube(-2, hy + hh, front, 4, 1, 3, c::INK);
                b.cube(-1, hy + hh + 1, front, 2, 2, 2, c::INK);
            }
            b.part("tail", [0.0, f(by + 1), f(bz)], Some("body"));
            b.line([0, by + 1, bz], [0, 1, bz - 3 - si * 2], 2, c::ROO_DARK);
        }
        _ => unreachable!("unknown pet kind {kind}"),
    }
    let mut voxels = VoxelMap::default();
    for p in &b.parts {
        for v in p.voxels.iter() {
            voxels.set(*v);
        }
    }
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    for v in voxels.iter() {
        for (a, value) in [v.x, v.y, v.z].into_iter().enumerate() {
            min[a] = min[a].min(value as f64);
            max[a] = max[a].max(value as f64 + 1.0);
        }
    }
    let center = std::array::from_fn(|i| (min[i] + max[i]) / 2.0);
    Definition {
        kind,
        age: s,
        parts: b.parts,
        tail_count,
        voxels,
        min,
        max,
        center,
        height: max[1] - min[1],
    }
}

/// One cell's journey from one form to the next.
#[derive(Clone, Debug)]
pub struct Track {
    pub part: String,
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub ca: u8,
    pub cb: u8,
    pub sa: f64,
    pub sb: f64,
    pub delay: f64,
}

#[derive(Clone, Debug)]
pub struct Transition {
    pub kind: &'static str,
    pub step: usize,
    pub tracks: Vec<Track>,
}

/// Where a growth cell is at `amount` (0..1) of its transition.
pub struct GrowthCell {
    pub p: [f64; 3],
    pub scale: f64,
    pub u: f64,
}

pub fn growth_cell(track: &Track, amount: f64) -> GrowthCell {
    let u = smooth(track.delay, 1.0, amount);
    let scale = if track.sa == track.sb {
        1.0 + 0.08 * (std::f64::consts::PI * u).sin()
    } else if track.sa == 0.0 {
        smooth(0.0, 0.68, u)
    } else {
        1.0 - smooth(0.32, 1.0, u)
    };
    let mut p: [f64; 3] = std::array::from_fn(|i| track.a[i] + (track.b[i] - track.a[i]) * u);
    p[1] = (scale / 2.0).max(p[1]);
    GrowthCell { p, scale, u }
}

/// Which half of a two-step growth animation `t` is in, and how far along.
pub fn growth_phase(t: f64) -> (usize, f64) {
    if t <= 0.5 {
        (0, smooth(0.06, 0.44, t))
    } else {
        (1, smooth(0.56, 0.94, t))
    }
}

#[derive(Clone)]
struct Sample {
    p: [f64; 3],
    color: u8,
}

fn distance(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let mut n = 0.0;
    for i in 0..3 {
        n += (a[i] - b[i]) * (a[i] - b[i]);
    }
    n
}

fn closest<'a>(cells: &'a [Sample], target: &[f64; 3]) -> &'a Sample {
    let mut best = &cells[0];
    for p in &cells[1..] {
        if distance(&p.p, target) < distance(&best.p, target) {
            best = p;
        }
    }
    best
}

fn growth_tracks(from: &Definition, to: &Definition) -> Transition {
    let position = |v: &Voxel, d: &Definition| {
        [
            v.x as f64 + 0.5 - d.center[0],
            v.y as f64 + 0.5 - d.min[1],
            v.z as f64 + 0.5 - d.center[2],
        ]
    };
    let bounds = |cells: &[Sample]| -> [[f64; 2]; 3] {
        std::array::from_fn(|a| {
            let lo = cells.iter().map(|v| v.p[a]).fold(f64::INFINITY, f64::min);
            let hi = cells
                .iter()
                .map(|v| v.p[a])
                .fold(f64::NEG_INFINITY, f64::max);
            [lo, hi]
        })
    };
    let sample = |part: &Part, d: &Definition| -> Vec<Sample> {
        part.voxels
            .iter()
            .map(|v| Sample {
                p: position(v, d),
                color: v.color,
            })
            .collect()
    };
    let project = |p: &[f64; 3], a: &[[f64; 2]; 3], b: &[[f64; 2]; 3]| -> [f64; 3] {
        std::array::from_fn(|i| {
            b[i][0] + ((p[i] - a[i][0]) / (a[i][1] - a[i][0]).max(1.0)) * (b[i][1] - b[i][0])
        })
    };
    let mut names: Vec<&str> = Vec::new();
    for p in from.parts.iter().chain(to.parts.iter()) {
        if !names.contains(&p.name.as_str()) {
            names.push(&p.name);
        }
    }
    let mut tracks = Vec::new();
    for name in names {
        let pa = from.part(name);
        let pb = to.part(name);
        let a = pa.map(|p| sample(p, from)).unwrap_or_default();
        let b = pb.map(|p| sample(p, to)).unwrap_or_default();
        let delay = if name == "hat" {
            0.16
        } else if name.starts_with("tail") {
            0.1
        } else if name == "head" || name == "eyes" {
            0.06
        } else {
            0.0
        };
        let mut add = |v: &Sample, w: &Sample, sa: f64, sb: f64| {
            tracks.push(Track {
                part: name.to_string(),
                a: v.p,
                b: w.p,
                ca: v.color,
                cb: w.color,
                sa,
                sb,
                delay,
            })
        };
        if !a.is_empty() && !b.is_empty() {
            let (ba, bb) = (bounds(&a), bounds(&b));
            let forward = a.len() <= b.len();
            let (small, large) = if forward { (&a, &b) } else { (&b, &a) };
            let mut used = vec![false; large.len()];
            for v in small {
                let target = if forward {
                    project(&v.p, &ba, &bb)
                } else {
                    project(&v.p, &bb, &ba)
                };
                let mut best = usize::MAX;
                let mut score = f64::INFINITY;
                for (i, cell) in large.iter().enumerate() {
                    if !used[i] {
                        let n = distance(&target, &cell.p)
                            + if v.color == cell.color { 0.0 } else { 0.4 };
                        if n < score {
                            score = n;
                            best = i;
                        }
                    }
                }
                used[best] = true;
                if forward {
                    add(v, &large[best], 1.0, 1.0);
                } else {
                    add(&large[best], v, 1.0, 1.0);
                }
            }
            for (i, v) in large.iter().enumerate() {
                if !used[i] {
                    let target = if forward {
                        project(&v.p, &bb, &ba)
                    } else {
                        project(&v.p, &ba, &bb)
                    };
                    let anchor = closest(small, &target);
                    if forward {
                        add(anchor, v, 0.0, 1.0);
                    } else {
                        add(v, anchor, 1.0, 0.0);
                    }
                }
            }
        } else {
            let growing = !b.is_empty();
            let (existing, destination) = if growing { (from, to) } else { (to, from) };
            let part = if growing { pb } else { pa }.expect("part exists on one side");
            let parent = part
                .parent
                .as_deref()
                .and_then(|p| existing.part(p))
                .or_else(|| existing.part("body"))
                .expect("every pet has a body");
            let anchors = sample(parent, existing);
            let whole = |d: &Definition| -> Vec<Sample> {
                d.voxels
                    .iter()
                    .map(|v| Sample {
                        p: position(v, d),
                        color: 0,
                    })
                    .collect()
            };
            let ba = bounds(&whole(destination));
            let bb = bounds(&whole(existing));
            for v in if growing { &b } else { &a } {
                let target = project(&v.p, &ba, &bb);
                let anchor = closest(&anchors, &target);
                if growing {
                    add(anchor, v, 0.0, 1.0);
                } else {
                    add(v, anchor, 1.0, 0.0);
                }
            }
        }
    }
    Transition {
        kind: from.kind,
        step: from.age,
        tracks,
    }
}

pub struct Models {
    pub definitions: Vec<Definition>,
    pub egg: VoxelMap,
    pub cracked: VoxelMap,
    pub shells: [VoxelMap; 3],
    pub transitions: Vec<Transition>,
}

impl Models {
    pub fn definition(&self, kind: &str, age: usize) -> &Definition {
        self.definitions
            .iter()
            .find(|d| d.kind == kind && d.age == age)
            .expect("known pet")
    }
    pub fn transition(&self, kind: &str, step: usize) -> &Transition {
        self.transitions
            .iter()
            .find(|t| t.kind == kind && t.step == step)
            .expect("known transition")
    }
}

pub fn build() -> Models {
    let definitions: Vec<Definition> = KINDS
        .iter()
        .flat_map(|kind| (0..3).map(move |s| build_definition(kind, s)))
        .collect();

    let mut egg = VoxelMap::default();
    for x in -6..6 {
        for y in 0..14 {
            for z in -6..6 {
                let (xf, yf, zf) = (x as f64, y as f64, z as f64);
                let fy = (yf + 0.5 - 7.0) / 7.0;
                let r = 5.3 - fy * 0.8;
                let iy = (yf + 0.5 - 7.0) / 5.7;
                let ir = 4.1 - iy * 0.8;
                let sq = |v: f64| v * v;
                if sq((xf + 0.5) / r) + fy * fy + sq((zf + 0.5) / r) > 1.0
                    || sq((xf + 0.5) / ir) + iy * iy + sq((zf + 0.5) / ir) < 1.0
                {
                    continue;
                }
                let speckle = (x - 2) * (x - 2) + (y - 9) * (y - 9) + (z - 3) * (z - 3) < 4
                    || (x + 3) * (x + 3) + (y - 4) * (y - 4) + (z - 2) * (z - 2) < 4;
                egg.set(Voxel {
                    x,
                    y,
                    z,
                    color: if speckle { c::SPECKLE } else { c::SHELL },
                });
            }
        }
    }

    const DIRECTIONS: [Cell; 6] = [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ];
    let mut regions: HashMap<Cell, usize> = HashMap::new();
    for v in egg.iter() {
        let region = if v.y >= 8 + (v.x + v.z + 24).div_euclid(2) % 2 {
            2
        } else if v.x >= (v.y + v.z + 24).div_euclid(2) % 2 {
            1
        } else {
            0
        };
        regions.insert(v.cell(), region);
    }
    let neighbors = |v: &Voxel| -> Vec<Cell> {
        DIRECTIONS
            .iter()
            .map(|d| (v.x + d.0, v.y + d.1, v.z + d.2))
            .filter(|c| egg.contains(*c))
            .collect()
    };
    for v in egg.iter() {
        let around = neighbors(v);
        let own = regions[&v.cell()];
        if !around.iter().any(|n| regions[n] == own) {
            let first = regions[&around[0]];
            regions.insert(v.cell(), first);
        }
    }
    let mut cracked = VoxelMap::default();
    let mut shells: [VoxelMap; 3] = Default::default();
    for v in egg.iter() {
        let part = regions[&v.cell()];
        let seam = neighbors(v).iter().any(|n| regions[n] < part);
        let next = Voxel {
            color: if seam { c::CRACK } else { v.color },
            ..*v
        };
        cracked.set(next);
        shells[part].set(next);
    }

    let transitions = KINDS
        .iter()
        .flat_map(|kind| {
            let defs = &definitions;
            (0..2).map(move |s| {
                let find = |age| defs.iter().find(|d| d.kind == *kind && d.age == age);
                growth_tracks(find(s).unwrap(), find(s + 1).unwrap())
            })
        })
        .collect();

    Models {
        definitions,
        egg,
        cracked,
        shells,
        transitions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: f64) -> String {
        format!("{}", js_round(v * 1000.0) as i64)
    }
    fn list(v: &[f64]) -> String {
        v.iter().map(|x| n(*x)).collect::<Vec<_>>().join(",")
    }
    fn hex(color: u8) -> &'static str {
        PALETTE[color as usize].1
    }

    /// The same text the JavaScript models produce, line for line.
    fn dump(m: &Models) -> String {
        let mut out = String::new();
        let mut line = |s: String| {
            out.push_str(&s);
            out.push('\n');
        };
        for d in &m.definitions {
            line(format!(
                "def {} {} tails={} min={} max={} center={} h={}",
                d.kind,
                d.age,
                d.tail_count,
                list(&d.min),
                list(&d.max),
                list(&d.center),
                n(d.height)
            ));
            for p in &d.parts {
                line(format!(
                    "part {} parent={} pivot={}",
                    p.name,
                    p.parent.as_deref().unwrap_or("-"),
                    list(&p.pivot)
                ));
                for v in p.voxels.iter() {
                    line(format!("v {} {} {} {}", v.x, v.y, v.z, hex(v.color)));
                }
            }
        }
        let named = [
            ("egg".to_string(), &m.egg),
            ("cracked".to_string(), &m.cracked),
            ("shell0".to_string(), &m.shells[0]),
            ("shell1".to_string(), &m.shells[1]),
            ("shell2".to_string(), &m.shells[2]),
        ];
        for (name, map) in named {
            line(name);
            for v in map.iter() {
                line(format!("v {} {} {} {}", v.x, v.y, v.z, hex(v.color)));
            }
        }
        for t in &m.transitions {
            line(format!("tr {} {}", t.kind, t.step));
            for k in &t.tracks {
                line(format!(
                    "t {} {} {} {} {} {} {} {}",
                    k.part,
                    list(&k.a),
                    list(&k.b),
                    hex(k.ca),
                    hex(k.cb),
                    k.sa,
                    k.sb,
                    n(k.delay)
                ));
            }
        }
        out
    }

    fn fnv1a(text: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    #[test]
    fn models_match_the_approved_javascript_output_exactly() {
        let text = dump(&build());
        if let Ok(path) = std::env::var("PET_MODEL_DUMP") {
            std::fs::write(path, &text).unwrap();
        }
        assert_eq!(text.lines().count(), 9390);
        assert_eq!(fnv1a(&text), 0xf64a_c47c_2911_c465);
    }

    #[test]
    fn six_species_three_forms_and_a_shared_shell() {
        let m = build();
        assert_eq!(m.definitions.len(), 18);
        for d in &m.definitions {
            assert!(d.voxels.len() > 50);
            assert!(d.part("body").is_some() && d.part("head").is_some());
        }
        let egg: Vec<Cell> = m.egg.iter().map(Voxel::cell).collect();
        let cracked: Vec<Cell> = m.cracked.iter().map(Voxel::cell).collect();
        assert_eq!(egg, cracked);
        assert!(m.egg.len() > 100);
        assert_eq!(
            m.shells.iter().map(VoxelMap::len).sum::<usize>(),
            m.egg.len()
        );
    }

    #[test]
    fn growth_moves_from_exact_source_cells_to_target_cells() {
        let m = build();
        assert_eq!(m.transitions.len(), 12);
        for t in &m.transitions {
            let mut moving = 0;
            for track in &t.tracks {
                let start = growth_cell(track, 0.0);
                let end = growth_cell(track, 1.0);
                let middle = growth_cell(track, 0.5);
                assert_eq!(start.scale, track.sa);
                assert_eq!(end.scale, track.sb);
                for i in 0..3 {
                    let lift = |p: f64, s: f64| if i == 1 { p.max(s / 2.0) } else { p };
                    assert_eq!(start.p[i], lift(track.a[i], track.sa));
                    assert_eq!(end.p[i], lift(track.b[i], track.sb));
                }
                assert!(middle.p.iter().all(|v| v.is_finite()) && middle.scale >= 0.0);
                if middle.u > 0.0 && middle.u < 1.0 {
                    moving += 1;
                }
            }
            assert!(moving > 0);
        }
    }
}
