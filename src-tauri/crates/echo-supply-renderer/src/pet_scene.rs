//! The pet companion scene for EchoBird's Pets page, drawn with WebGL2
//! straight from WebAssembly. It replaces the three.js `petScene.js` and
//! keeps its behaviour: the same voxel pets, poses, hatch, growth, tints,
//! framing and thumbnails. `pet_pose` decides what to show; this module
//! owns the GPU buffers and issues the draw calls.

use std::collections::HashMap;

use js_sys::{Float32Array, Uint16Array};
use wasm_bindgen::prelude::*;
use web_sys::{
    HtmlCanvasElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlVertexArrayObject,
};

use crate::gl::{err, grid, Mat4, Mesh, Program};
use crate::pet_mesh::{MeshData, STRIDE};
use crate::pet_models::{build, Models, Voxel, VoxelMap, KINDS};
use crate::pet_pose::{
    cell_instances, core_color, frame, lights, palette, Frame, MeshKey, Mode, Pet, View, TINTS,
};
use crate::shaders::{PET_FS, PET_VS, QUAD_VS, SHAPE_FS};

/// EchoBird orange and gold for level-up effects (sRGB hex).
const ORANGE: &str = "#ff8a3d";
const GOLD: &str = "#ffcf5a";
/// Where the camera looks from (see pet_pose), for the rim glow.
const CAMERA_DIR: [f32; 3] = [0.0, 12.0, 80.0];

/// Palette slots after the shared 28 colours: the core's light and shade.
const CORE_LIGHT: u8 = 28;
const CORE_SHADE: u8 = 29;
/// Floats per growth-cell instance: centre, scale, colour a, colour b, blend.
const CELL_STRIDE: i32 = 7;

struct GpuMesh {
    vao: WebGlVertexArrayObject,
    vertices: WebGlBuffer,
    indices: WebGlBuffer,
    ranges: Vec<(u32, u32)>,
}

impl GpuMesh {
    fn new(gl: &Gl, data: &MeshData) -> Result<Self, JsValue> {
        let vao = gl
            .create_vertex_array()
            .ok_or_else(|| err("No vertex array"))?;
        let vertices = gl.create_buffer().ok_or_else(|| err("No buffer"))?;
        let indices = gl.create_buffer().ok_or_else(|| err("No buffer"))?;
        gl.bind_vertex_array(Some(&vao));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertices));
        gl.buffer_data_with_array_buffer_view(
            Gl::ARRAY_BUFFER,
            &Float32Array::from(data.vertices.as_slice()),
            Gl::STATIC_DRAW,
        );
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&indices));
        gl.buffer_data_with_array_buffer_view(
            Gl::ELEMENT_ARRAY_BUFFER,
            &Uint16Array::from(data.indices.as_slice()),
            Gl::STATIC_DRAW,
        );
        let stride = (STRIDE * 4) as i32;
        for (location, size, offset) in [(0, 3, 0), (1, 3, 3), (2, 2, 6), (3, 3, 8), (4, 4, 11)] {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_with_i32(location, size, Gl::FLOAT, false, stride, offset * 4);
        }
        gl.bind_vertex_array(None);
        Ok(Self {
            vao,
            vertices,
            indices,
            ranges: data.ranges.clone(),
        })
    }

    fn draw(&self, gl: &Gl, range: usize) {
        let (first, count) = self.ranges[range];
        if count == 0 {
            return;
        }
        gl.bind_vertex_array(Some(&self.vao));
        gl.draw_elements_with_i32(
            Gl::TRIANGLES,
            count as i32,
            Gl::UNSIGNED_SHORT,
            first as i32 * 2,
        );
    }

    fn delete(&self, gl: &Gl) {
        gl.delete_buffer(Some(&self.vertices));
        gl.delete_buffer(Some(&self.indices));
        gl.delete_vertex_array(Some(&self.vao));
    }
}

#[wasm_bindgen]
pub struct PetScene {
    gl: Gl,
    canvas: HtmlCanvasElement,
    models: Models,
    program: Program,
    shape: Program,
    quad: Mesh,
    pets: HashMap<usize, GpuMesh>,
    egg: GpuMesh,
    shells: Vec<GpuMesh>,
    core: GpuMesh,
    cube: GpuMesh,
    cell_buffer: WebGlBuffer,
    pet: Pet,
    mode: Mode,
    progress: f64,
    angle: f64,
    tilt: f64,
    daylight: bool,
    reduced: bool,
    disposed: bool,
}

fn kind_index(kind: &str) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(1)
}

#[wasm_bindgen]
impl PetScene {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<PetScene, JsValue> {
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"antialias".into(), &true.into())?;
        js_sys::Reflect::set(&options, &"alpha".into(), &true.into())?;
        js_sys::Reflect::set(&options, &"powerPreference".into(), &"low-power".into())?;
        let gl: Gl = canvas
            .get_context_with_context_options("webgl2", &options)?
            .ok_or_else(|| err("WebGL2 is unavailable"))?
            .dyn_into()?;
        let models = build();
        let program = Program::new(&gl, PET_VS, PET_FS)?;
        let shape = Program::new(&gl, QUAD_VS, SHAPE_FS)?;
        let quad = Mesh::new(&gl, &grid(1, 1), 2, false)?;

        let mut egg = MeshData::default();
        egg.push(&models.egg, &models.egg, Some(&models.cracked), [0.0; 3]);
        let egg = GpuMesh::new(&gl, &egg)?;
        let shells = models
            .shells
            .iter()
            .map(|shell| {
                let mut mesh = MeshData::default();
                mesh.push(shell, shell, None, [0.0; 3]);
                GpuMesh::new(&gl, &mesh)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut core_map = VoxelMap::default();
        for x in -2..=2 {
            for y in -2..=2 {
                for z in -2i32..=2 {
                    if x * x + y * y + z * z <= 4 {
                        let color = if y + z > 0 { CORE_LIGHT } else { CORE_SHADE };
                        core_map.set(Voxel { x, y, z, color });
                    }
                }
            }
        }
        let mut core = MeshData::default();
        core.push(&core_map, &core_map, None, [0.5; 3]);
        let core = GpuMesh::new(&gl, &core)?;

        let cube = GpuMesh::new(&gl, &MeshData::cube())?;
        let cell_buffer = gl.create_buffer().ok_or_else(|| err("No buffer"))?;
        gl.bind_vertex_array(Some(&cube.vao));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&cell_buffer));
        let stride = CELL_STRIDE * 4;
        gl.enable_vertex_attrib_array(5);
        gl.vertex_attrib_pointer_with_i32(5, 4, Gl::FLOAT, false, stride, 0);
        gl.vertex_attrib_divisor(5, 1);
        gl.enable_vertex_attrib_array(6);
        gl.vertex_attrib_pointer_with_i32(6, 3, Gl::FLOAT, false, stride, 16);
        gl.vertex_attrib_divisor(6, 1);
        gl.bind_vertex_array(None);

        gl.enable(Gl::DEPTH_TEST);
        gl.depth_func(Gl::LEQUAL);
        gl.enable(Gl::CULL_FACE);
        gl.cull_face(Gl::BACK);
        gl.enable(Gl::BLEND);
        gl.blend_func_separate(
            Gl::SRC_ALPHA,
            Gl::ONE_MINUS_SRC_ALPHA,
            Gl::ONE,
            Gl::ONE_MINUS_SRC_ALPHA,
        );
        Ok(PetScene {
            gl,
            canvas,
            models,
            program,
            shape,
            quad,
            pets: HashMap::new(),
            egg,
            shells,
            core,
            cube,
            cell_buffer,
            pet: Pet {
                kind: kind_index("mecha"),
                stage: 2,
                color: 0,
            },
            mode: Mode::Still,
            progress: 0.0,
            angle: 28.0,
            tilt: 0.0,
            daylight: false,
            reduced: false,
            disposed: false,
        })
    }

    /// The pet to show when nothing is animating.
    pub fn show(&mut self, kind: &str, stage: u32, color: u32) {
        self.pet = Pet {
            kind: kind_index(kind),
            stage: (stage as usize).min(4),
            color: (color as usize).min(TINTS.len() - 1),
        };
    }

    /// The running animation ("still", "level", "interact", "crack",
    /// "hatch" or "grow") and how far along it is.
    pub fn set_mode(&mut self, mode: &str, progress: f64) {
        self.mode = Mode::parse(mode);
        self.progress = progress;
    }

    pub fn rotate(&mut self, dx: f64, dy: f64) {
        self.angle = (self.angle + dx * 0.6 + 540.0).rem_euclid(360.0) - 180.0;
        self.tilt = (self.tilt + dy * 0.007).clamp(-0.65, 0.65);
    }

    /// The light theme needs softer, fuller light (thumbnails keep studio light).
    pub fn set_daylight(&mut self, daylight: bool) {
        self.daylight = daylight;
    }

    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced = reduced;
    }

    /// Draws the current pet into a `width` x `height` CSS-pixel canvas.
    pub fn render(&mut self, width: f64, height: f64, dpr: f64) -> Result<(), JsValue> {
        let view = View {
            mode: self.mode,
            progress: self.progress,
            angle: self.angle,
            tilt: self.tilt,
            miniature: false,
            reduced: self.reduced,
        };
        self.draw(self.pet, &view, width, height, dpr)
    }

    /// A 96 px three-quarter portrait as a PNG data URL. Call `render`
    /// afterwards to put the main view back.
    pub fn thumbnail(
        &mut self,
        kind: &str,
        stage: u32,
        color: u32,
        dpr: f64,
    ) -> Result<String, JsValue> {
        let pet = Pet {
            kind: kind_index(kind),
            stage: (stage as usize).min(4),
            color: (color as usize).min(TINTS.len() - 1),
        };
        let view = View {
            mode: Mode::Still,
            progress: 0.0,
            angle: 0.0,
            tilt: 0.0,
            miniature: true,
            reduced: self.reduced,
        };
        self.draw(pet, &view, 96.0, 96.0, dpr)?;
        self.canvas.to_data_url()
    }

    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        let gl = &self.gl;
        for mesh in self.pets.values() {
            mesh.delete(gl);
        }
        self.pets.clear();
        self.egg.delete(gl);
        for shell in &self.shells {
            shell.delete(gl);
        }
        self.core.delete(gl);
        self.cube.delete(gl);
        gl.delete_buffer(Some(&self.cell_buffer));
        self.quad.delete(gl);
        self.program.delete(gl);
        self.shape.delete(gl);
    }
}

impl PetScene {
    fn draw(
        &mut self,
        pet: Pet,
        view: &View,
        width: f64,
        height: f64,
        dpr: f64,
    ) -> Result<(), JsValue> {
        if self.disposed || width < 1.0 || height < 1.0 {
            return Ok(());
        }
        let ratio = dpr.clamp(1.0, 1.75);
        let (w, h) = (
            (width * ratio).floor() as u32,
            (height * ratio).floor() as u32,
        );
        if self.canvas.width() != w {
            self.canvas.set_width(w);
        }
        if self.canvas.height() != h {
            self.canvas.set_height(h);
        }
        let gl = self.gl.clone();
        gl.viewport(0, 0, w as i32, h as i32);
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.depth_mask(true);
        gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);

        let f = frame(&self.models, pet, view, width, height);
        let vp = f.proj.mul(&f.view);
        self.draw_voxels(&gl, &f, pet, view, &vp)?;
        self.draw_ground(&gl, &f, &vp);
        self.draw_fx(&gl, &f, &vp);
        self.draw_sparks(&gl, &f, pet, &vp);
        Ok(())
    }

    fn mesh(&mut self, key: MeshKey) -> Result<(&GpuMesh, usize), JsValue> {
        Ok(match key {
            MeshKey::Part { def, part } => {
                if !self.pets.contains_key(&def) {
                    let data = MeshData::for_definition(&self.models.definitions[def]);
                    self.pets.insert(def, GpuMesh::new(&self.gl, &data)?);
                }
                (&self.pets[&def], part)
            }
            MeshKey::Egg => (&self.egg, 0),
            MeshKey::Shell(i) => (&self.shells[i], 0),
        })
    }

    fn draw_voxels(
        &mut self,
        gl: &Gl,
        f: &Frame,
        pet: Pet,
        view: &View,
        vp: &Mat4,
    ) -> Result<(), JsValue> {
        let mut colors: Vec<f32> = palette(pet.kind, pet.color).concat();
        colors.extend_from_slice(&[1.0, 1.0, 1.0]);
        colors.extend_from_slice(&crate::pet_pose::linear("#d4e3ec").map(|v| v as f32));
        // Level-up colours: EchoBird orange, gold, and the pet's own tint.
        for hex in [ORANGE, GOLD, TINTS[pet.color]] {
            colors.extend_from_slice(&crate::pet_pose::linear(hex).map(|v| v as f32));
        }
        let aura = if f.fx.gold {
            crate::pet_pose::linear(GOLD)
        } else {
            let (o, t) = (
                crate::pet_pose::linear(ORANGE),
                crate::pet_pose::linear(TINTS[pet.color]),
            );
            std::array::from_fn(|i| o[i] * 0.6 + t[i] * 0.4)
        };
        let [key_dir, key, fill_dir, fill, ambient] = lights(self.daylight && !view.miniature);
        gl.enable(Gl::DEPTH_TEST);
        gl.enable(Gl::CULL_FACE);
        let p = &mut self.program;
        p.bind(gl);
        p.mat4(gl, "u_vp", vp);
        p.f3v(gl, "u_palette", &colors);
        p.f3(gl, "u_key_dir", key_dir);
        p.f3(gl, "u_key", key);
        p.f3(gl, "u_fill_dir", fill_dir);
        p.f3(gl, "u_fill", fill);
        p.f3(gl, "u_ambient", ambient);
        p.f1(gl, "u_unlit", 0.0);
        p.f1(gl, "u_cells", 0.0);
        p.f4(
            gl,
            "u_aura",
            [aura[0] as f32, aura[1] as f32, aura[2] as f32, f.fx.glow],
        );
        let d = (CAMERA_DIR[0] * CAMERA_DIR[0]
            + CAMERA_DIR[1] * CAMERA_DIR[1]
            + CAMERA_DIR[2] * CAMERA_DIR[2])
            .sqrt();
        p.f3(gl, "u_view_dir", CAMERA_DIR.map(|v| v / d));
        for item in &f.items {
            self.program.mat4(gl, "u_model", &item.model);
            self.program.f1(gl, "u_crack", item.crack);
            let (mesh, range) = self.mesh(item.mesh)?;
            mesh.draw(gl, range);
        }
        if let Some(core) = &f.core {
            let p = &mut self.program;
            p.mat4(gl, "u_model", core);
            p.f1(gl, "u_crack", 0.0);
            p.f1(gl, "u_unlit", 1.0);
            p.f3(gl, "u_glow", core_color(pet.color));
            self.core.draw(gl, 0);
            self.program.f1(gl, "u_unlit", 0.0);
        }
        if let Some(cells) = &f.cells {
            let data: Vec<f32> = cell_instances(&self.models, cells).concat();
            let p = &mut self.program;
            p.mat4(gl, "u_model", &cells.group);
            p.f1(gl, "u_crack", 0.0);
            p.f1(gl, "u_cells", 1.0);
            gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.cell_buffer));
            gl.buffer_data_with_array_buffer_view(
                Gl::ARRAY_BUFFER,
                &Float32Array::from(data.as_slice()),
                Gl::DYNAMIC_DRAW,
            );
            gl.bind_vertex_array(Some(&self.cube.vao));
            gl.draw_elements_instanced_with_i32(
                Gl::TRIANGLES,
                self.cube.ranges[0].1 as i32,
                Gl::UNSIGNED_SHORT,
                0,
                data.len() as i32 / CELL_STRIDE,
            );
            self.program.f1(gl, "u_cells", 0.0);
        }
        if !f.fx.confetti.is_empty() {
            // Voxel confetti: one instanced cube per bit, in world space.
            let data: Vec<f32> =
                f.fx.confetti
                    .iter()
                    .flat_map(|b| {
                        let s = b.slot as f32;
                        [
                            b.pos[0] as f32,
                            b.pos[1] as f32,
                            b.pos[2] as f32,
                            b.size as f32,
                            s,
                            s,
                            0.0,
                        ]
                    })
                    .collect();
            let p = &mut self.program;
            p.mat4(gl, "u_model", &Mat4::identity());
            p.f1(gl, "u_crack", 0.0);
            p.f1(gl, "u_cells", 1.0);
            p.f4(gl, "u_aura", [1.0, 1.0, 1.0, 0.15]);
            gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.cell_buffer));
            gl.buffer_data_with_array_buffer_view(
                Gl::ARRAY_BUFFER,
                &Float32Array::from(data.as_slice()),
                Gl::DYNAMIC_DRAW,
            );
            gl.bind_vertex_array(Some(&self.cube.vao));
            gl.draw_elements_instanced_with_i32(
                Gl::TRIANGLES,
                self.cube.ranges[0].1 as i32,
                Gl::UNSIGNED_SHORT,
                0,
                data.len() as i32 / CELL_STRIDE,
            );
            self.program.f1(gl, "u_cells", 0.0);
        }
        gl.bind_vertex_array(None);
        Ok(())
    }

    /// Light column, floor rings and energy motes for a level-up.
    fn draw_fx(&mut self, gl: &Gl, f: &Frame, vp: &Mat4) {
        let fx = &f.fx;
        if fx.beam.is_none() && fx.rings.is_empty() && fx.motes.is_empty() {
            return;
        }
        let srgb = |hex: &str| {
            let v = u32::from_str_radix(&hex[1..], 16).unwrap_or(0);
            [
                ((v >> 16) & 255) as f32 / 255.0,
                ((v >> 8) & 255) as f32 / 255.0,
                (v & 255) as f32 / 255.0,
            ]
        };
        let (orange, gold) = (srgb(ORANGE), srgb(GOLD));
        gl.disable(Gl::CULL_FACE);
        gl.depth_mask(false);
        let s = &mut self.shape;
        s.bind(gl);
        s.mat4(gl, "u_vp", vp);
        if let Some(beam) = &fx.beam {
            // Behind and through the pet: depth-tested so the pet stays in front.
            let c = if beam.gold { gold } else { orange };
            let m = Mat4::translate(
                beam.base[0],
                beam.base[1] + beam.height / 2.0,
                beam.base[2] - 3.0,
            );
            s.f1(gl, "u_shape", 4.0);
            s.mat4(gl, "u_model", &m);
            s.f4(gl, "u_rect", [0.0, 0.0, beam.width, beam.height]);
            s.f4(gl, "u_color", [c[0], c[1], c[2], 0.55 * beam.alpha]);
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        s.f1(gl, "u_shape", 3.0);
        for ring in &fx.rings {
            let c = if ring.gold { gold } else { orange };
            s.mat4(
                gl,
                "u_model",
                &ring.model.mul(&Mat4::rot_x(-std::f32::consts::FRAC_PI_2)),
            );
            s.f4(
                gl,
                "u_rect",
                [0.0, 0.0, ring.radius * 2.0, ring.radius * 2.0],
            );
            s.f4(gl, "u_color", [c[0], c[1], c[2], 0.9 * ring.alpha]);
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        // Motes always on top: they are energy flowing into the pet.
        gl.disable(Gl::DEPTH_TEST);
        s.f1(gl, "u_shape", 0.0);
        for mote in &fx.motes {
            let c = if mote.gold { gold } else { orange };
            let m = Mat4::translate(
                mote.pos[0] as f32,
                mote.pos[1] as f32,
                mote.pos[2] as f32 + 8.0,
            );
            let size = mote.size as f32 * 1.6;
            s.mat4(gl, "u_model", &m);
            s.f4(gl, "u_rect", [0.0, 0.0, size, size]);
            s.f4(gl, "u_color", [c[0], c[1], c[2], mote.alpha as f32]);
            self.quad.draw(gl, Gl::TRIANGLES);
            s.f4(gl, "u_rect", [0.0, 0.0, size * 0.4, size * 0.4]);
            s.f4(gl, "u_color", [1.0, 0.97, 0.9, mote.alpha as f32]);
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        gl.enable(Gl::DEPTH_TEST);
        gl.depth_mask(true);
    }

    /// A soft shadow on the floor, drawn after the pet so its feet cover it.
    fn draw_ground(&mut self, gl: &Gl, f: &Frame, vp: &Mat4) {
        let Some(ground) = &f.ground else {
            return;
        };
        gl.disable(Gl::CULL_FACE);
        gl.depth_mask(false);
        let s = &mut self.shape;
        s.bind(gl);
        s.mat4(gl, "u_vp", vp);
        s.mat4(
            gl,
            "u_model",
            &ground.model.mul(&Mat4::rot_x(-std::f32::consts::FRAC_PI_2)),
        );
        s.f1(gl, "u_shape", 0.0);
        s.f4(
            gl,
            "u_rect",
            [0.0, 0.0, ground.radius[0] * 2.0, ground.radius[1] * 2.0],
        );
        s.f4(gl, "u_color", [0.08, 0.1, 0.14, ground.alpha]);
        self.quad.draw(gl, Gl::TRIANGLES);
        gl.depth_mask(true);
    }

    /// Sparkles in the pet's colourway for hatching, growing and levelling.
    fn draw_sparks(&mut self, gl: &Gl, f: &Frame, pet: Pet, vp: &Mat4) {
        if f.sparks.is_empty() {
            return;
        }
        let tint = u32::from_str_radix(&TINTS[pet.color][1..], 16).unwrap_or(0);
        let channel = |shift: u32| (((tint >> shift) & 255) as f32 / 255.0) * 0.45 + 0.55;
        let color = [channel(16), channel(8), channel(0)];
        gl.disable(Gl::DEPTH_TEST);
        gl.disable(Gl::CULL_FACE);
        let s = &mut self.shape;
        s.bind(gl);
        s.mat4(gl, "u_vp", vp);
        s.f1(gl, "u_shape", 1.0);
        for spark in &f.sparks {
            let m = Mat4::translate(spark.pos[0], spark.pos[1], spark.pos[2])
                .mul(&Mat4::rot_z(spark.spin));
            s.mat4(gl, "u_model", &m);
            s.f4(gl, "u_rect", [0.0, 0.0, spark.size * 2.0, spark.size * 2.0]);
            s.f4(gl, "u_color", [color[0], color[1], color[2], spark.alpha]);
            self.quad.draw(gl, Gl::TRIANGLES);
        }
        gl.enable(Gl::DEPTH_TEST);
    }
}
