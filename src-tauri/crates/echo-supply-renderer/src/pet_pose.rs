//! What the pet scene shows at a given moment: which meshes, where, and from
//! which camera. A faithful port of the original three.js `petScene.js`
//! (poses, special moves, the hatch, growth, framing and tints), plus two
//! touches of polish: a soft ground shadow and sparkles on big moments.
//!
//! Everything here is plain math so it can be tested natively; `pet_scene`
//! turns a [`Frame`] into WebGL draw calls.

use std::f64::consts::PI;

use crate::mat::Mat4;
use crate::pet_models::{c, growth_cell, growth_phase, smooth, Definition, Models, KINDS, PALETTE};

/// The egg stays cracked from this point of the hatch until it opens.
pub const CRACKED_EGG_PROGRESS: f64 = 0.46;
/// The six colourways, in the order the app stores them.
pub const TINTS: [&str; 6] = [
    "#88b5cf", "#b7a4cb", "#91bfa5", "#dfaa70", "#dba2a5", "#dfca78",
];
const SHELL_ORIGINS: [[f64; 3]; 3] = [[-3.0, 4.0, 0.0], [3.0, 4.0, 0.0], [0.0, 10.0, 0.0]];
const CAMERA: [f32; 3] = [0.0, 12.0, 80.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Still,
    Level,
    Interact,
    Crack,
    Hatch,
    Grow,
}

impl Mode {
    pub fn parse(name: &str) -> Mode {
        match name {
            "level" => Mode::Level,
            "interact" => Mode::Interact,
            "crack" => Mode::Crack,
            "hatch" => Mode::Hatch,
            "grow" => Mode::Grow,
            _ => Mode::Still,
        }
    }
}

/// A pet as the app describes it: species (index into [`KINDS`]),
/// stage 0..=4 (egg, cracked, baby, young, adult) and colourway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pet {
    pub kind: usize,
    pub stage: usize,
    pub color: usize,
}

impl Pet {
    pub fn kind_name(&self) -> &'static str {
        KINDS[self.kind]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub mode: Mode,
    pub progress: f64,
    /// Turn around the vertical axis, in degrees (28 by default).
    pub angle: f64,
    pub tilt: f64,
    /// The fixed three-quarter view and studio light used for thumbnails.
    pub miniature: bool,
    pub reduced: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKey {
    Part { def: usize, part: usize },
    Egg,
    Shell(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub mesh: MeshKey,
    pub model: Mat4,
    /// How far the egg's crack colour has spread (only the whole egg uses it).
    pub crack: f32,
}

/// Growth cells mid-transition: which transition, how far, and the group's
/// transform.
#[derive(Clone, Copy, Debug)]
pub struct Cells {
    pub transition: usize,
    pub amount: f64,
    pub group: Mat4,
}

/// A soft shadow on the floor under the pet, in the pet's own frame.
#[derive(Clone, Copy, Debug)]
pub struct Ground {
    pub model: Mat4,
    pub radius: [f32; 2],
    pub alpha: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Spark {
    pub pos: [f32; 3],
    pub size: f32,
    pub alpha: f32,
    pub spin: f32,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub items: Vec<Item>,
    /// The glowing core inside the cracked egg (unlit).
    pub core: Option<Mat4>,
    pub cells: Option<Cells>,
    pub ground: Option<Ground>,
    pub sparks: Vec<Spark>,
    pub view: Mat4,
    pub proj: Mat4,
}

fn f(v: f64) -> f32 {
    v as f32
}
fn v3(v: [f64; 3]) -> [f32; 3] {
    [f(v[0]), f(v[1]), f(v[2])]
}
fn translate(v: [f64; 3]) -> Mat4 {
    Mat4::translate(f(v[0]), f(v[1]), f(v[2]))
}

/// Local min and max corners.
type Bounds = [[f64; 3]; 2];
/// Floor height in object space, footprint radii, and lift off the floor.
type Floor = (f64, [f64; 2], f64);

/// Per-part rotation and scale, plus the root's own transform.
struct RigPose {
    rot: Vec<[f64; 3]>,
    scale: Vec<[f64; 3]>,
    root_pos: [f64; 3],
    root_scale: [f64; 3],
}

impl RigPose {
    fn rest(def: &Definition) -> RigPose {
        RigPose {
            rot: vec![[0.0; 3]; def.parts.len()],
            scale: vec![[1.0; 3]; def.parts.len()],
            root_pos: [0.0; 3],
            root_scale: [1.0; 3],
        }
    }
}

/// The idle sway, blink and each species' special move at time `t` (0..1).
fn pose_rig(def: &Definition, t: f64, special: bool) -> RigPose {
    let mut p = RigPose::rest(def);
    let idx = |name: &str| def.part_index(name);
    let e = if t <= 0.0 || t >= 1.0 {
        0.0
    } else {
        (PI * t).sin()
    };
    let wave = (t * PI * 2.0).sin();
    let rot = |p: &mut RigPose, name: &str, axis: usize, value: f64| {
        if let Some(i) = idx(name) {
            p.rot[i][axis] = value;
        }
    };
    if let Some(b) = idx("body") {
        p.scale[b][1] = 1.0 + 0.018 * wave;
    }
    rot(&mut p, "head", 1, 0.09 * wave);
    rot(&mut p, "head", 2, 0.025 * wave);
    let blink = if t > 0.36 && t < 0.46 {
        ((t - 0.36) / 0.1 * PI).sin()
    } else {
        0.0
    };
    if let Some(eyes) = idx("eyes") {
        if def.kind != "mecha" {
            p.scale[eyes][1] = 1.0 - 0.88 * blink;
        }
    }
    rot(&mut p, "tail", 1, 0.1 * wave);
    for i in 0..def.tail_count {
        let name = format!("tail-{i}");
        rot(
            &mut p,
            &name,
            1,
            0.045 * (t * PI * 2.0 + i as f64 * 0.5).sin() * e,
        );
    }
    if !special {
        return p;
    }
    match def.kind {
        "unicorn" => {
            rot(&mut p, "right-front", 0, -0.65 * e);
            rot(&mut p, "head", 0, 0.22 * e);
            rot(&mut p, "tail", 1, 0.35 * wave);
        }
        "mecha" => {
            rot(&mut p, "head", 1, 0.35 * wave);
            rot(&mut p, "right-arm", 2, 1.15 * e);
            rot(&mut p, "right-arm", 0, -0.35 * e);
            rot(&mut p, "left-arm", 2, -0.16 * e);
            rot(&mut p, "body", 1, 0.08 * wave);
        }
        "kaiju" => {
            rot(&mut p, "head", 0, -0.2 * e);
            rot(&mut p, "jaw", 0, 0.65 * e);
            rot(&mut p, "right-arm", 0, -0.45 * e);
            rot(&mut p, "left-arm", 0, -0.45 * e);
            rot(&mut p, "tail", 1, 0.28 * wave);
            rot(&mut p, "body", 0, -0.08 * e);
        }
        "fox" => {
            rot(&mut p, "head", 2, 0.22 * e);
            rot(&mut p, "right-front", 0, -0.5 * e);
            let count = def.tail_count as f64;
            for i in 0..def.tail_count {
                let name = format!("tail-{i}");
                let side = i as f64 - (count - 1.0) / 2.0;
                let sign = if side > 0.0 {
                    1.0
                } else if side < 0.0 {
                    -1.0
                } else {
                    0.0
                };
                rot(&mut p, &name, 2, -sign * 0.13 * e);
                rot(
                    &mut p,
                    &name,
                    1,
                    0.12 * (t * PI * 2.0 + i as f64 * 0.4).sin() * e,
                );
            }
        }
        "alien" => {
            rot(&mut p, "head", 2, 0.15 * wave);
            rot(&mut p, "right-arm", 2, 1.7 * e);
            rot(&mut p, "right-arm", 0, 0.2 * (t * PI * 8.0).sin() * e);
            rot(&mut p, "body", 1, 0.1 * e);
        }
        "kangaroo" => {
            p.root_pos[1] = (t * PI * 2.0).sin().max(0.0) * [1.0, 1.4, 1.8][def.age];
            rot(&mut p, "body", 0, 0.2 * e);
            rot(&mut p, "head", 0, 0.16 * e);
            rot(&mut p, "right-arm", 2, 1.15 * e);
            rot(&mut p, "hat", 2, 0.22 * e);
            rot(&mut p, "tail", 0, 0.12 * e);
        }
        _ => {}
    }
    p
}

/// World matrices for every part of a posed rig under `outer`.
fn rig_items(def_index: usize, def: &Definition, pose: &RigPose, outer: &Mat4) -> Vec<Item> {
    let root = outer.mul(&translate(pose.root_pos)).mul(&Mat4::scale(
        f(pose.root_scale[0]),
        f(pose.root_scale[1]),
        f(pose.root_scale[2]),
    ));
    let mut world: Vec<Mat4> = Vec::with_capacity(def.parts.len());
    let mut items = Vec::with_capacity(def.parts.len());
    for (i, part) in def.parts.iter().enumerate() {
        let parent = part.parent.as_deref().and_then(|name| def.part_index(name));
        let parent_pivot = parent.map_or([0.0; 3], |p| def.parts[p].pivot);
        let bind: [f64; 3] = std::array::from_fn(|a| part.pivot[a] - parent_pivot[a]);
        let local = Mat4::trs(v3(bind), v3(pose.rot[i]), v3(pose.scale[i]));
        let m = parent.map_or(root, |p| world[p]).mul(&local);
        world.push(m);
        items.push(Item {
            mesh: MeshKey::Part {
                def: def_index,
                part: i,
            },
            model: m,
            crack: 0.0,
        });
    }
    items
}

fn def_index(models: &Models, kind: &str, age: usize) -> usize {
    models
        .definitions
        .iter()
        .position(|d| d.kind == kind && d.age == age)
        .expect("known pet")
}

struct Hatch {
    wobble_rz: f64,
    wobble_scale: f64,
    egg_visible: bool,
    crack: f64,
    pieces: [([f64; 3], [f64; 3]); 3],
    pieces_visible: bool,
    piece_scale: f64,
    core_scale: f64,
    core_visible: bool,
    rig_pos: [f64; 3],
    rig_scale: f64,
    root_pos: [f64; 3],
    root_scale_y: f64,
    head_x: f64,
}

/// The egg wobbling, cracking into three pieces and the baby climbing out.
fn pose_hatch(def: &Definition, t: f64, reduced: bool) -> Hatch {
    let opening = smooth(0.34, 0.78, t);
    let birth = smooth(0.38, 0.84, t);
    let crack = smooth(0.12, 0.34, t);
    let wobble_rz = if reduced {
        0.0
    } else {
        (t * 52.0).sin() * 0.11 * (PI * smooth(0.0, 0.44, t)).sin().powi(2)
    };
    let core_scale = 1.0 - smooth(CRACKED_EGG_PROGRESS, 0.72, t);
    let pieces = std::array::from_fn(|i| {
        let p = SHELL_ORIGINS[i];
        let mut pos = [p[0], p[1] - 7.0, p[2]];
        let mut rot = [0.0; 3];
        if i < 2 {
            pos[0] += if i == 0 { -8.0 } else { 8.0 } * opening;
            pos[1] += 1.3 * (opening * PI).sin() - 0.6 * opening;
            pos[2] += opening * 2.0;
            rot[2] = if i == 0 { 1.0 } else { -1.0 } * opening * 0.9;
        } else {
            pos[1] += 5.5 * (opening * PI).sin() - 6.0 * opening;
            pos[2] -= 8.0 * opening;
            rot[0] = -1.1 * opening;
        }
        (pos, rot)
    });
    let size = 0.65f64
        .min(6.0 / (def.max[2] - def.min[2]))
        .min(8.0 / def.height);
    let settle = (PI * smooth(0.82, 1.0, t)).sin();
    Hatch {
        wobble_rz,
        wobble_scale: 1.0,
        egg_visible: t < 0.34,
        crack,
        pieces,
        pieces_visible: t >= 0.34,
        piece_scale: 1.0,
        core_scale,
        core_visible: t >= 0.34 && core_scale > 0.001,
        rig_pos: [
            0.0,
            -7.0 + 2.0 * (1.0 - birth)
                + if reduced {
                    0.0
                } else {
                    (birth * PI).sin() * 1.1
                },
            0.0,
        ],
        rig_scale: size + (1.0 - size) * birth,
        root_pos: [-def.center[0], 0.0, -def.center[2]],
        root_scale_y: 1.0 - 0.12 * (1.0 - birth) - 0.05 * settle,
        head_x: 0.2 * (1.0 - birth) * smooth(0.3, 0.5, t),
    }
}

fn shell_frame(t: f64) -> [[f64; 3]; 2] {
    let u = smooth(0.34, 0.78, t);
    [
        [-6.0 - 12.0 * u, -7.0 - u, -6.0 - 9.0 * u],
        [6.0 + 12.0 * u, 7.0 + 2.0 * u, 6.0 + 4.0 * u],
    ]
}

fn rig_bounds(def: &Definition) -> [[f64; 3]; 2] {
    [
        [
            def.min[0] - def.center[0],
            -def.height / 2.0,
            def.min[2] - def.center[2],
        ],
        [
            def.max[0] - def.center[0],
            def.height / 2.0,
            def.max[2] - def.center[2],
        ],
    ]
}

/// A small deterministic hash for sparkle layouts.
fn hash(i: u32, salt: u32) -> f64 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f64 / 65535.0
}

/// A burst of sparkles from `origin`, `age` 0..1 into it.
fn burst(
    origin: [f64; 3],
    age: f64,
    count: u32,
    reach: f64,
    salt: u32,
) -> Vec<([f64; 3], f64, f64)> {
    if !(0.0..1.0).contains(&age) {
        return Vec::new();
    }
    (0..count)
        .map(|i| {
            let a = hash(i, salt) * PI * 2.0;
            let up = 0.25 + hash(i, salt + 1) * 0.9;
            let speed = reach * (0.55 + hash(i, salt + 2) * 0.6);
            let travel = 1.0 - (1.0 - age).powi(2);
            let pos = [
                origin[0] + a.cos() * speed * travel,
                origin[1] + up * speed * travel - 4.0 * age * age,
                origin[2] + a.sin() * speed * travel * 0.6,
            ];
            let twinkle = 0.65 + 0.35 * (age * 30.0 + i as f64 * 1.7).sin();
            let size = 0.7 + hash(i, salt + 3) * 0.8;
            (pos, size, (1.0 - age).powf(1.5) * twinkle)
        })
        .collect()
}

/// Everything to draw for `pet` seen through `view` on a `width` x `height`
/// canvas.
pub fn frame(models: &Models, pet: Pet, view: &View, width: f64, height: f64) -> Frame {
    let kind = pet.kind_name();
    let mini = view.miniature;
    let (mode, progress) = (view.mode, view.progress);
    let object = Mat4::euler(
        if mini { 0.0 } else { f(view.tilt) },
        if mini {
            0.45
        } else {
            f(view.angle.to_radians())
        },
        0.0,
    );
    let mut items = Vec::new();
    let mut core = None;
    let mut cells = None;
    let mut sparkles: Vec<([f64; 3], f64, f64)> = Vec::new();
    // The object's own matrix, its local bounds, and the floor for its shadow
    // (height in object space, footprint radii, lift off the floor).
    let (matrix, bounds, floor): (Mat4, Bounds, Floor);

    let hatching = !mini && (mode == Mode::Hatch || mode == Mode::Crack);
    let growing = !mini && mode == Mode::Grow;
    if hatching || (pet.stage == 1 && !growing) {
        let baby = def_index(models, kind, 0);
        let def = &models.definitions[baby];
        let t = if !hatching {
            CRACKED_EGG_PROGRESS
        } else if mode == Mode::Crack {
            progress * CRACKED_EGG_PROGRESS
        } else {
            progress
        };
        let mut h = pose_hatch(def, t, view.reduced);
        let mut show_rig = hatching;
        let mut b = shell_frame(t);
        if hatching && mode == Mode::Crack {
            h.wobble_rz *= (PI * progress).sin();
            show_rig = false;
        } else if !hatching {
            h.wobble_rz = if !mini && !view.reduced && mode == Mode::Interact {
                (progress * PI * 6.0).sin() * 0.05 * (progress * PI).sin()
            } else {
                0.0
            };
            h.wobble_scale = if !mini && mode == Mode::Level {
                1.0 + 0.045 * (progress * PI).sin()
            } else {
                1.0
            };
        }
        if hatching && mode == Mode::Hatch {
            let settle = smooth(0.84, 1.0, t);
            h.rig_pos[1] += (7.0 - def.height / 2.0) * settle;
            h.piece_scale = 1.0 - settle;
            let target = rig_bounds(def);
            for side in 0..2 {
                for a in 0..3 {
                    b[side][a] += (target[side][a] - b[side][a]) * settle;
                }
            }
        }
        let wobble = object.mul(&Mat4::trs(
            [0.0; 3],
            [0.0, 0.0, f(h.wobble_rz)],
            [f(h.wobble_scale); 3],
        ));
        if h.egg_visible {
            items.push(Item {
                mesh: MeshKey::Egg,
                model: wobble.mul(&Mat4::translate(0.0, -7.0, 0.0)),
                crack: f(h.crack),
            });
        }
        if h.pieces_visible && h.piece_scale > 0.0 {
            for (i, (pos, rot)) in h.pieces.iter().enumerate() {
                let o = SHELL_ORIGINS[i];
                let model = wobble
                    .mul(&Mat4::trs(v3(*pos), v3(*rot), [f(h.piece_scale); 3]))
                    .mul(&translate([-o[0], -o[1], -o[2]]));
                items.push(Item {
                    mesh: MeshKey::Shell(i),
                    model,
                    crack: 0.0,
                });
            }
        }
        if h.core_visible {
            let s = f(h.core_scale);
            core = Some(wobble.mul(&Mat4::trs([0.0, 1.5, 1.0], [0.0; 3], [s; 3])));
        }
        if show_rig {
            let mut pose = pose_rig(def, 0.0, false);
            pose.root_pos = h.root_pos;
            pose.root_scale = [1.0, h.root_scale_y, 1.0];
            if let Some(head) = def.part_index("head") {
                pose.rot[head][0] = h.head_x;
            }
            let outer = object.mul(&Mat4::trs(v3(h.rig_pos), [0.0; 3], [f(h.rig_scale); 3]));
            items.extend(rig_items(baby, def, &pose, &outer));
        }
        if hatching && mode == Mode::Hatch {
            sparkles = burst([0.0, 1.0, 1.0], (t - 0.34) / 0.5, 22, 13.0, 11);
        } else if hatching {
            sparkles = burst([0.0, 2.0, 2.0], (progress - 0.45) / 0.55, 10, 8.0, 5);
        }
        matrix = object;
        bounds = b;
        floor = (-7.0, [6.0, 6.0], 0.0);
    } else if growing {
        let (step, amount) = growth_phase(progress);
        let from = models.definition(kind, step);
        let to = models.definition(kind, step + 1);
        let height = from.height + (to.height - from.height) * amount;
        let bound = |side: usize, a: usize| {
            let pick = |d: &Definition| {
                if side == 0 {
                    d.min[a] - d.center[a]
                } else {
                    d.max[a] - d.center[a]
                }
            };
            pick(from) * (1.0 - amount) + pick(to) * amount
        };
        let group = Mat4::translate(0.0, f(-height / 2.0), 0.0).mul(&object);
        let transition = models
            .transitions
            .iter()
            .position(|t| t.kind == kind && t.step == step)
            .expect("known transition");
        cells = Some(Cells {
            transition,
            amount,
            group,
        });
        matrix = group;
        bounds = [
            [bound(0, 0), 0.0, bound(0, 2)],
            [bound(1, 0), height, bound(1, 2)],
        ];
        let footprint = [
            (bound(1, 0) - bound(0, 0)) / 2.0,
            (bound(1, 2) - bound(0, 2)) / 2.0,
        ];
        floor = (0.0, footprint, 0.0);
        let local = (progress - if step == 0 { 0.06 } else { 0.56 }) / 0.4;
        sparkles = burst([0.0, height * 0.55, 0.0], local, 14, 9.0, 23 + step as u32);
    } else if pet.stage == 0 {
        let pulse = !mini && mode == Mode::Level;
        let s = if pulse {
            1.0 + 0.045 * (progress * PI).sin()
        } else {
            1.0
        };
        let rz = if !mini && !view.reduced && mode == Mode::Interact {
            (progress * PI * 6.0).sin() * 0.05 * (progress * PI).sin()
        } else {
            0.0
        };
        items.push(Item {
            mesh: MeshKey::Egg,
            model: object.mul(&Mat4::trs([0.0, -7.0, 0.0], [0.0, 0.0, f(rz)], [f(s); 3])),
            crack: 0.0,
        });
        if pulse {
            sparkles = burst([0.0, 0.0, 2.0], progress, 8, 9.0, 31);
        }
        matrix = object;
        bounds = [[-6.0, -7.0, -6.0], [6.0, 7.0, 6.0]];
        floor = (-7.0, [6.0, 6.0], 0.0);
    } else {
        let index = def_index(models, kind, pet.stage - 2);
        let def = &models.definitions[index];
        let t = if !mini && mode == Mode::Interact {
            progress
        } else {
            0.0
        };
        let mut pose = pose_rig(def, t, true);
        let jump = pose.root_pos[1];
        pose.root_pos = [
            -def.center[0],
            jump - def.height / 2.0 - def.min[1],
            -def.center[2],
        ];
        let mut lift = jump;
        if !mini && mode == Mode::Level {
            let pulse = (progress * PI).sin();
            pose.root_pos[1] += pulse * 0.6;
            pose.root_scale = [1.0 + 0.04 * pulse; 3];
            lift += pulse * 0.6;
            sparkles = burst([0.0, 0.0, 2.0], progress, 10, 10.0, 41);
        }
        items.extend(rig_items(index, def, &pose, &object));
        matrix = object;
        bounds = rig_bounds(def);
        let footprint = [
            (def.max[0] - def.min[0]) / 2.0,
            (def.max[2] - def.min[2]) / 2.0,
        ];
        floor = (-def.height / 2.0, footprint, lift);
    }

    // Fit an orthographic camera around the transformed bounds, as before.
    let view_m = Mat4::look_at(CAMERA, [0.0; 3]);
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for x in [bounds[0][0], bounds[1][0]] {
        for y in [bounds[0][1], bounds[1][1]] {
            for z in [bounds[0][2], bounds[1][2]] {
                let p = matrix.point([f(x), f(y), f(z)]);
                for a in 0..3 {
                    lo[a] = lo[a].min(p[a]);
                    hi[a] = hi[a].max(p[a]);
                }
            }
        }
    }
    let aspect = (width / height) as f32;
    let mut half = 1.0f32;
    for x in [lo[0], hi[0]] {
        for y in [lo[1], hi[1]] {
            for z in [lo[2], hi[2]] {
                let q = view_m.point([x, y, z]);
                half = half.max(q[1].abs()).max(q[0].abs() / aspect);
            }
        }
    }
    half *= if mini { 1.15 } else { 1.12 };
    let proj = Mat4::ortho(-half * aspect, half * aspect, -half, half, 0.1, 250.0);

    let (floor_y, footprint, lift) = floor;
    let ground = Some(Ground {
        model: matrix.mul(&Mat4::translate(0.0, f(floor_y) - 0.02, 0.0)),
        radius: [
            f(footprint[0] * 1.25 / (1.0 + lift * 0.06)),
            f(footprint[1] * 1.15 / (1.0 + lift * 0.06)),
        ],
        alpha: f(0.24 / (1.0 + lift * 0.35)),
    });
    let sparks = if mini || view.reduced {
        Vec::new()
    } else {
        sparkles
            .into_iter()
            .enumerate()
            .map(|(i, (p, size, alpha))| {
                let w = matrix.point(v3(p));
                Spark {
                    pos: [w[0], w[1], w[2] + 6.0],
                    size: f(size),
                    alpha: f(alpha),
                    spin: f(progress * 3.0 + i as f64),
                }
            })
            .collect()
    };
    Frame {
        items,
        core,
        cells,
        ground,
        sparks,
        view: view_m,
        proj,
    }
}

/// Growth cell transforms and colour blend for a [`Cells`] frame:
/// (centre, scale, colour a, colour b, blend).
pub fn cell_instances(models: &Models, cells: &Cells) -> Vec<[f32; 7]> {
    models.transitions[cells.transition]
        .tracks
        .iter()
        .map(|track| {
            let cell = growth_cell(track, cells.amount);
            [
                f(cell.p[0]),
                f(cell.p[1]),
                f(cell.p[2]),
                f(cell.scale.max(0.000001)),
                track.ca as f32,
                track.cb as f32,
                f(cell.u),
            ]
        })
        .collect()
}

fn srgb_to_linear(c: f64) -> f64 {
    if c < 0.04045 {
        c * 0.0773993808
    } else {
        (c * 0.9478672986 + 0.0521327014).powf(2.4)
    }
}

/// A hex colour as linear RGB, as three.js stores `Color`.
pub fn linear(hex: &str) -> [f64; 3] {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    [
        srgb_to_linear(((v >> 16) & 255) as f64 / 255.0),
        srgb_to_linear(((v >> 8) & 255) as f64 / 255.0),
        srgb_to_linear((v & 255) as f64 / 255.0),
    ]
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn times(a: [f64; 3], k: f64) -> [f64; 3] {
    a.map(|v| v * k)
}

/// The palette in linear RGB with this pet's colourway applied: shells,
/// and each species' signature colours, follow the chosen tint.
pub fn palette(kind: usize, color: usize) -> [[f32; 3]; 28] {
    let mut out: [[f64; 3]; 28] = std::array::from_fn(|i| linear(PALETTE[i].1));
    let tint = linear(TINTS[color.min(TINTS.len() - 1)]);
    let cream = linear(PALETTE[c::SHELL as usize].1);
    let mut set = |key: u8, value: [f64; 3]| out[key as usize] = value;
    set(c::SHELL, lerp(tint, cream, 0.48));
    set(c::SPECKLE, times(tint, 0.8));
    set(c::CRACK, times(tint, 0.35));
    match KINDS[kind] {
        "mecha" => set(c::BLUE, tint),
        "unicorn" => set(
            c::WHITE,
            lerp(tint, linear(PALETTE[c::WHITE as usize].1), 0.7),
        ),
        "kaiju" => {
            set(c::BEAST, times(tint, 0.7));
            set(c::BEAST_DARK, times(tint, 0.42));
            set(c::SPINE, lerp(tint, cream, 0.35));
        }
        "fox" => {
            set(c::FOX, tint);
            set(c::FOX_DARK, times(tint, 0.5));
        }
        "alien" => {
            set(c::GREEN, tint);
            set(c::GREEN_DARK, times(tint, 0.55));
        }
        "kangaroo" => {
            set(c::ROO, times(tint, 0.85));
            set(c::ROO_DARK, times(tint, 0.5));
        }
        _ => {}
    }
    out.map(|v| v.map(|x| x as f32))
}

/// The core's glow: the tint, more saturated and a little lighter, worked
/// out in linear HSL like three.js's `offsetHSL(0, 0.45, 0.1)`.
pub fn core_color(color: usize) -> [f32; 3] {
    let [r, g, b] = linear(TINTS[color.min(TINTS.len() - 1)]);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (min + max) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if max != min {
        let d = max - min;
        s = if l <= 0.5 {
            d / (max + min)
        } else {
            d / (2.0 - max - min)
        };
        h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } / 6.0;
    }
    let s = (s + 0.45).clamp(0.0, 1.0);
    let l = (l + 0.1).clamp(0.0, 1.0);
    let hue = |p: f64, q: f64, mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * 6.0 * (2.0 / 3.0 - t)
        } else {
            p
        }
    };
    let p = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let q = 2.0 * l - p;
    [
        hue(q, p, h + 1.0 / 3.0) as f32,
        hue(q, p, h) as f32,
        hue(q, p, h - 1.0 / 3.0) as f32,
    ]
}

/// three.js-style Lambert lights, already divided by π: key and fill
/// direction and colour, then ambient. Daylight is for the light theme.
pub fn lights(daylight: bool) -> [[f32; 3]; 5] {
    let (ambient, key, fill) = if daylight {
        (2.8, 2.7, 1.25)
    } else {
        (1.65, 2.5, 0.6)
    };
    let norm = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [f(v[0] / l), f(v[1] / l), f(v[2] / l)]
    };
    let k = linear("#fff5e7");
    let fl = linear("#d9ecff");
    [
        norm([-30.0, 55.0, 60.0]),
        k.map(|v| f(v * key / PI)),
        norm([30.0, 10.0, -35.0]),
        fl.map(|v| f(v * fill / PI)),
        [f(ambient / PI); 3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet_models::build;

    fn view(mode: Mode, progress: f64) -> View {
        View {
            mode,
            progress,
            angle: 28.0,
            tilt: 0.0,
            miniature: false,
            reduced: false,
        }
    }
    fn pet(stage: usize) -> Pet {
        Pet {
            kind: 1,
            stage,
            color: 0,
        }
    }
    fn centre(m: &Mat4) -> [f32; 3] {
        let p = m.point([0.0; 3]);
        [p[0], p[1], p[2]]
    }

    #[test]
    fn level_ten_shows_three_separated_shell_pieces_instead_of_a_recoloured_egg() {
        let m = build();
        let f = frame(&m, pet(1), &view(Mode::Still, 0.0), 400.0, 400.0);
        let shells: Vec<&Item> = f
            .items
            .iter()
            .filter(|i| matches!(i.mesh, MeshKey::Shell(_)))
            .collect();
        assert_eq!(shells.len(), 3);
        assert!(!f.items.iter().any(|i| i.mesh == MeshKey::Egg));
        let (a, b) = (centre(&shells[0].model), centre(&shells[1].model));
        assert!((a[0] - b[0]).abs() > 0.5, "side pieces have moved apart");
    }

    #[test]
    fn the_cracked_shell_matches_at_the_upgrade_end_and_when_shown_again() {
        let m = build();
        let end = frame(&m, pet(0), &view(Mode::Crack, 1.0), 400.0, 400.0);
        let shown = frame(&m, pet(1), &view(Mode::Still, 0.0), 400.0, 400.0);
        assert_eq!(end.items.len(), shown.items.len());
        for (a, b) in end.items.iter().zip(&shown.items) {
            assert_eq!(a.mesh, b.mesh);
            for (x, y) in a.model.0.iter().zip(&b.model.0) {
                assert!((x - y).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn only_the_cracked_egg_has_a_single_glowing_core() {
        let m = build();
        for stage in 0..5 {
            let f = frame(&m, pet(stage), &view(Mode::Still, 0.0), 400.0, 400.0);
            assert_eq!(f.core.is_some(), stage == 1, "stage {stage}");
        }
    }

    #[test]
    fn every_pet_and_stage_frames_inside_the_view() {
        let m = build();
        for kind in 0..6 {
            for stage in 0..5 {
                for mini in [false, true] {
                    let mut v = view(Mode::Still, 0.0);
                    v.miniature = mini;
                    let p = Pet {
                        kind,
                        stage,
                        color: kind,
                    };
                    let f = frame(&m, p, &v, 300.0, 200.0);
                    let vp = f.proj.mul(&f.view);
                    for item in &f.items {
                        let c = vp.point(centre(&item.model));
                        assert!(c[0].abs() <= 1.0 && c[1].abs() <= 1.0, "{kind} {stage}");
                    }
                }
            }
        }
    }

    #[test]
    fn hatching_and_growing_run_end_to_end_without_gaps() {
        let m = build();
        for kind in 0..6 {
            let p = Pet {
                kind,
                stage: 1,
                color: 0,
            };
            for i in 0..=20 {
                let t = CRACKED_EGG_PROGRESS + (1.0 - CRACKED_EGG_PROGRESS) * i as f64 / 20.0;
                let f = frame(&m, p, &view(Mode::Hatch, t), 400.0, 400.0);
                assert!(!f.items.is_empty());
            }
            let p = Pet {
                kind,
                stage: 2,
                color: 0,
            };
            for i in 0..=20 {
                let f = frame(&m, p, &view(Mode::Grow, i as f64 / 20.0), 400.0, 400.0);
                let cells = f.cells.expect("growth cells");
                assert!(!cell_instances(&m, &cells).is_empty());
            }
        }
    }

    #[test]
    fn tints_recolour_the_signature_colours_only() {
        let base = palette(0, 0);
        let fox = palette(3, 3);
        assert_ne!(fox[c::FOX as usize], base[c::FOX as usize]);
        assert_eq!(fox[c::INK as usize], base[c::INK as usize]);
        let core = core_color(0);
        assert!(core.iter().all(|v| (0.0..=1.0).contains(v)));
    }
}
