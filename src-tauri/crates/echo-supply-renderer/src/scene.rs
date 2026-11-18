//! WebGL2 scene for the pet supply: a black pack you tear open, five
//! face-down cards, and one card that flips to show the reward the app has
//! already saved. React owns the business state; this module owns drawing,
//! pointer feel and timing, and reports events through `frame`'s bit flags.
//!
//! The scene never knows the reward before the pick: React passes the real
//! reward's words to `reveal` only after `EV_PICKED`.

use wasm_bindgen::prelude::*;
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement, WebGl2RenderingContext as Gl,
    WebGlTexture,
};

use crate::gl::{
    err, grid, offscreen_canvas, texture_from_canvas, texture_from_image, Mat4, Mesh, Program,
};
use crate::shaders::*;
use crate::{
    clamp01, ease_back, ease_out, fan_slot, fan_spacing, slot_at, smooth, tear_path, Rng, Spring,
    AUTO_TEAR_SECONDS, CARD_H, CARD_W, PACK_ASPECT, REVEAL_SECONDS, TEAR_CUE_PROGRESS,
    TEAR_SAMPLES,
};

/// Bits returned by `frame`.
pub const EV_TEAR_CUE: u32 = 1;
pub const EV_OPENED: u32 = 2;
pub const EV_READY: u32 = 4;
pub const EV_PICKED: u32 = 8;
pub const EV_REVEAL_MID: u32 = 16;
pub const EV_REVEAL_DONE: u32 = 32;

const FOV: f32 = 0.52;
const EYE_Z: f32 = 3.4;
const PACK_H: f32 = 1.0;
const FAN_SCALE: f32 = 0.78;
/// Card thickness and corner radius, in pack heights.
const CARD_T: f32 = 0.016;
const CARD_R: f32 = 0.045;
/// Depth the fanned cards rest at; spacing is computed for this plane.
const FAN_Z: f32 = 0.0;
const REVEAL_SCALE: f32 = 1.16;

/// Mascot layers, as sent by JS in `add_art`.
const L_BODY: usize = 0;
const L_EYES: usize = 1;
const L_JOY: usize = 2;
const L_WINCE: usize = 3;
const L_SMILE: usize = 4;
const L_OPEN: usize = 5;
const LAYERS: usize = 6;
/// Echo (pack/combo), Sprout (growth), Twin (double), Zip (haste), Puff (super).
const CHARS: usize = 5;
const DESIGNS: usize = 5;
const DESIGN_CHAR: [usize; DESIGNS] = [1, 2, 3, 0, 4];

const RIBBON_COLORS: [[f32; 3]; 7] = [
    [1.0, 0.42, 0.62],
    [1.0, 0.72, 0.32],
    [1.0, 0.9, 0.35],
    [0.5, 0.9, 0.48],
    [0.35, 0.82, 1.0],
    [0.52, 0.55, 1.0],
    [0.8, 0.5, 1.0],
];

/// Canvas letter spacing is newer than this web-sys; browsers without it ignore the field.
fn letter_spacing(ctx: &CanvasRenderingContext2d, value: &str) {
    let _ = js_sys::Reflect::set(ctx, &"letterSpacing".into(), &value.into());
}

/// Edge colours for all five revealed card designs.
const EDGE_TONES: [[f32; 3]; DESIGNS] = [
    [0.74, 0.88, 0.58],
    [0.97, 0.8, 0.47],
    [0.56, 0.5, 0.9],
    [0.98, 0.72, 0.78],
    [0.68, 0.79, 0.98],
];

/// The card's sides as a closed band of triangles around the rounded
/// rectangle, between the back (z = -T/2) and the front (z = +T/2).
fn rim_vertices() -> Vec<f32> {
    let (hw, hh) = (CARD_W / 2.0 - CARD_R, CARD_H / 2.0 - CARD_R);
    let mut ring = Vec::new();
    for (cx, cy, start) in [
        (hw, hh, 0.0f32),
        (-hw, hh, 90.0),
        (-hw, -hh, 180.0),
        (hw, -hh, 270.0),
    ] {
        for k in 0..=8 {
            let a = (start + k as f32 * 90.0 / 8.0).to_radians();
            ring.push((cx + a.cos() * CARD_R, cy + a.sin() * CARD_R));
        }
    }
    let (f, b) = (CARD_T / 2.0, -CARD_T / 2.0);
    let mut out = Vec::with_capacity(ring.len() * 18);
    for i in 0..ring.len() {
        let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
        out.extend_from_slice(&[p.0, p.1, f, q.0, q.1, f, p.0, p.1, b]);
        out.extend_from_slice(&[p.0, p.1, b, q.0, q.1, f, q.0, q.1, b]);
    }
    out
}

fn rounded_rect(ctx: &CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, r: f64) {
    ctx.begin_path();
    ctx.move_to(x + r, y);
    let _ = ctx.arc_to(x + w, y, x + w, y + h, r);
    let _ = ctx.arc_to(x + w, y + h, x, y + h, r);
    let _ = ctx.arc_to(x, y + h, x, y, r);
    let _ = ctx.arc_to(x, y, x + w, y, r);
    ctx.close_path();
}

const FONT: &str = "-apple-system, system-ui, 'PingFang SC', 'Microsoft YaHei', sans-serif";

struct Sparkle {
    pos: [f32; 3],
    vel: [f32; 3],
    size: f32,
    color: [f32; 3],
}

/// Where one card is this frame.
#[derive(Clone, Copy)]
struct CardPose {
    index: usize,
    pos: [f32; 3],
    roll: f32,
    yaw: f32,
    pitch: f32,
    scale: f32,
    alpha: f32,
}

#[wasm_bindgen]
pub struct SupplyScene {
    gl: Gl,
    canvas: HtmlCanvasElement,
    pack_prog: Program,
    sprite_prog: Program,
    shape_prog: Program,
    ribbon_prog: Program,
    card_prog: Program,
    rim_prog: Program,
    rim_mesh: Mesh,
    pack_mesh: Mesh,
    card_mesh: Mesh,
    quad: Mesh,
    ribbon_mesh: Mesh,
    print: Option<WebGlTexture>,
    logo: Option<HtmlImageElement>,
    card_back: Option<WebGlTexture>,
    card_front: Option<WebGlTexture>,
    art: Vec<Option<WebGlTexture>>,
    copy: [String; 5],
    width: f32,
    height: f32,
    dpr: f32,
    nx: f32,
    ny: f32,
    pointer_world: (f32, f32),
    hovering: bool,
    rx: Spring,
    ry: Spring,
    rz: Spring,
    lift: Spring,
    tear: [f32; TEAR_SAMPLES + 1],
    seed: u64,
    progress: Spring,
    dir: f32,
    drag: Option<(f32, f32)>,
    moved: bool,
    auto_from: Option<(f32, f32)>,
    opened_at: Option<f32>,
    cue_sent: bool,
    opened_sent: bool,
    ready_sent: bool,
    hover: [Spring; 5],
    /// Per-card pitch and yaw: a lean toward the pointer, or a gentle waft.
    card_tilt: [[Spring; 2]; 5],
    /// Where the pointer last touched each card, in face uv.
    glare: [(f32, f32); 5],
    hover_index: Option<usize>,
    picked: Option<usize>,
    picked_sent: bool,
    reveal_at: Option<f32>,
    reveal_kind: usize,
    mid_sent: bool,
    done_sent: bool,
    card_rx: Spring,
    card_ry: Spring,
    reduced: bool,
    time: f32,
    last_ms: Option<f64>,
    blink_at: f32,
    blink_until: f32,
    sparkles: Vec<Sparkle>,
    disposed: bool,
}

#[wasm_bindgen]
impl SupplyScene {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<SupplyScene, JsValue> {
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"antialias".into(), &true.into())?;
        js_sys::Reflect::set(&options, &"premultipliedAlpha".into(), &false.into())?;
        js_sys::Reflect::set(&options, &"alpha".into(), &true.into())?;
        let gl: Gl = canvas
            .get_context_with_context_options("webgl2", &options)?
            .ok_or_else(|| err("WebGL2 is unavailable"))?
            .dyn_into()?;
        let pack_prog = Program::new(&gl, PACK_VS, PACK_FS)?;
        let sprite_prog = Program::new(&gl, QUAD_VS, SPRITE_FS)?;
        let shape_prog = Program::new(&gl, QUAD_VS, SHAPE_FS)?;
        let ribbon_prog = Program::new(&gl, RIBBON_VS, RIBBON_FS)?;
        let card_prog = Program::new(&gl, CARD_VS, CARD_FS)?;
        let rim_prog = Program::new(&gl, RIM_VS, RIM_FS)?;
        let rim_mesh = Mesh::new(&gl, &rim_vertices(), 3, false)?;
        let pack_mesh = Mesh::new(&gl, &grid(110, 160), 2, false)?;
        let card_mesh = Mesh::new(&gl, &grid(8, 10), 2, false)?;
        let quad = Mesh::new(&gl, &grid(1, 1), 2, false)?;
        let ribbon_mesh = Mesh::new(&gl, &[0.0; 4], 4, true)?;
        gl.enable(Gl::BLEND);
        gl.blend_func_separate(
            Gl::SRC_ALPHA,
            Gl::ONE_MINUS_SRC_ALPHA,
            Gl::ONE,
            Gl::ONE_MINUS_SRC_ALPHA,
        );
        gl.enable(Gl::DEPTH_TEST);
        gl.depth_func(Gl::LEQUAL);
        let seed = 1;
        let mut scene = SupplyScene {
            gl,
            canvas,
            pack_prog,
            sprite_prog,
            shape_prog,
            ribbon_prog,
            card_prog,
            rim_prog,
            rim_mesh,
            pack_mesh,
            card_mesh,
            quad,
            ribbon_mesh,
            print: None,
            logo: None,
            card_back: None,
            card_front: None,
            art: (0..CHARS * LAYERS).map(|_| None).collect(),
            copy: [
                "EchoBird".into(),
                "TOKENPET".into(),
                "TEAR HERE \u{2192}".into(),
                "Supply pack".into(),
                "Pick 1 of 5".into(),
            ],
            width: 600.0,
            height: 800.0,
            dpr: 1.0,
            nx: 0.0,
            ny: 0.0,
            pointer_world: (0.0, 0.0),
            hovering: false,
            rx: Spring::default(),
            ry: Spring::default(),
            rz: Spring::default(),
            lift: Spring::default(),
            tear: tear_path(seed),
            seed,
            progress: Spring::default(),
            dir: 1.0,
            drag: None,
            moved: false,
            auto_from: None,
            opened_at: None,
            cue_sent: false,
            opened_sent: false,
            ready_sent: false,
            hover: [Spring::default(); 5],
            card_tilt: [[Spring::default(); 2]; 5],
            glare: [(0.5, 0.3); 5],
            hover_index: None,
            picked: None,
            picked_sent: false,
            reveal_at: None,
            reveal_kind: 0,
            mid_sent: false,
            done_sent: false,
            card_rx: Spring::default(),
            card_ry: Spring::default(),
            reduced: false,
            time: 0.0,
            last_ms: None,
            blink_at: 2.0,
            blink_until: 0.0,
            sparkles: Vec::new(),
            disposed: false,
        };
        scene.redraw_print()?;
        scene.redraw_back()?;
        Ok(scene)
    }

    /// Supplies one mascot layer. `character`: 0 echo, 1 sprout, 2 twin, 3 zip.
    /// `layer`: 0 body, 1 eyes, 2 joy, 3 wince, 4 smile, 5 open.
    pub fn add_art(
        &mut self,
        character: u32,
        layer: u32,
        image: &HtmlImageElement,
    ) -> Result<(), JsValue> {
        let (c, l) = (character as usize, layer as usize);
        if c >= CHARS || l >= LAYERS {
            return Err(err("Unknown art slot"));
        }
        if let Some(old) = self.art[c * LAYERS + l].take() {
            self.gl.delete_texture(Some(&old));
        }
        self.art[c * LAYERS + l] = Some(texture_from_image(&self.gl, image)?);
        Ok(())
    }

    /// Printed words on the pack: brand, kind, tear hint, pack label, count.
    pub fn set_copy(
        &mut self,
        brand: String,
        kind: String,
        hint: String,
        label: String,
        count: String,
    ) -> Result<(), JsValue> {
        self.copy = [brand, kind, hint, label, count];
        self.redraw_print()?;
        self.redraw_back()
    }

    /// EchoBird's bird mark, drawn on the pack and as the card-back emblem.
    pub fn set_logo(&mut self, image: HtmlImageElement) -> Result<(), JsValue> {
        self.logo = Some(image);
        self.redraw_print()?;
        self.redraw_back()
    }

    pub fn resize(&mut self, css_width: f32, css_height: f32, dpr: f32) {
        self.width = css_width.max(1.0);
        self.height = css_height.max(1.0);
        self.dpr = dpr.clamp(1.0, 3.0);
        self.canvas.set_width((self.width * self.dpr) as u32);
        self.canvas.set_height((self.height * self.dpr) as u32);
    }

    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced = reduced;
    }

    /// Starts over with a fresh, sealed pack (preview and retries).
    pub fn reset(&mut self, seed: u32) {
        self.seed = seed as u64;
        self.tear = tear_path(self.seed);
        self.progress = Spring::default();
        self.drag = None;
        self.auto_from = None;
        self.opened_at = None;
        self.cue_sent = false;
        self.opened_sent = false;
        self.ready_sent = false;
        self.hover = [Spring::default(); 5];
        self.hover_index = None;
        self.picked = None;
        self.picked_sent = false;
        self.reveal_at = None;
        self.mid_sent = false;
        self.done_sent = false;
        self.sparkles.clear();
        if let Some(t) = self.card_front.take() {
            self.gl.delete_texture(Some(&t));
        }
    }

    /// Skips straight to five face-down cards when the pack is still sealed
    /// (e.g. the dialog restores the choice step). Does nothing once opening,
    /// so a running tear or deal is never cut short.
    pub fn skip_to_cards(&mut self) {
        if self.opened_at.is_some() || self.auto_from.is_some() {
            return;
        }
        self.force_cards();
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.nx = (x / self.width) * 2.0 - 1.0;
        self.ny = 1.0 - (y / self.height) * 2.0;
        self.pointer_world = (self.world_x(x), self.world_y(y));
        self.hovering = true;
        if let Some((sx, _)) = self.drag {
            let dx = self.pointer_world.0 - sx;
            if dx.abs() > 0.02 {
                self.moved = true;
            }
            if self.progress.to < 0.999 {
                if self.progress.x < 0.01 && dx.abs() > 0.02 {
                    self.dir = dx.signum();
                }
                self.progress.to = clamp01(dx * self.dir / (PACK_ASPECT * 0.95));
            }
        }
        if self.choosing() {
            self.hover_index = self.card_at(self.pointer_world.0, self.pointer_world.1);
        }
    }

    /// Returns true when the press landed on something interactive.
    pub fn pointer_down(&mut self, x: f32, y: f32) -> bool {
        let (wx, wy) = (self.world_x(x), self.world_y(y));
        if self.choosing() {
            return self.card_at(wx, wy).is_some();
        }
        if self.opened_at.is_some() || self.auto_from.is_some() {
            return false;
        }
        let on_pack = wx.abs() < PACK_ASPECT * 0.55 && wy.abs() < PACK_H * 0.55;
        if on_pack {
            self.drag = Some((wx, wy));
            self.moved = false;
        }
        on_pack
    }

    /// What a release did: 0 nothing, 1 a click on the sealed pack (the
    /// caller opens it), 2 a drag that tore far enough (the tear now finishes
    /// by itself). A click on a face-down card picks it (see `EV_PICKED`).
    /// Call it from the pointerup handler so audio can unlock in the gesture.
    pub fn pointer_up(&mut self, x: f32, y: f32) -> u32 {
        if self.choosing() {
            if let Some(i) = self.card_at(self.world_x(x), self.world_y(y)) {
                self.pick(i as u32);
            }
            return 0;
        }
        let Some(_) = self.drag.take() else { return 0 };
        if !self.moved {
            return 1;
        }
        if self.progress.to >= 0.35 {
            self.finish_tear();
            2
        } else {
            self.progress.to = 0.0;
            0
        }
    }

    /// After the tear is committed: [ms until the tear sound, ms until the
    /// five cards are ready]. Zero means it has already happened.
    pub fn timing(&self) -> Vec<f64> {
        let speed = if self.reduced { 4.0 } else { 1.0 };
        let ready_age = 2.0 / speed;
        if let Some(age) = self.opened_age() {
            return vec![0.0, ((ready_age - age).max(0.0) * 1000.0) as f64];
        }
        let Some((from_t, from_p)) = self.auto_from else {
            return vec![0.0, 0.0];
        };
        let span = AUTO_TEAR_SECONDS * (1.0 - from_p).max(0.3);
        let elapsed = self.time - from_t;
        // Solve smooth(t) for the cue progress by bisection.
        let mut cue = 0.0;
        if from_p < TEAR_CUE_PROGRESS {
            let target = (TEAR_CUE_PROGRESS - from_p) / (1.0 - from_p);
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..24 {
                let mid = (lo + hi) / 2.0;
                if smooth(mid) < target {
                    lo = mid
                } else {
                    hi = mid
                }
            }
            cue = (hi * span - elapsed).max(0.0);
        }
        let ready = (span - elapsed).max(0.0) + ready_age;
        vec![(cue * 1000.0) as f64, (ready * 1000.0) as f64]
    }

    pub fn leave(&mut self) {
        self.hovering = false;
        self.nx = 0.0;
        self.ny = 0.0;
        if self.choosing() {
            self.hover_index = None;
        }
        if self.drag.take().is_some() && self.progress.to < 0.35 {
            self.progress.to = 0.0;
        }
    }

    /// Opens the pack without dragging (click, Enter, Space).
    pub fn open(&mut self) {
        if self.opened_at.is_some() || self.auto_from.is_some() {
            return;
        }
        if self.reduced {
            self.progress = Spring {
                x: 1.0,
                v: 0.0,
                to: 1.0,
            };
            self.opened_at = Some(self.time);
            return;
        }
        self.finish_tear();
    }

    /// Keyboard focus on a face-down card (preview only; nothing is revealed).
    pub fn focus(&mut self, index: i32) {
        if self.choosing() {
            self.hover_index = (0..5).contains(&index).then_some(index as usize);
        }
    }

    /// Confirms one face-down card. Only the first pick counts.
    pub fn pick(&mut self, index: u32) {
        if self.picked.is_some() || index >= 5 {
            return;
        }
        if !self.ready_sent {
            // Picked through the accessible buttons or a reduced-motion path
            // before the deal finished: show the five cards at once.
            self.force_cards();
            self.ready_sent = true;
        }
        self.picked = Some(index as usize);
        self.hover_index = Some(index as usize);
    }

    /// The picked slot, or -1.
    pub fn picked_index(&self) -> i32 {
        self.picked.map(|i| i as i32).unwrap_or(-1)
    }

    /// Shows the real reward on the picked card and flips it.
    /// `kind`: 0 growth, 1 double, 2 haste. The words come from the app.
    pub fn reveal(
        &mut self,
        kind: u32,
        name: String,
        value: String,
        detail: String,
        badge: String,
    ) -> Result<(), JsValue> {
        if self.picked.is_none() || self.reveal_at.is_some() {
            return Ok(());
        }
        self.reveal_kind = (kind as usize).min(DESIGNS - 1);
        self.redraw_front(&name, &value, &detail, &badge)?;
        self.reveal_at = Some(self.time);
        Ok(())
    }

    /// Shows an already revealed reward with no pack or choice (restoring a saved state).
    pub fn show_revealed(
        &mut self,
        kind: u32,
        name: String,
        value: String,
        detail: String,
        badge: String,
    ) -> Result<(), JsValue> {
        self.force_cards();
        self.picked = Some(2);
        self.reveal_kind = (kind as usize).min(DESIGNS - 1);
        self.redraw_front(&name, &value, &detail, &badge)?;
        self.reveal_at = Some(self.time - 10.0);
        self.mid_sent = true;
        self.done_sent = true;
        self.picked_sent = true;
        self.ready_sent = true;
        Ok(())
    }

    /// Advances and draws one frame. Returns event bits (see EV_*).
    pub fn frame(&mut self, now_ms: f64) -> u32 {
        if self.disposed {
            return 0;
        }
        let dt = match self.last_ms {
            Some(prev) => ((now_ms - prev) / 1000.0).clamp(0.0, 0.05) as f32,
            None => 0.0,
        };
        self.last_ms = Some(now_ms);
        self.time += dt;
        let mut events = 0;
        let speed = if self.reduced { 4.0 } else { 1.0 };

        if let Some((from_t, from_p)) = self.auto_from {
            let t = clamp01((self.time - from_t) / (AUTO_TEAR_SECONDS * (1.0 - from_p).max(0.3)));
            self.progress.x = from_p + (1.0 - from_p) * smooth(t);
            if t >= 1.0 && self.opened_at.is_none() {
                self.opened_at = Some(self.time);
                self.burst();
            }
        } else {
            self.progress.step(dt, 260.0, 30.0);
            self.progress.x = self.progress.x.clamp(0.0, 1.0);
        }
        if !self.cue_sent && self.progress.x >= TEAR_CUE_PROGRESS {
            self.cue_sent = true;
            events |= EV_TEAR_CUE;
        }
        if let Some(age) = self.opened_age() {
            if !self.opened_sent && age * speed > 0.9 {
                self.opened_sent = true;
                events |= EV_OPENED;
            }
            if !self.ready_sent && age * speed > 2.0 {
                self.ready_sent = true;
                events |= EV_READY;
            }
        }
        if self.picked.is_some() && !self.picked_sent {
            self.picked_sent = true;
            events |= EV_PICKED;
        }
        if let Some(at) = self.reveal_at {
            let t = (self.time - at) * speed / REVEAL_SECONDS;
            if !self.mid_sent && t >= 0.55 {
                self.mid_sent = true;
                events |= EV_REVEAL_MID;
            }
            if !self.done_sent && t >= 1.0 {
                self.done_sent = true;
                events |= EV_REVEAL_DONE;
            }
        }

        // Tilt toward the pointer; lean away from the pull while tearing.
        let motion = if self.reduced { 0.3 } else { 1.0 };
        let (tx, ty) = if self.hovering {
            (self.nx, self.ny)
        } else {
            (0.0, 0.0)
        };
        self.ry.to = tx * 0.38 * motion;
        self.rx.to = -ty * 0.26 * motion;
        let pulling = self.progress.x > 0.0 && self.opened_at.is_none();
        self.rz.to = if pulling {
            -self.dir * self.progress.x * 0.06
        } else {
            0.0
        } + (self.time * 0.9).sin() * 0.012 * motion;
        self.lift.to = (self.time * 1.4).sin() * 0.012 * motion;
        for s in [&mut self.rx, &mut self.ry, &mut self.rz] {
            s.step(dt, 90.0, 13.0);
        }
        self.lift.step(dt, 60.0, 12.0);
        for (i, h) in self.hover.iter_mut().enumerate() {
            h.to = if self.hover_index == Some(i) {
                1.0
            } else {
                0.0
            };
            h.step(dt, 170.0, 19.0);
        }
        // A hovered back leans toward where the pointer is on it, so that
        // corner presses away; the others waft gently, as if in moving air.
        let spacing = self.spacing();
        let (px, py) = self.pointer_world;
        let waft = if self.reduced { 0.0 } else { 1.0 };
        for i in 0..5 {
            let (sx, sy, _) = fan_slot(i, spacing);
            let (w, h) = (CARD_W * FAN_SCALE, CARD_H * FAN_SCALE);
            let hx = ((px - sx) / w + 0.5).clamp(0.0, 1.0);
            let cy = self.fan_y() + sy + 0.09 * self.hover[i].x;
            let hy = (0.5 - (py - cy) / h).clamp(0.0, 1.0);
            let on = self.hover_index == Some(i) && self.choosing();
            let phase = self.time + i as f32 * 1.7;
            let (pitch, yaw) = if on {
                self.glare[i] = (hx, hy);
                (
                    (0.5 - hy) * 0.6 * if self.reduced { 0.5 } else { 1.0 },
                    (hx - 0.5) * 0.68 * if self.reduced { 0.5 } else { 1.0 },
                )
            } else {
                (
                    (phase * 0.9).cos() * 0.035 * waft,
                    phase.sin() * 0.06 * waft,
                )
            };
            self.card_tilt[i][0].to = pitch;
            self.card_tilt[i][1].to = yaw;
            for t in &mut self.card_tilt[i] {
                t.step(dt, 110.0, 13.0);
            }
        }
        // The revealed card turns toward the pointer.
        self.card_ry.to = tx * 0.42 * motion;
        self.card_rx.to = -ty * 0.3 * motion;
        self.card_rx.step(dt, 80.0, 12.0);
        self.card_ry.step(dt, 80.0, 12.0);

        if self.time > self.blink_at {
            self.blink_until = self.time + 0.14;
            self.blink_at = self.time + 2.2 + Rng::new((self.time * 1000.0) as u64).range(0.0, 3.0);
        }
        for s in &mut self.sparkles {
            s.vel[1] -= 1.6 * dt;
            for (p, v) in s.pos.iter_mut().zip(s.vel) {
                *p += v * dt;
            }
        }

        self.draw();
        events
    }

    /// Stops drawing and frees every GPU object this scene created.
    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        let gl = &self.gl;
        for p in [
            &self.pack_prog,
            &self.sprite_prog,
            &self.shape_prog,
            &self.ribbon_prog,
            &self.card_prog,
            &self.rim_prog,
        ] {
            p.delete(gl);
        }
        for m in [
            &self.pack_mesh,
            &self.card_mesh,
            &self.rim_mesh,
            &self.quad,
            &self.ribbon_mesh,
        ] {
            m.delete(gl);
        }
        for t in [
            self.print.take(),
            self.card_back.take(),
            self.card_front.take(),
        ]
        .into_iter()
        .flatten()
        {
            gl.delete_texture(Some(&t));
        }
        for t in self.art.iter_mut().filter_map(Option::take) {
            gl.delete_texture(Some(&t));
        }
    }
}

impl SupplyScene {
    fn force_cards(&mut self) {
        self.progress = Spring {
            x: 1.0,
            v: 0.0,
            to: 1.0,
        };
        self.auto_from = None;
        self.opened_at = Some(self.time - 10.0);
        self.cue_sent = true;
        self.opened_sent = true;
    }

    fn fov(&self) -> f32 {
        let aspect = self.width / self.height;
        // Keep things the same size in narrow windows.
        if aspect < 0.75 {
            2.0 * ((FOV / 2.0).tan() * 0.75 / aspect).atan()
        } else {
            FOV
        }
    }

    fn view_proj(&self) -> Mat4 {
        Mat4::perspective(self.fov(), self.width / self.height, 0.1, 20.0)
            .mul(&Mat4::translate(0.0, 0.0, -EYE_Z))
    }

    fn half_extent(&self) -> (f32, f32) {
        let h = (self.fov() / 2.0).tan() * EYE_Z;
        (h * self.width / self.height, h)
    }

    fn world_x(&self, css_x: f32) -> f32 {
        ((css_x / self.width) * 2.0 - 1.0) * self.half_extent().0
    }

    fn world_y(&self, css_y: f32) -> f32 {
        (1.0 - (css_y / self.height) * 2.0) * self.half_extent().1
    }

    fn opened_age(&self) -> Option<f32> {
        self.opened_at.map(|at| self.time - at)
    }

    /// Five backs are fanned out and nothing is picked yet.
    fn choosing(&self) -> bool {
        self.ready_sent && self.picked.is_none()
    }

    fn spacing(&self) -> f32 {
        // Visible half width at the fan's depth, so the outer cards stay on screen.
        let at_depth = self.half_extent().0 * (EYE_Z - FAN_Z) / EYE_Z;
        fan_spacing(at_depth, FAN_SCALE)
    }

    fn card_at(&self, wx: f32, wy: f32) -> Option<usize> {
        let half_h = CARD_H * FAN_SCALE * 0.5;
        if (wy - self.fan_y()).abs() > half_h + 0.05 {
            return None;
        }
        slot_at(wx, self.spacing(), CARD_W * FAN_SCALE)
    }

    fn fan_y(&self) -> f32 {
        -0.02
    }

    fn pack_model(&self) -> Mat4 {
        let speed = if self.reduced { 4.0 } else { 1.0 };
        let drop = self
            .opened_age()
            .map(|a| ease_out((a * speed - 0.95) / 0.6) * 1.5)
            .unwrap_or(0.0);
        self.pack_model_dropped(drop)
    }

    /// The pack's transform without its exit drop: ribbons and sparkles stay
    /// where the pack burst open instead of sinking with it.
    fn pack_model_dropped(&self, drop: f32) -> Mat4 {
        Mat4::translate(0.0, 0.03 + self.lift.x - drop, 0.0)
            .mul(&Mat4::rot_y(self.ry.x))
            .mul(&Mat4::rot_x(self.rx.x))
            .mul(&Mat4::rot_z(self.rz.x))
    }

    /// The plane through the pack's torn opening, facing up the pack, in
    /// world space as (normal, offset). Once the pack is gone nothing clips.
    fn opening_plane(&self) -> [f32; 4] {
        if self.pack_alpha() <= 0.0 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        // The highest point of the tear, so no card shows over the front lip.
        let depth = self.tear.iter().copied().fold(f32::MAX, f32::min);
        let pack = self.pack_model();
        let o = pack.point([0.0, 0.5 - depth, 0.0]);
        let up = pack.point([0.0, 1.5 - depth, 0.0]);
        let n = [up[0] - o[0], up[1] - o[1], up[2] - o[2]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        let n = [n[0] / len, n[1] / len, n[2] / len];
        [n[0], n[1], n[2], -(n[0] * o[0] + n[1] * o[1] + n[2] * o[2])]
    }

    fn pack_alpha(&self) -> f32 {
        let speed = if self.reduced { 4.0 } else { 1.0 };
        self.opened_age()
            .map(|a| 1.0 - smooth((a * speed - 1.05) / 0.45))
            .unwrap_or(1.0)
    }

    fn finish_tear(&mut self) {
        self.auto_from = Some((self.time, self.progress.x));
        self.drag = None;
    }

    fn burst(&mut self) {
        if self.reduced {
            return;
        }
        let mut rng = Rng::new(self.seed ^ 0xA5A5);
        self.sparkles = (0..26)
            .map(|i| {
                let a = rng.range(-1.1, 1.1);
                let speed = rng.range(0.6, 1.4);
                Sparkle {
                    pos: [rng.range(-0.3, 0.3), 0.38, 0.05],
                    vel: [a.sin() * speed * 0.6, a.cos() * speed, rng.range(0.1, 0.5)],
                    size: rng.range(0.025, 0.06),
                    color: RIBBON_COLORS[i % RIBBON_COLORS.len()],
                }
            })
            .collect();
    }

    /// Where every visible card is at this moment, back to front.
    fn card_poses(&self) -> Vec<CardPose> {
        let Some(age) = self.opened_age() else {
            return Vec::new();
        };
        let speed = if self.reduced { 4.0 } else { 1.0 };
        let a = age * speed;
        if a < 0.35 {
            return Vec::new();
        }
        let spacing = self.spacing();
        let pack = self.pack_model();
        let mut poses = Vec::with_capacity(5);
        for i in 0..5 {
            // 1) Rise out of the opening as a stack (inside the pack, behind its front).
            let rise = ease_out((a - 0.35) / 0.6);
            let inside = pack.point([
                0.0,
                -0.42 + 0.72 * rise + i as f32 * 0.004,
                0.01 - i as f32 * 0.002,
            ]);
            // 2) Move forward to the centre while the pack sinks away, then fan out.
            let gather = smooth((a - 0.9) / 0.45);
            let fan = ease_back((a - 1.2 - i as f32 * 0.06) / 0.65);
            let (sx, sy, roll) = fan_slot(i, spacing);
            let centre = [0.0, self.fan_y() + 0.05, FAN_Z + 0.2];
            let slot = [sx, self.fan_y() + sy, FAN_Z + i as f32 * 0.001];
            let mut pos = [0.0; 3];
            for (k, p) in pos.iter_mut().enumerate() {
                let stacked = inside[k] + (centre[k] - inside[k]) * gather;
                *p = stacked + (slot[k] - stacked) * fan;
            }
            let scale = 1.0 + (FAN_SCALE - 1.0) * gather;
            let mut pose = CardPose {
                index: i,
                pos,
                roll: roll * fan,
                yaw: std::f32::consts::PI + self.ry.x * (1.0 - gather),
                pitch: self.rx.x * (1.0 - gather),
                scale,
                alpha: 1.0,
            };
            // Hover lifts the card in place and leans it toward the pointer.
            let h = self.hover[i].x;
            pose.pos[1] += 0.09 * h;
            pose.pos[2] += 0.22 * h;
            pose.roll *= 1.0 - h;
            pose.yaw += self.card_tilt[i][1].x * fan;
            pose.pitch += self.card_tilt[i][0].x * fan;

            if let (Some(picked), Some(at)) = (self.picked, self.reveal_at) {
                let t = (self.time - at) * speed / REVEAL_SECONDS;
                if i == picked {
                    let travel = ease_out(t / 0.6);
                    let target = [0.0, 0.0, 0.55];
                    for (p, goal) in pose.pos.iter_mut().zip(target) {
                        *p += (goal - *p) * travel;
                    }
                    pose.roll *= 1.0 - travel;
                    pose.scale += (REVEAL_SCALE - pose.scale) * travel;
                    let flip = smooth((t - 0.28) / 0.55);
                    pose.yaw = std::f32::consts::PI * (1.0 + flip) + self.card_ry.x * flip;
                    pose.pitch = self.card_rx.x * flip;
                    // A small hop at the middle of the flip.
                    pose.pos[1] += (flip * std::f32::consts::PI).sin() * 0.06;
                } else {
                    let delay = (i as f32 - picked as f32).abs() * 0.05;
                    let out = smooth((t - delay) / 0.55);
                    pose.pos[1] -= 1.5 * out * out;
                    pose.alpha = 1.0 - out;
                }
            }
            poses.push(pose);
        }
        poses.sort_by(|a, b| {
            a.pos[2]
                .partial_cmp(&b.pos[2])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        poses
    }

    fn card_model(&self, pose: &CardPose) -> Mat4 {
        Mat4::translate(pose.pos[0], pose.pos[1], pose.pos[2])
            .mul(&Mat4::rot_z(pose.roll))
            .mul(&Mat4::rot_y(pose.yaw))
            .mul(&Mat4::rot_x(pose.pitch))
            .mul(&Mat4::scale(pose.scale, pose.scale, pose.scale))
    }

    fn redraw_print(&mut self) -> Result<(), JsValue> {
        let (w, h) = (1024u32, (1024.0 / PACK_ASPECT) as u32);
        let canvas = offscreen_canvas(w, h)?;
        let ctx: CanvasRenderingContext2d = canvas
            .get_context("2d")?
            .ok_or_else(|| err("No 2D context"))?
            .dyn_into()?;
        let (w, h) = (w as f64, h as f64);
        ctx.set_text_baseline("alphabetic");
        ctx.set_fill_style_str("rgba(255,255,255,0.55)");
        ctx.set_font(&format!("600 26px {FONT}"));
        letter_spacing(&ctx, "4px");
        ctx.fill_text(&self.copy[2], w * 0.075, h * 0.098)?;
        letter_spacing(&ctx, "0px");
        ctx.set_fill_style_str("#ffffff");
        // The wordmark fills the width beside the mark, shrinking for long names.
        let logo_room = if self.logo.is_some() { 132.0 } else { 0.0 };
        ctx.set_font(&format!("800 118px {FONT}"));
        let natural = ctx.measure_text(&self.copy[0])?.width();
        let brand_px = (118.0 * (w * 0.86 - logo_room) / natural.max(1.0)).min(118.0);
        ctx.set_font(&format!("800 {brand_px:.0}px {FONT}"));
        let mut brand_x = w * 0.07;
        if let Some(logo) = &self.logo {
            // The bird mark, about the height of the wordmark.
            let size = 128.0;
            ctx.draw_image_with_html_image_element_and_dw_and_dh(
                logo,
                brand_x - 8.0,
                h * 0.26 - 108.0,
                size,
                size,
            )?;
            brand_x += size + 4.0;
        }
        ctx.fill_text(&self.copy[0], brand_x, h * 0.26)?;
        ctx.set_fill_style_str("rgba(255,255,255,0.72)");
        ctx.set_font(&format!("700 30px {FONT}"));
        letter_spacing(&ctx, "5px");
        ctx.set_text_align("right");
        // The kind of pack sits on its own line under the wordmark.
        ctx.fill_text(&self.copy[1], w * 0.93, h * 0.305)?;
        letter_spacing(&ctx, "0px");
        ctx.set_font(&format!("600 40px {FONT}"));
        ctx.set_fill_style_str("rgba(255,255,255,0.9)");
        ctx.set_text_align("left");
        ctx.fill_text(&self.copy[3], w * 0.075, h * 0.895)?;
        ctx.set_text_align("right");
        ctx.fill_text(&self.copy[4], w * 0.925, h * 0.895)?;
        if let Some(old) = self.print.take() {
            self.gl.delete_texture(Some(&old));
        }
        self.print = Some(texture_from_canvas(&self.gl, &canvas)?);
        Ok(())
    }

    /// The card back: a thin inset border and the brand, quiet on black.
    fn redraw_back(&mut self) -> Result<(), JsValue> {
        let (w, h) = (600u32, 840u32);
        let canvas = offscreen_canvas(w, h)?;
        let ctx: CanvasRenderingContext2d = canvas
            .get_context("2d")?
            .ok_or_else(|| err("No 2D context"))?
            .dyn_into()?;
        let (w, h) = (w as f64, h as f64);
        ctx.set_stroke_style_str("rgba(255,255,255,0.16)");
        ctx.set_line_width(3.0);
        rounded_rect(&ctx, 26.0, 26.0, w - 52.0, h - 52.0, 26.0);
        ctx.stroke();
        ctx.set_stroke_style_str("rgba(255,255,255,0.07)");
        ctx.set_line_width(2.0);
        rounded_rect(&ctx, 40.0, 40.0, w - 80.0, h - 80.0, 18.0);
        ctx.stroke();
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        if let Some(logo) = &self.logo {
            // The orange EchoBird mark as the emblem, with the name beneath.
            let size = w * 0.5;
            ctx.draw_image_with_html_image_element_and_dw_and_dh(
                logo,
                (w - size) / 2.0,
                h * 0.5 - size * 0.62,
                size,
                size,
            )?;
            ctx.set_fill_style_str("rgba(255,255,255,0.42)");
            ctx.set_font(&format!("800 34px {FONT}"));
            ctx.fill_text(&self.copy[0], w * 0.5, h * 0.5 + size * 0.5)?;
            ctx.set_font(&format!("700 16px {FONT}"));
            letter_spacing(&ctx, "6px");
            ctx.set_fill_style_str("rgba(255,255,255,0.24)");
            ctx.fill_text(&self.copy[1], w * 0.5 + 3.0, h * 0.5 + size * 0.5 + 38.0)?;
            letter_spacing(&ctx, "0px");
        } else {
            ctx.set_fill_style_str("rgba(255,255,255,0.34)");
            ctx.set_font(&format!("800 54px {FONT}"));
            ctx.fill_text(&self.copy[0], w * 0.5, h * 0.5)?;
            ctx.set_font(&format!("700 18px {FONT}"));
            letter_spacing(&ctx, "6px");
            ctx.set_fill_style_str("rgba(255,255,255,0.22)");
            ctx.fill_text(&self.copy[1], w * 0.5, h * 0.5 + 52.0)?;
            letter_spacing(&ctx, "0px");
        }
        if let Some(old) = self.card_back.take() {
            self.gl.delete_texture(Some(&old));
        }
        self.card_back = Some(texture_from_canvas(&self.gl, &canvas)?);
        Ok(())
    }

    /// The card front for the real reward: body colour, art window, words.
    fn redraw_front(
        &mut self,
        name: &str,
        value: &str,
        detail: &str,
        badge: &str,
    ) -> Result<(), JsValue> {
        let design = self.reveal_kind;
        let (w, h) = (600u32, 840u32);
        let canvas = offscreen_canvas(w, h)?;
        let ctx: CanvasRenderingContext2d = canvas
            .get_context("2d")?
            .ok_or_else(|| err("No 2D context"))?
            .dyn_into()?;
        let (w, h) = (w as f64, h as f64);
        // (body top, body bottom, window top, window bottom, ink, soft ink)
        let palette: [(&str, &str, &str, &str, &str, &str); DESIGNS] = [
            (
                "#eefbd6",
                "#c9ec9b",
                "#1f7d5d",
                "#155c46",
                "#1c2b12",
                "rgba(28,43,18,0.6)",
            ),
            (
                "#fff1b0",
                "#ffb954",
                "#ff6aa9",
                "#ff9b45",
                "#4a2300",
                "rgba(74,35,0,0.6)",
            ),
            (
                "#4b4fd0",
                "#5a27a8",
                "#1b2a8a",
                "#2c1a78",
                "#ffffff",
                "rgba(255,255,255,0.7)",
            ),
            (
                "#ffe3e8",
                "#ffb4c6",
                "#ff6f9a",
                "#ffb067",
                "#4a1024",
                "rgba(74,16,36,0.6)",
            ),
            (
                "#e6f2ff",
                "#b8d3ff",
                "#4f86ff",
                "#a07cff",
                "#10265a",
                "rgba(16,38,90,0.6)",
            ),
        ];
        let (b0, b1, w0, w1, ink, soft) = palette[design];
        let body = ctx.create_linear_gradient(0.0, 0.0, w, h);
        body.add_color_stop(0.0, b0)?;
        body.add_color_stop(1.0, b1)?;
        ctx.set_fill_style_canvas_gradient(&body);
        ctx.fill_rect(0.0, 0.0, w, h);

        // Art window with a pattern that suits the reward.
        let (wx, wy, ww, wh) = (40.0, 40.0, w - 80.0, h * 0.53);
        ctx.save();
        rounded_rect(&ctx, wx, wy, ww, wh, 26.0);
        ctx.clip();
        let win = ctx.create_linear_gradient(wx, wy, wx + ww, wy + wh);
        win.add_color_stop(0.0, w0)?;
        win.add_color_stop(1.0, w1)?;
        ctx.set_fill_style_canvas_gradient(&win);
        ctx.fill_rect(wx, wy, ww, wh);
        let (cx, cy) = (wx + ww / 2.0, wy + wh / 2.0);
        match design {
            0 => {
                // Growth: a fine grid, like graph paper for a growing chart.
                ctx.set_stroke_style_str("rgba(255,255,255,0.10)");
                ctx.set_line_width(1.5);
                let mut x = wx;
                while x < wx + ww {
                    ctx.begin_path();
                    ctx.move_to(x, wy);
                    ctx.line_to(x - wh * 0.4, wy + wh);
                    ctx.stroke();
                    ctx.begin_path();
                    ctx.move_to(x - wh * 0.4, wy);
                    ctx.line_to(x, wy + wh);
                    ctx.stroke();
                    x += 34.0;
                }
            }
            1 => {
                // Double: warm rays from the centre.
                ctx.set_fill_style_str("rgba(255,255,255,0.18)");
                for k in 0..24 {
                    let a0 = k as f64 / 24.0 * std::f64::consts::TAU;
                    let a1 = a0 + std::f64::consts::TAU / 48.0;
                    ctx.begin_path();
                    ctx.move_to(cx, cy);
                    ctx.line_to(cx + a0.cos() * 900.0, cy + a0.sin() * 900.0);
                    ctx.line_to(cx + a1.cos() * 900.0, cy + a1.sin() * 900.0);
                    ctx.close_path();
                    ctx.fill();
                }
            }
            3 => {
                // Rose: soft rings spreading from the centre.
                ctx.set_line_width(14.0);
                for k in 1..18 {
                    ctx.set_stroke_style_str(if k % 2 == 0 {
                        "rgba(255,255,255,0.16)"
                    } else {
                        "rgba(255,255,255,0.06)"
                    });
                    ctx.begin_path();
                    let _ = ctx.arc(cx, cy, k as f64 * 22.0, 0.0, std::f64::consts::TAU);
                    ctx.stroke();
                }
            }
            4 => {
                // Sky: rising bubbles of different sizes.
                let mut rng = Rng::new(29);
                for _ in 0..34 {
                    let r = rng.range(4.0, 22.0) as f64;
                    let (x, y) = (
                        wx + rng.next_f32() as f64 * ww,
                        wy + rng.next_f32() as f64 * wh,
                    );
                    ctx.set_stroke_style_str("rgba(255,255,255,0.35)");
                    ctx.set_fill_style_str("rgba(255,255,255,0.08)");
                    ctx.set_line_width(2.0);
                    ctx.begin_path();
                    let _ = ctx.arc(x, y, r, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                    ctx.stroke();
                }
            }
            _ => {
                // Haste: a field of stars.
                let mut rng = Rng::new(17);
                for _ in 0..90 {
                    let r = rng.range(0.8, 2.6) as f64;
                    ctx.set_fill_style_str(&format!(
                        "rgba(255,255,255,{:.2})",
                        rng.range(0.35, 0.95)
                    ));
                    ctx.begin_path();
                    let _ = ctx.arc(
                        wx + rng.next_f32() as f64 * ww,
                        wy + rng.next_f32() as f64 * wh,
                        r,
                        0.0,
                        std::f64::consts::TAU,
                    );
                    ctx.fill();
                }
            }
        }
        // The window sits pressed into the card: shade along its top and sides.
        let top = ctx.create_linear_gradient(0.0, wy, 0.0, wy + 30.0);
        top.add_color_stop(0.0, "rgba(0,0,0,0.34)")?;
        top.add_color_stop(1.0, "rgba(0,0,0,0)")?;
        ctx.set_fill_style_canvas_gradient(&top);
        ctx.fill_rect(wx, wy, ww, 30.0);
        for (x0, x1) in [(wx, wx + 16.0), (wx + ww, wx + ww - 16.0)] {
            let side = ctx.create_linear_gradient(x0, 0.0, x1, 0.0);
            side.add_color_stop(0.0, "rgba(0,0,0,0.18)")?;
            side.add_color_stop(1.0, "rgba(0,0,0,0)")?;
            ctx.set_fill_style_canvas_gradient(&side);
            ctx.fill_rect(x0.min(x1), wy, 16.0, wh);
        }
        ctx.restore();
        ctx.set_stroke_style_str("rgba(0,0,0,0.16)");
        ctx.set_line_width(2.0);
        rounded_rect(&ctx, wx, wy, ww, wh, 26.0);
        ctx.stroke();
        // and its lower lip catches the light.
        ctx.set_stroke_style_str("rgba(255,255,255,0.55)");
        ctx.set_line_width(2.0);
        ctx.begin_path();
        ctx.move_to(wx + 26.0, wy + wh + 2.0);
        ctx.line_to(wx + ww - 26.0, wy + wh + 2.0);
        ctx.stroke();

        // Name and badge.
        ctx.set_text_baseline("alphabetic");
        ctx.set_text_align("left");
        ctx.set_fill_style_str(ink);
        ctx.set_font(&format!("800 18px {FONT}"));
        let reserved_badge = ctx.measure_text(badge)?.width() + 54.0;
        ctx.set_font(&format!("800 58px {FONT}"));
        let title_width = ctx.measure_text(name)?.width();
        let title_size =
            (58.0 * ((w - 96.0 - reserved_badge) / title_width.max(1.0)).min(1.0)).max(14.0);
        ctx.set_font(&format!("800 {title_size:.1}px {FONT}"));
        ctx.fill_text(name, 48.0, h * 0.68)?;
        ctx.set_font(&format!("800 18px {FONT}"));
        letter_spacing(&ctx, "3px");
        let badge_w = ctx.measure_text(badge)?.width() + 30.0;
        ctx.set_fill_style_str("rgba(255,255,255,0.45)");
        rounded_rect(
            &ctx,
            w - 48.0 - badge_w,
            h * 0.68 - 34.0,
            badge_w,
            40.0,
            20.0,
        );
        ctx.fill();
        ctx.set_fill_style_str(ink);
        ctx.set_text_align("center");
        ctx.fill_text(badge, w - 48.0 - badge_w / 2.0 + 1.5, h * 0.68 - 7.0)?;
        letter_spacing(&ctx, "0px");
        // Divider, then the amount and what it does.
        ctx.set_fill_style_str(soft);
        ctx.fill_rect(48.0, h * 0.735, w - 96.0, 2.0);
        ctx.set_text_align("left");
        ctx.set_fill_style_str(ink);
        ctx.set_font(&format!("800 66px {FONT}"));
        let value_size = 66.0 * ((w - 96.0) / ctx.measure_text(value)?.width().max(1.0)).min(1.0);
        ctx.set_font(&format!("800 {value_size:.1}px {FONT}"));
        ctx.fill_text(value, 48.0, h * 0.85)?;
        ctx.set_fill_style_str(soft);
        ctx.set_font(&format!("600 26px {FONT}"));
        let detail_size =
            26.0 * ((w - 100.0) / ctx.measure_text(detail)?.width().max(1.0)).min(1.0);
        ctx.set_font(&format!("600 {detail_size:.1}px {FONT}"));
        ctx.fill_text(detail, 50.0, h * 0.915)?;

        if let Some(old) = self.card_front.take() {
            self.gl.delete_texture(Some(&old));
        }
        self.card_front = Some(texture_from_canvas(&self.gl, &canvas)?);
        Ok(())
    }

    fn draw(&mut self) {
        let gl = self.gl.clone();
        gl.viewport(
            0,
            0,
            self.canvas.width() as i32,
            self.canvas.height() as i32,
        );
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);
        let vp = self.view_proj();
        let pack_alpha = self.pack_alpha();
        let poses = self.card_poses();
        let cards_in_front = self
            .opened_age()
            .map(|a| a * if self.reduced { 4.0 } else { 1.0 } > 0.95)
            .unwrap_or(false);

        if pack_alpha > 0.0 {
            self.draw_shadow(&gl, &vp, pack_alpha);
        }
        if pack_alpha > 0.0 {
            self.draw_pack(&gl, &vp, pack_alpha);
        }
        // Cards draw over the pack but are cut at its opening, which sinks
        // with the pack: they slide out of the slit instead of appearing on
        // its front.
        gl.clear(Gl::DEPTH_BUFFER_BIT);
        if cards_in_front {
            self.draw_card_shadows(&gl, &vp, &poses);
        }
        self.draw_cards(&gl, &vp, &poses, cards_in_front);
        if let Some(age) = self.opened_age() {
            gl.depth_mask(false);
            let model = self.pack_model_dropped(0.0);
            self.draw_ribbons(&gl, &vp, &model, age);
            self.draw_sparkles(&gl, &vp, age);
            gl.depth_mask(true);
        }
    }

    fn draw_shadow(&mut self, gl: &Gl, vp: &Mat4, alpha: f32) {
        gl.disable(Gl::DEPTH_TEST);
        self.shape_prog.bind(gl);
        self.shape_prog.mat4(gl, "u_vp", vp);
        let ground = Mat4::translate(0.0, -0.62, -0.2).mul(&Mat4::rot_x(-1.35));
        self.shape_prog.mat4(gl, "u_model", &ground);
        self.shape_prog.f4(
            gl,
            "u_rect",
            [0.0, 0.0, PACK_ASPECT * 1.15 + self.ry.x.abs() * 0.2, 0.32],
        );
        self.shape_prog.f4(
            gl,
            "u_color",
            [0.0, 0.0, 0.0, (0.32 - self.lift.x * 2.0) * alpha],
        );
        self.shape_prog.f1(gl, "u_shape", 0.0);
        self.quad.draw(gl, Gl::TRIANGLES);
        gl.enable(Gl::DEPTH_TEST);
    }

    fn draw_pack(&mut self, gl: &Gl, vp: &Mat4, alpha: f32) {
        let model = self.pack_model();
        let fly = self
            .opened_at
            .map(|at| clamp01((self.time - at) / 0.7))
            .unwrap_or(0.0);
        self.pack_prog.bind(gl);
        let p = &mut self.pack_prog;
        p.mat4(gl, "u_vp", vp);
        p.mat4(gl, "u_model", &model);
        p.f1(gl, "u_w", PACK_ASPECT);
        p.fv(gl, "u_tear", &self.tear);
        p.f1(gl, "u_front", self.progress.x * 1.02);
        p.f1(gl, "u_dir", self.dir);
        p.f1(gl, "u_lift", 1.0);
        p.f1(gl, "u_bulge", 0.075);
        p.f3(gl, "u_eye", [0.0, 0.0, EYE_Z]);
        p.f1(gl, "u_time", self.time);
        p.i1(gl, "u_print", 0);
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, self.print.as_ref());
        // The pack grid runs top to bottom, so its front faces wind clockwise.
        gl.front_face(Gl::CW);
        let strip = 1.0 - smooth((fly - 0.5) / 0.5);
        for (part, a, fly_amount) in [
            (2.0, alpha, 0.0),
            (0.0, alpha, 0.0),
            (1.0, strip * alpha, fly),
        ] {
            if a <= 0.001 {
                continue;
            }
            p.f1(gl, "u_part", part);
            p.f1(gl, "u_alpha", a);
            p.f1(gl, "u_fly", fly_amount);
            self.pack_mesh.draw(gl, Gl::TRIANGLES);
        }
        gl.front_face(Gl::CCW);

        // Echo on the pack front, floating just above the print.
        let tearing = self.progress.x > 0.02 && self.opened_at.is_none();
        let happy = self.opened_at.is_some();
        let eyes = if happy {
            L_JOY
        } else if tearing {
            L_WINCE
        } else {
            L_EYES
        };
        let squash = if tearing {
            1.0 - 0.04 * self.progress.x
        } else {
            1.0
        };
        let local = model
            .mul(&Mat4::translate(0.0, 0.0, 0.11))
            .mul(&Mat4::scale(1.0 / squash.max(0.9), squash, 1.0));
        let look = if eyes == L_EYES {
            [self.nx * 0.012, self.ny * 0.008]
        } else {
            [0.0, 0.0]
        };
        self.draw_character(
            gl,
            vp,
            &local,
            0,
            [0.0, -0.07],
            PACK_ASPECT * 0.78,
            eyes,
            if tearing { L_OPEN } else { L_SMILE },
            look,
            alpha,
        );
    }

    /// Draws a mascot's layers at `center` with `size` in `model` space.
    #[allow(clippy::too_many_arguments)]
    fn draw_character(
        &mut self,
        gl: &Gl,
        vp: &Mat4,
        model: &Mat4,
        character: usize,
        center: [f32; 2],
        size: f32,
        eyes: usize,
        mouth: usize,
        look: [f32; 2],
        alpha: f32,
    ) {
        let Some(body) = self.art[character * LAYERS + L_BODY].clone() else {
            return;
        };
        gl.depth_mask(false);
        self.sprite_prog.bind(gl);
        let s = &mut self.sprite_prog;
        s.mat4(gl, "u_vp", vp);
        s.mat4(gl, "u_model", model);
        s.i1(gl, "u_tex", 0);
        s.f4(gl, "u_tint", [1.0, 1.0, 1.0, alpha]);
        s.f4(gl, "u_crop", [0.0, 0.0, 1.0, 1.0]);
        s.f4(gl, "u_rect", [center[0], center[1], size, size]);
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, Some(&body));
        self.quad.draw(gl, Gl::TRIANGLES);
        if let Some(tex) = self.art[character * LAYERS + eyes].clone() {
            let blink = eyes == L_EYES && self.time < self.blink_until;
            let h = if blink { 0.12 } else { 1.0 };
            let eye_line = center[1] + size * (0.5 - 0.43);
            s.f4(
                gl,
                "u_rect",
                [
                    center[0] + look[0],
                    eye_line + (center[1] - eye_line) * h + look[1],
                    size,
                    size * h,
                ],
            );
            gl.bind_texture(Gl::TEXTURE_2D, Some(&tex));
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        if let Some(tex) = self.art[character * LAYERS + mouth].clone() {
            s.f4(gl, "u_rect", [center[0], center[1], size, size]);
            gl.bind_texture(Gl::TEXTURE_2D, Some(&tex));
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        gl.depth_mask(true);
    }

    /// The character's body as a soft dark shape, drawn a few times slightly
    /// spread to fake a blur.
    #[allow(clippy::too_many_arguments)]
    fn draw_silhouette(
        &mut self,
        gl: &Gl,
        vp: &Mat4,
        model: &Mat4,
        character: usize,
        center: [f32; 2],
        size: f32,
        alpha: f32,
    ) {
        let Some(body) = self.art[character * LAYERS + L_BODY].clone() else {
            return;
        };
        gl.depth_mask(false);
        self.sprite_prog.bind(gl);
        let s = &mut self.sprite_prog;
        s.mat4(gl, "u_vp", vp);
        s.mat4(gl, "u_model", model);
        s.i1(gl, "u_tex", 0);
        s.f4(gl, "u_crop", [0.0, 0.0, 1.0, 1.0]);
        s.f4(gl, "u_tint", [0.05, 0.03, 0.1, alpha / 4.0]);
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, Some(&body));
        for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let spread = size * 0.012;
            s.f4(
                gl,
                "u_rect",
                [center[0] + dx * spread, center[1] + dy * spread, size, size],
            );
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        gl.depth_mask(true);
    }

    fn draw_card_shadows(&mut self, gl: &Gl, vp: &Mat4, poses: &[CardPose]) {
        gl.disable(Gl::DEPTH_TEST);
        self.shape_prog.bind(gl);
        self.shape_prog.mat4(gl, "u_vp", vp);
        self.shape_prog.f1(gl, "u_shape", 0.0);
        for pose in poses {
            let lift = (pose.pos[2] - FAN_Z).max(0.0);
            let m = Mat4::translate(
                pose.pos[0],
                pose.pos[1] - CARD_H * pose.scale * 0.55 - 0.05,
                -0.1,
            )
            .mul(&Mat4::rot_x(-1.3));
            self.shape_prog.mat4(gl, "u_model", &m);
            self.shape_prog.f4(
                gl,
                "u_rect",
                [0.0, 0.0, CARD_W * pose.scale * (1.2 + lift), 0.16],
            );
            self.shape_prog.f4(
                gl,
                "u_color",
                [0.0, 0.0, 0.0, 0.22 * pose.alpha / (1.0 + lift * 2.0)],
            );
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        gl.enable(Gl::DEPTH_TEST);
    }

    fn draw_cards(&mut self, gl: &Gl, vp: &Mat4, poses: &[CardPose], out: bool) {
        if poses.is_empty() {
            return;
        }
        // Soft shadows behind the cards first, so no card shades another.
        if out {
            gl.depth_mask(false);
            self.shape_prog.bind(gl);
            self.shape_prog.mat4(gl, "u_vp", vp);
            self.shape_prog.f1(gl, "u_shape", 2.0);
            for pose in poses {
                let lift = self.hover[pose.index].x
                    + if Some(pose.index) == self.picked {
                        1.0
                    } else {
                        0.0
                    };
                let m =
                    self.card_model(pose)
                        .mul(&Mat4::translate(0.0, -0.045 - 0.03 * lift, -0.04));
                self.shape_prog.mat4(gl, "u_model", &m);
                self.shape_prog.f4(
                    gl,
                    "u_rect",
                    [
                        0.0,
                        0.0,
                        CARD_W * (1.3 + 0.1 * lift),
                        CARD_H * (1.24 + 0.08 * lift),
                    ],
                );
                self.shape_prog.f4(
                    gl,
                    "u_color",
                    [0.08, 0.06, 0.16, 0.3 * pose.alpha / (1.0 + 0.4 * lift)],
                );
                self.quad.draw(gl, Gl::TRIANGLES);
            }
            gl.depth_mask(true);
        }
        let has_front = self.card_front.is_some();
        let clip = self.opening_plane();
        for pose in poses {
            if pose.alpha <= 0.002 {
                continue;
            }
            let model = self.card_model(pose);
            self.card_prog.bind(gl);
            let c = &mut self.card_prog;
            c.mat4(gl, "u_vp", vp);
            c.mat4(gl, "u_model", &model);
            c.f2(gl, "u_size", CARD_W, CARD_H);
            c.f3(gl, "u_eye", [0.0, 0.0, EYE_Z]);
            c.f1(gl, "u_alpha", pose.alpha);
            c.f1(gl, "u_time", self.time);
            c.f4(gl, "u_clip", clip);
            c.f2(gl, "u_tilt", pose.yaw.sin(), pose.pitch);
            let picked = Some(pose.index) == self.picked;
            let (glare, hover) = if picked {
                (
                    (0.5 + self.nx * 0.45, 0.45 - self.ny * 0.45),
                    if self.hovering { 0.6 } else { 0.0 },
                )
            } else {
                (self.glare[pose.index], self.hover[pose.index].x)
            };
            c.f2(gl, "u_glare", glare.0, glare.1);
            c.f1(gl, "u_hover", hover);
            c.f1(
                gl,
                "u_has_front",
                if has_front && Some(pose.index) == self.picked {
                    1.0
                } else {
                    0.0
                },
            );
            c.i1(gl, "u_front", 0);
            c.i1(gl, "u_back", 1);
            gl.active_texture(Gl::TEXTURE0);
            gl.bind_texture(
                Gl::TEXTURE_2D,
                self.card_front.as_ref().or(self.card_back.as_ref()),
            );
            gl.active_texture(Gl::TEXTURE1);
            gl.bind_texture(Gl::TEXTURE_2D, self.card_back.as_ref());
            gl.active_texture(Gl::TEXTURE0);
            gl.front_face(Gl::CW);
            for (mode, z) in [(1.0, -CARD_T / 2.0), (0.0, CARD_T / 2.0)] {
                c.f1(gl, "u_mode", mode);
                c.f1(gl, "u_z", z);
                c.f1(gl, "u_grow", 0.0);
                self.card_mesh.draw(gl, Gl::TRIANGLES);
            }
            gl.front_face(Gl::CCW);
            // The sides: the face's colour on the revealed front, dark on the backs.
            let front = if picked && has_front {
                (0.5 + pose.yaw.cos() * 2.5).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let tone = EDGE_TONES[self.reveal_kind.min(DESIGNS - 1)];
            let edge: [f32; 3] = std::array::from_fn(|k| 0.11 + (tone[k] - 0.11) * front);
            let r = &mut self.rim_prog;
            r.bind(gl);
            r.mat4(gl, "u_vp", vp);
            r.mat4(gl, "u_model", &model);
            r.f2(gl, "u_half", CARD_W / 2.0, CARD_H / 2.0);
            r.f1(gl, "u_radius", CARD_R);
            r.f3(gl, "u_color", edge);
            r.f3(gl, "u_eye", [0.0, 0.0, EYE_Z]);
            r.f1(gl, "u_alpha", pose.alpha);
            r.f1(gl, "u_thick", CARD_T);
            r.f4(gl, "u_clip", clip);
            self.rim_mesh.draw(gl, Gl::TRIANGLES);
            // The character floats above the front, on its own layer.
            if Some(pose.index) == self.picked && has_front && pose.yaw.cos() > 0.0 {
                let character = DESIGN_CHAR[self.reveal_kind];
                let par = [-self.card_ry.x * 0.05, self.card_rx.x * 0.05];
                let local = model.mul(&Mat4::translate(par[0], par[1], 0.09));
                let done = self
                    .reveal_at
                    .map(|at| self.time - at > REVEAL_SECONDS)
                    .unwrap_or(false);
                let fresh = self
                    .reveal_at
                    .map(|at| self.time - at < REVEAL_SECONDS + 1.0)
                    .unwrap_or(false);
                let eyes = if done && fresh { L_JOY } else { L_EYES };
                let look = [self.nx * 0.01, self.ny * 0.008];
                // A soft shadow on the art window, offset away from the light,
                // so the character floats above the card instead of sitting on it.
                let char_y = CARD_H * 0.5 - CARD_H * 0.048 - CARD_H * 0.53 * 0.5;
                let shadow = model.mul(&Mat4::translate(
                    0.012 - par[0] * 0.6,
                    char_y - CARD_W * 0.2 - par[1] * 0.6,
                    0.012,
                ));
                gl.depth_mask(false);
                self.shape_prog.bind(gl);
                self.shape_prog.mat4(gl, "u_vp", vp);
                self.shape_prog.mat4(gl, "u_model", &shadow);
                self.shape_prog.f1(gl, "u_shape", 0.0);
                self.shape_prog
                    .f4(gl, "u_rect", [0.0, 0.0, CARD_W * 0.5, CARD_W * 0.14]);
                self.shape_prog
                    .f4(gl, "u_color", [0.0, 0.0, 0.0, 0.35 * pose.alpha]);
                self.quad.draw(gl, Gl::TRIANGLES);
                gl.depth_mask(true);
                // The character's own soft drop shadow, a little below it.
                let under = model.mul(&Mat4::translate(par[0] * 0.5, par[1] * 0.5, 0.05));
                self.draw_silhouette(
                    gl,
                    vp,
                    &under,
                    character,
                    [0.006, char_y - 0.012],
                    CARD_W * 0.64,
                    0.26 * pose.alpha,
                );
                self.draw_character(
                    gl,
                    vp,
                    &local,
                    character,
                    [0.0, CARD_H * 0.5 - CARD_H * 0.048 - CARD_H * 0.53 * 0.5],
                    CARD_W * 0.64,
                    eyes,
                    L_SMILE,
                    look,
                    pose.alpha,
                );
            }
        }
    }

    fn draw_ribbons(&mut self, gl: &Gl, vp: &Mat4, model: &Mat4, age: f32) {
        let fade = 1.0 - smooth((age - 1.3) / 0.6);
        if fade <= 0.0 || self.reduced {
            return;
        }
        let grow = ease_out(age / 0.65);
        let mut rng = Rng::new(self.seed ^ 0x51);
        self.ribbon_prog.bind(gl);
        self.ribbon_prog.mat4(gl, "u_vp", vp);
        let count = 16;
        for i in 0..count {
            let k = i as f32 / (count - 1) as f32;
            let base_x = (k - 0.5) * PACK_ASPECT * 0.86;
            let spread = (k - 0.5) * 1.6 + rng.range(-0.12, 0.12);
            let len = rng.range(0.7, 1.05) * grow;
            let amp = rng.range(0.018, 0.032);
            let freq = rng.range(16.0, 24.0);
            let phase = rng.range(0.0, std::f32::consts::TAU) + age * 9.0;
            let width = 0.02;
            let segs = 46;
            let mut verts = Vec::with_capacity((segs + 1) * 8);
            for j in 0..=segs {
                let t = j as f32 / segs as f32;
                let s = t * len;
                let dir = (spread * t * 0.9).sin_cos();
                let wave = amp * (s * freq - phase).sin() * (0.3 + t);
                let px = base_x + dir.0 * s * 0.8 + dir.1 * wave;
                let py = 0.38 + dir.1 * s - dir.0 * wave * 0.4;
                let pz = 0.02 + 0.08 * t;
                let (nx, ny) = (dir.1 * width * 0.5, -dir.0 * width * 0.5);
                for side in [-1.0f32, 1.0] {
                    let w = model.point([px + nx * side, py + ny * side, pz]);
                    verts.extend_from_slice(&[w[0], w[1], w[2], t]);
                }
            }
            self.ribbon_mesh.update(gl, &verts);
            let c = RIBBON_COLORS[i % RIBBON_COLORS.len()];
            self.ribbon_prog
                .f4(gl, "u_color", [c[0], c[1], c[2], 0.95 * fade]);
            self.ribbon_mesh.draw(gl, Gl::TRIANGLE_STRIP);
        }
    }

    fn draw_sparkles(&mut self, gl: &Gl, vp: &Mat4, age: f32) {
        let fade = 1.0 - smooth((age - 0.9) / 0.8);
        if fade <= 0.0 {
            return;
        }
        let model = self.pack_model_dropped(0.0);
        self.shape_prog.bind(gl);
        self.shape_prog.mat4(gl, "u_vp", vp);
        self.shape_prog.f1(gl, "u_shape", 1.0);
        for (i, s) in self.sparkles.iter().enumerate() {
            let twinkle = 0.6 + 0.4 * (age * 14.0 + i as f32).sin();
            let p = model.point(s.pos);
            let m = Mat4::translate(p[0], p[1], p[2]).mul(&Mat4::rot_z(age * 2.0 + i as f32));
            self.shape_prog.mat4(gl, "u_model", &m);
            self.shape_prog
                .f4(gl, "u_rect", [0.0, 0.0, s.size * 2.0, s.size * 2.0]);
            self.shape_prog.f4(
                gl,
                "u_color",
                [s.color[0], s.color[1], s.color[2], fade * twinkle],
            );
            self.quad.draw(gl, Gl::TRIANGLES);
        }
    }
}
