//! Thin WebGL2 helpers: programs, buffers, textures and 4x4 matrices.

use std::collections::HashMap;

use js_sys::Float32Array;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    HtmlCanvasElement, HtmlImageElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram,
    WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
};

pub fn err(message: &str) -> JsValue {
    JsValue::from_str(message)
}

pub struct Program {
    pub program: WebGlProgram,
    uniforms: HashMap<&'static str, Option<WebGlUniformLocation>>,
}

impl Program {
    pub fn new(gl: &Gl, vertex: &str, fragment: &str) -> Result<Self, JsValue> {
        let compile = |kind: u32, source: &str| -> Result<_, JsValue> {
            let shader = gl
                .create_shader(kind)
                .ok_or_else(|| err("Unable to create shader"))?;
            gl.shader_source(&shader, source);
            gl.compile_shader(&shader);
            if gl
                .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
                .as_bool()
                != Some(true)
            {
                return Err(err(&gl.get_shader_info_log(&shader).unwrap_or_default()));
            }
            Ok(shader)
        };
        let vs = compile(Gl::VERTEX_SHADER, vertex)?;
        let fs = compile(Gl::FRAGMENT_SHADER, fragment)?;
        let program = gl
            .create_program()
            .ok_or_else(|| err("Unable to create program"))?;
        gl.attach_shader(&program, &vs);
        gl.attach_shader(&program, &fs);
        gl.bind_attrib_location(&program, 0, "a_pos");
        gl.link_program(&program);
        gl.delete_shader(Some(&vs));
        gl.delete_shader(Some(&fs));
        if gl
            .get_program_parameter(&program, Gl::LINK_STATUS)
            .as_bool()
            != Some(true)
        {
            return Err(err(&gl.get_program_info_log(&program).unwrap_or_default()));
        }
        Ok(Self {
            program,
            uniforms: HashMap::new(),
        })
    }

    pub fn bind(&self, gl: &Gl) {
        gl.use_program(Some(&self.program));
    }

    fn loc(&mut self, gl: &Gl, name: &'static str) -> Option<WebGlUniformLocation> {
        self.uniforms
            .entry(name)
            .or_insert_with(|| gl.get_uniform_location(&self.program, name))
            .clone()
    }

    pub fn f1(&mut self, gl: &Gl, name: &'static str, v: f32) {
        gl.uniform1f(self.loc(gl, name).as_ref(), v);
    }
    pub fn f2(&mut self, gl: &Gl, name: &'static str, a: f32, b: f32) {
        gl.uniform2f(self.loc(gl, name).as_ref(), a, b);
    }
    pub fn f3(&mut self, gl: &Gl, name: &'static str, v: [f32; 3]) {
        gl.uniform3f(self.loc(gl, name).as_ref(), v[0], v[1], v[2]);
    }
    pub fn f4(&mut self, gl: &Gl, name: &'static str, v: [f32; 4]) {
        gl.uniform4f(self.loc(gl, name).as_ref(), v[0], v[1], v[2], v[3]);
    }
    pub fn i1(&mut self, gl: &Gl, name: &'static str, v: i32) {
        gl.uniform1i(self.loc(gl, name).as_ref(), v);
    }
    pub fn f3v(&mut self, gl: &Gl, name: &'static str, v: &[f32]) {
        gl.uniform3fv_with_f32_array(self.loc(gl, name).as_ref(), v);
    }
    pub fn fv(&mut self, gl: &Gl, name: &'static str, v: &[f32]) {
        gl.uniform1fv_with_f32_array(self.loc(gl, name).as_ref(), v);
    }
    pub fn mat4(&mut self, gl: &Gl, name: &'static str, m: &Mat4) {
        gl.uniform_matrix4fv_with_f32_array(self.loc(gl, name).as_ref(), false, &m.0);
    }

    pub fn delete(&self, gl: &Gl) {
        gl.delete_program(Some(&self.program));
    }
}

/// A vertex array with one float attribute stream at location 0.
pub struct Mesh {
    pub vao: WebGlVertexArrayObject,
    pub buffer: WebGlBuffer,
    pub count: i32,
    pub components: i32,
}

impl Mesh {
    pub fn new(gl: &Gl, data: &[f32], components: i32, dynamic: bool) -> Result<Self, JsValue> {
        let vao = gl
            .create_vertex_array()
            .ok_or_else(|| err("No vertex array"))?;
        let buffer = gl.create_buffer().ok_or_else(|| err("No buffer"))?;
        gl.bind_vertex_array(Some(&vao));
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buffer));
        let usage = if dynamic {
            Gl::DYNAMIC_DRAW
        } else {
            Gl::STATIC_DRAW
        };
        gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &Float32Array::from(data), usage);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, components, Gl::FLOAT, false, 0, 0);
        gl.bind_vertex_array(None);
        Ok(Self {
            vao,
            buffer,
            count: data.len() as i32 / components,
            components,
        })
    }

    pub fn update(&mut self, gl: &Gl, data: &[f32]) {
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.buffer));
        gl.buffer_data_with_array_buffer_view(
            Gl::ARRAY_BUFFER,
            &Float32Array::from(data),
            Gl::DYNAMIC_DRAW,
        );
        self.count = data.len() as i32 / self.components;
    }

    pub fn draw(&self, gl: &Gl, mode: u32) {
        gl.bind_vertex_array(Some(&self.vao));
        gl.draw_arrays(mode, 0, self.count);
    }

    pub fn delete(&self, gl: &Gl) {
        gl.delete_buffer(Some(&self.buffer));
        gl.delete_vertex_array(Some(&self.vao));
    }
}

/// A grid of (u, v) points in [0,1]^2 as triangles.
pub fn grid(cols: usize, rows: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(cols * rows * 12);
    for j in 0..rows {
        for i in 0..cols {
            let (u0, u1) = (i as f32 / cols as f32, (i + 1) as f32 / cols as f32);
            let (v0, v1) = (j as f32 / rows as f32, (j + 1) as f32 / rows as f32);
            out.extend_from_slice(&[u0, v0, u1, v0, u0, v1, u0, v1, u1, v0, u1, v1]);
        }
    }
    out
}

pub fn texture_from_image(gl: &Gl, image: &HtmlImageElement) -> Result<WebGlTexture, JsValue> {
    let texture = gl.create_texture().ok_or_else(|| err("No texture"))?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.pixel_storei(Gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0);
    gl.tex_image_2d_with_u32_and_u32_and_html_image_element(
        Gl::TEXTURE_2D,
        0,
        Gl::RGBA as i32,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        image,
    )?;
    finish_texture(gl);
    Ok(texture)
}

pub fn texture_from_canvas(gl: &Gl, canvas: &HtmlCanvasElement) -> Result<WebGlTexture, JsValue> {
    let texture = gl.create_texture().ok_or_else(|| err("No texture"))?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.pixel_storei(Gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0);
    gl.tex_image_2d_with_u32_and_u32_and_html_canvas_element(
        Gl::TEXTURE_2D,
        0,
        Gl::RGBA as i32,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        canvas,
    )?;
    finish_texture(gl);
    Ok(texture)
}

fn finish_texture(gl: &Gl) {
    gl.generate_mipmap(Gl::TEXTURE_2D);
    gl.tex_parameteri(
        Gl::TEXTURE_2D,
        Gl::TEXTURE_MIN_FILTER,
        Gl::LINEAR_MIPMAP_LINEAR as i32,
    );
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
}

pub fn offscreen_canvas(width: u32, height: u32) -> Result<HtmlCanvasElement, JsValue> {
    let canvas: HtmlCanvasElement = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| err("Missing document"))?
        .create_element("canvas")?
        .dyn_into()?;
    canvas.set_width(width);
    canvas.set_height(height);
    Ok(canvas)
}

pub use crate::mat::Mat4;
