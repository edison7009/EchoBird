//! GLSL ES 3.0 shaders for the supply scene.

pub const PACK_VS: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_pos;
uniform mat4 u_vp;
uniform mat4 u_model;
uniform float u_w;
uniform float u_part;      // 0 body, 1 tear strip, 2 inner back lip
uniform float u_tear[65];
uniform float u_front;     // how far the tear has run, 0..1 along u_dir
uniform float u_dir;       // +1 peels from the left edge, -1 from the right
uniform float u_lift;
uniform float u_fly;       // strip carried away after the tear, 0..1
uniform float u_bulge;
out vec2 v_uv;
out vec3 v_n;
out vec3 v_world;
out float v_free;

float bulge(vec2 uv) {
    float c = 0.068;
    float vv = clamp((uv.y - c) / (1.0 - 2.0 * c), 0.0, 1.0);
    float bx = 1.0 - pow(abs(uv.x * 2.0 - 1.0), 2.4);
    float by = 1.0 - pow(abs(vv * 2.0 - 1.0), 3.0);
    float seal = smoothstep(c - 0.005, c + 0.06, uv.y) * smoothstep(1.0 - c + 0.005, 1.0 - c - 0.06, uv.y);
    return u_bulge * sqrt(max(bx, 0.0)) * sqrt(max(by, 0.0)) * seal;
}

void main() {
    v_uv = a_pos;
    float e = 0.004;
    float dzdu = (bulge(a_pos + vec2(e, 0.0)) - bulge(a_pos - vec2(e, 0.0))) / (2.0 * e * u_w);
    float dzdv = (bulge(a_pos + vec2(0.0, e)) - bulge(a_pos - vec2(0.0, e))) / (2.0 * e);
    vec3 n = normalize(vec3(-dzdu, dzdv, 1.0));
    vec3 p = vec3((a_pos.x - 0.5) * u_w, 0.5 - a_pos.y, bulge(a_pos));
    v_free = 0.0;
    if (u_part > 1.5) {
        // The inside of the back panel, just behind the front.
        p.z = -0.02 - 0.4 * bulge(a_pos);
        n = vec3(0.0, 0.0, 1.0);
    } else if (u_part > 0.5) {
        // Columns behind the tear front peel up and curl toward the viewer.
        float along = u_dir > 0.0 ? a_pos.x : 1.0 - a_pos.x;
        float d = (u_front - along) * u_w;
        if (d > 0.0) {
            // Curl around the tear front, wide at first and tighter toward the
            // free end, while the free end rises up and away from the pack.
            float r = 0.075 + d * 0.28;
            float th = min(d / r, 2.5);
            float frontx = (u_dir > 0.0 ? u_front - 0.5 : 0.5 - u_front) * u_w;
            float arc = d - th * r;
            p.x = frontx - u_dir * (r * sin(th) + arc * cos(th));
            p.z += r * (1.0 - cos(th)) + arc * sin(th);
            p.y += u_lift * (0.02 * smoothstep(0.0, 0.08, d) + 0.42 * d * d + 0.12 * d);
            float c = cos(th), s = sin(th);
            n = normalize(vec3(n.x * c - u_dir * n.z * s, n.y, u_dir * n.x * s + n.z * c));
            v_free = smoothstep(0.0, 0.03, d);
        }
        if (u_fly > 0.0) {
            float f = u_fly;
            p.y += f * f * 0.9 + f * 0.15;
            p.x += u_dir * f * 0.35;
            p.z += f * 0.25;
            float a = u_dir * f * 0.9;
            p.xy = vec2(p.x * cos(a) - p.y * sin(a), p.x * sin(a) + p.y * cos(a));
        }
    }
    vec4 world = u_model * vec4(p, 1.0);
    v_world = world.xyz;
    v_n = normalize(mat3(u_model) * n);
    gl_Position = u_vp * world;
}
"#;

pub const PACK_FS: &str = r#"#version 300 es
precision highp float;
in vec2 v_uv;
in vec3 v_n;
in vec3 v_world;
in float v_free;
uniform sampler2D u_print;
uniform float u_part;
uniform float u_tear[65];
uniform float u_front;
uniform float u_dir;
uniform float u_w;
uniform vec3 u_eye;
uniform float u_alpha;
uniform float u_time;
out vec4 o;

float tearY(float u) {
    float x = clamp(u, 0.0, 1.0) * 64.0;
    int i = min(int(floor(x)), 63);
    return mix(u_tear[i], u_tear[i + 1], x - float(i));
}
float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }

void main() {
    float c = 0.068;
    vec2 uv = v_uv;
    float line = tearY(uv.x);
    float along = u_dir > 0.0 ? uv.x : 1.0 - uv.x;
    bool opened = along < u_front;

    if (u_part < 0.5 && uv.y < line) discard;
    if (u_part > 0.5 && u_part < 1.5 && uv.y > line) discard;
    if (u_part > 1.5 && uv.y < line + 0.012) discard;

    // Crimped teeth on both seals, and a body a little narrower than the seals.
    float tri = abs(fract(uv.x * 46.0) - 0.5) * 2.0;
    if (uv.y < 0.011 * tri || uv.y > 1.0 - 0.011 * tri) discard;
    float body = clamp((uv.y - c) / (1.0 - 2.0 * c), 0.0, 1.0);
    float inset = (uv.y > c && uv.y < 1.0 - c) ? 0.011 * sin(3.14159 * body) : 0.0;
    if (uv.x < inset || uv.x > 1.0 - inset) discard;

    vec3 n = normalize(v_n);
    if (!gl_FrontFacing) n = -n;
    vec3 v = normalize(u_eye - v_world);
    vec3 l = normalize(vec3(-0.45, 0.65, 0.75));
    vec3 h = normalize(l + v);
    float ndl = max(dot(n, l), 0.0);
    float ndh = max(dot(n, h), 0.0);
    float fres = pow(1.0 - max(dot(n, v), 0.0), 3.0);
    bool seal = uv.y < c || uv.y > 1.0 - c;

    vec3 col;
    if (u_part > 1.5) {
        // Inside of the pack: dim brushed metal fading into the dark.
        float depth = clamp((uv.y - line) / 0.18, 0.0, 1.0);
        col = mix(vec3(0.62, 0.64, 0.7), vec3(0.08, 0.08, 0.1), depth);
        col += 0.05 * sin(uv.x * 140.0 + uv.y * 30.0);
    } else if (!gl_FrontFacing) {
        // The silver inner layer, seen as the strip curls over.
        float band = 0.5 + 0.5 * sin(uv.x * 26.0 + uv.y * 70.0 + dot(n, v) * 6.0);
        col = mix(vec3(0.74, 0.77, 0.84), vec3(0.98, 0.99, 1.0), band);
        col += pow(ndh, 40.0) * 0.6;
        col *= 0.8 + 0.25 * ndl;
    } else {
        // Black film: soft wide sheen plus one narrow highlight that slides with the tilt.
        col = vec3(0.028, 0.028, 0.034);
        col += vec3(0.16) * ndl * 0.35;
        col += vec3(1.0) * pow(ndh, 110.0) * 0.85;
        col += vec3(0.85, 0.88, 1.0) * pow(ndh, 14.0) * 0.12;
        vec3 r = reflect(-v, n);
        float streak = exp(-pow((dot(r, normalize(vec3(0.55, 0.25, 0.8))) - 0.92) * 22.0, 2.0));
        col += vec3(0.9, 0.93, 1.0) * streak * 0.22;
        col += vec3(0.5, 0.55, 0.65) * fres * 0.18;
        if (seal) {
            // Heat-seal ridges catch the light in fine vertical lines.
            float ridge = sin(uv.x * u_w * 950.0);
            col += vec3(0.05) * ridge + vec3(0.03);
            col *= 0.92;
        } else {
            // A faint print texture so the black reads as film, not plastic.
            col += (hash(floor(uv * vec2(380.0, 550.0))) - 0.5) * 0.012;
        }
        vec4 print = texture(u_print, uv);
        col = mix(col, print.rgb * (0.85 + 0.25 * ndl) + pow(ndh, 60.0) * 0.4, print.a);
        // Torn edges show the bright metal layer, thin and uneven.
        if (opened && u_part < 0.5) {
            float edge = 1.0 - smoothstep(0.0, 0.006 + 0.004 * hash(vec2(floor(uv.x * 160.0), 1.0)), uv.y - line);
            col = mix(col, vec3(0.92, 0.94, 1.0), edge);
        }
        if (u_part > 0.5 && v_free > 0.0) {
            float edge = 1.0 - smoothstep(0.0, 0.005, line - uv.y);
            col = mix(col, vec3(0.9, 0.92, 0.98), edge * v_free);
        }
    }
    o = vec4(col, u_alpha);
}
"#;

pub const QUAD_VS: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_pos;
uniform mat4 u_vp;
uniform mat4 u_model;
uniform vec4 u_rect;   // x, y, w, h in model units
out vec2 v_uv;
void main() {
    v_uv = vec2(a_pos.x, 1.0 - a_pos.y);
    vec2 p = u_rect.xy + (a_pos - 0.5) * u_rect.zw;
    gl_Position = u_vp * u_model * vec4(p, 0.0, 1.0);
}
"#;

pub const SPRITE_FS: &str = r#"#version 300 es
precision highp float;
in vec2 v_uv;
uniform sampler2D u_tex;
uniform vec4 u_tint;
uniform vec4 u_crop;   // u0, v0, u1, v1 of the texture to show
out vec4 o;
void main() {
    vec2 uv = mix(u_crop.xy, u_crop.zw, v_uv);
    vec4 t = texture(u_tex, uv);
    o = vec4(t.rgb * u_tint.rgb, t.a * u_tint.a);
}
"#;

pub const SHAPE_FS: &str = r#"#version 300 es
precision highp float;
in vec2 v_uv;
uniform vec4 u_color;
uniform float u_shape;   // 0 soft shadow, 1 four-point sparkle
out vec4 o;
void main() {
    vec2 p = v_uv * 2.0 - 1.0;
    float a;
    if (u_shape < 0.5) {
        a = exp(-dot(p, p) * 3.2) * smoothstep(1.0, 0.6, length(p));
    } else if (u_shape > 1.5) {
        // A soft rounded rectangle: a card's shadow on what is behind it.
        vec2 q = abs(p) - vec2(0.62, 0.68);
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0);
        a = 1.0 - smoothstep(-0.16, 0.3, d);
    } else {
        float star = max(1.0 - abs(p.x) * 6.0 - abs(p.y) * 1.2, 1.0 - abs(p.y) * 6.0 - abs(p.x) * 1.2);
        a = clamp(star, 0.0, 1.0) + exp(-dot(p, p) * 18.0) * 0.8;
    }
    o = vec4(u_color.rgb, u_color.a * a);
}
"#;

pub const RIBBON_VS: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec4 a_pos;   // world xyz, t along the ribbon
uniform mat4 u_vp;
out float v_t;
void main() {
    v_t = a_pos.w;
    gl_Position = u_vp * vec4(a_pos.xyz, 1.0);
}
"#;

pub const RIBBON_FS: &str = r#"#version 300 es
precision highp float;
in float v_t;
uniform vec4 u_color;
out vec4 o;
void main() {
    float a = u_color.a * smoothstep(1.0, 0.75, v_t) * smoothstep(0.0, 0.04, v_t);
    o = vec4(u_color.rgb * (0.9 + 0.2 * v_t), a);
}
"#;

/// One card plane: front face, back face, or the edge between them.
pub const CARD_VS: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_pos;
uniform mat4 u_vp;
uniform mat4 u_model;
uniform vec2 u_size;
uniform float u_z;
uniform float u_grow;
out vec2 v_uv;
out vec3 v_world;
out vec3 v_n;
void main() {
    v_uv = a_pos;
    vec2 p = (vec2(a_pos.x, 1.0 - a_pos.y) - 0.5) * (u_size + u_grow);
    vec4 w = u_model * vec4(p, u_z, 1.0);
    v_world = w.xyz;
    v_n = normalize(mat3(u_model) * vec3(0.0, 0.0, 1.0));
    gl_Position = u_vp * w;
}
"#;

pub const CARD_FS: &str = r#"#version 300 es
precision highp float;
in vec2 v_uv;
in vec3 v_world;
in vec3 v_n;
uniform sampler2D u_front;
uniform sampler2D u_back;
uniform float u_mode;      // 0 front plane, 1 back plane, 2 edge
uniform vec2 u_size;
uniform float u_grow;
uniform vec2 u_tilt;       // how the card is turned, for foil and glare
uniform vec3 u_eye;
uniform float u_alpha;
uniform float u_time;
uniform float u_has_front;
uniform vec2 u_glare;      // where the light (or the pointer) hits, in face uv
uniform float u_hover;
uniform vec4 u_clip;       // world-space plane; only the side where it is >= 0 draws
out vec4 o;

float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
vec3 hsv(float h, float s, float v) {
    vec3 k = clamp(abs(mod(h * 6.0 + vec3(0.0, 4.0, 2.0), 6.0) - 3.0) - 1.0, 0.0, 1.0);
    return v * mix(vec3(1.0), k, s);
}

// A thick clear coat over the face: two studio windows reflect in it and
// slide across as the card turns, a hot spot sits where the light hits, and
// the coat gathers bright along the rim and the top edge.
vec3 coat(vec3 col, vec3 n, vec3 v, vec2 uv, vec2 p, vec2 size, float d, float k, float spot) {
    vec3 r = reflect(-v, n);
    vec2 q = r.xy / max(r.z, 0.25);
    float s = q.x * 0.82 + q.y * 0.57;
    float w1 = smoothstep(-0.2, -0.15, s) * (1.0 - smoothstep(-0.05, 0.0, s));
    float w2 = smoothstep(0.06, 0.085, s) * (1.0 - smoothstep(0.13, 0.155, s));
    float fade = smoothstep(-0.8, -0.15, q.y) * (1.0 - smoothstep(0.6, 1.1, q.y));
    float bands = (w1 * 0.2 + w2 * 0.14) * fade * k;
    // Screen-like, so the bands still read on bright faces.
    col = 1.0 - (1.0 - col) * (1.0 - bands * 1.6);
    vec2 g = (uv - u_glare) * vec2(size.x / size.y, 1.0);
    col += vec3(1.0) * exp(-dot(g, g) * spot) * (0.04 + 0.26 * u_hover) * k;
    col *= 1.0 - smoothstep(0.15, 0.95, length(g)) * (0.04 + 0.1 * u_hover) * k;
    float e = -d;
    col += vec3(1.0) * (1.0 - smoothstep(0.001, 0.0035, e)) * 0.32 * k;
    float along = 1.0 - smoothstep(size.x * 0.5 - 0.08, size.x * 0.5 - 0.02, abs(p.x));
    col += vec3(1.0) * (1.0 - smoothstep(0.0, 0.016, size.y * 0.5 - p.y)) * 0.24 * k * along;
    col *= 1.0 - (1.0 - smoothstep(0.0, 0.03, p.y + size.y * 0.5)) * 0.16;
    return col;
}

void main() {
    // Cards still inside the pack show only above its torn opening.
    if (dot(vec4(v_world, 1.0), u_clip) < 0.0) discard;
    // Rounded corners with a soft, resolution-independent edge.
    vec2 size = u_size + u_grow;
    vec2 p = (v_uv - 0.5) * size;
    float r = 0.045 + u_grow * 0.5;
    vec2 q = abs(p) - (size * 0.5 - r);
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
    float aa = clamp(0.5 - d / fwidth(d), 0.0, 1.0);
    if (aa <= 0.0) discard;

    vec3 n = normalize(v_n);
    if (!gl_FrontFacing) n = -n;
    vec3 v = normalize(u_eye - v_world);
    float fres = pow(1.0 - max(dot(n, v), 0.0), 2.0);
    vec3 col;
    float alpha = u_alpha * aa;

    if (u_mode > 1.5) {
        // Card edge: a pale, slightly pearly layer between the faces.
        col = vec3(0.86, 0.86, 0.9) * (0.75 + 0.35 * fres);
    } else if (u_mode > 0.5) {
        if (gl_FrontFacing) discard;
        // Back: black gloss with one narrow, soft highlight that slides with the tilt.
        vec2 uv = vec2(1.0 - v_uv.x, v_uv.y);
        col = mix(vec3(0.045, 0.045, 0.052), vec3(0.02, 0.02, 0.026), uv.y);
        vec4 mark = texture(u_back, uv);
        col = mix(col, mark.rgb, mark.a);
        float s = uv.x * 0.78 + uv.y * 0.42;
        float pos = 0.62 + u_tilt.x * 0.9 - u_tilt.y * 0.5;
        col += vec3(0.85, 0.88, 0.95) * exp(-pow((s - pos) * 13.0, 2.0)) * 0.17;
        col += vec3(0.3, 0.32, 0.38) * fres * 0.12;
        vec2 bp = vec2(-p.x, p.y);
        col = coat(col, n, v, uv, bp, size, d, 0.55, 40.0);
    } else {
        if (!gl_FrontFacing) discard;
        if (u_has_front < 0.5) discard;
        vec4 base = texture(u_front, v_uv);
        col = base.rgb;
        // Holographic foil: hue shifts across the card and with the tilt.
        float h = fract(v_uv.x * 0.7 + v_uv.y * 0.45 + u_tilt.x * 0.65 + u_tilt.y * 0.4);
        vec3 foil = hsv(h, 0.5, 1.0);
        col = mix(col, col * 0.78 + foil * 0.38, 0.22);
        // Glitter specks that twinkle as the card turns.
        vec2 cell = floor(v_uv * vec2(150.0, 210.0));
        float g = hash(cell);
        float tw = 0.5 + 0.5 * sin(g * 60.0 + u_tilt.x * 24.0 + u_tilt.y * 18.0 + u_time * 2.0);
        // Fewer, softer specks on dark cards, where they would otherwise dominate.
        float lum = dot(base.rgb, vec3(0.299, 0.587, 0.114));
        col += step(mix(0.993, 0.986, lum), g) * tw * mix(0.32, 0.5, lum);
        col += vec3(1.0) * fres * 0.06;
        col = coat(col, n, v, v_uv, p, size, d, 1.0, 9.0);
    }
    o = vec4(col, alpha);
}
"#;

/// Voxel pets: three.js-style Lambert light (key, fill, ambient), with soft
/// per-vertex occlusion and a thin highlight on the shape's outer corners.
/// Growth cells reuse it with one instanced cube per cell.
pub const PET_VS: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;
layout(location = 3) in vec3 a_extra;   // colour a, colour b, occlusion
layout(location = 4) in vec4 a_edges;   // outer-corner flags: u=0, u=1, v=0, v=1
layout(location = 5) in vec4 i_cell;    // growth: centre, scale
layout(location = 6) in vec3 i_color;   // growth: colour a, colour b, blend
uniform mat4 u_vp;
uniform mat4 u_model;
uniform vec3 u_palette[30];
uniform float u_crack;   // how far the egg's crack colour has spread
uniform float u_cells;
out vec3 v_normal;
out vec2 v_uv;
out vec3 v_color;
out float v_ao;
out vec4 v_edges;
float smoother(float a, float b, float t) {
    float u = clamp((t - a) / (b - a), 0.0, 1.0);
    return u * u * u * (u * (u * 6.0 - 15.0) + 10.0);
}
void main() {
    vec3 p = a_pos;
    if (u_cells > 0.5) {
        p = a_pos * i_cell.w + i_cell.xyz;
        v_color = mix(u_palette[int(i_color.x + 0.5)], u_palette[int(i_color.y + 0.5)], i_color.z);
    } else {
        float k = (1.0 - a_pos.y / 14.0) * 0.3;
        float spread = u_crack > 0.0 ? smoother(k, 0.7 + k, u_crack) : 0.0;
        v_color = mix(u_palette[int(a_extra.x + 0.5)], u_palette[int(a_extra.y + 0.5)], spread);
    }
    v_normal = transpose(inverse(mat3(u_model))) * a_normal;
    v_uv = a_uv;
    v_ao = a_extra.z;
    v_edges = a_edges;
    gl_Position = u_vp * u_model * vec4(p, 1.0);
}
"#;

pub const PET_FS: &str = r#"#version 300 es
precision highp float;
in vec3 v_normal;
in vec2 v_uv;
in vec3 v_color;
in float v_ao;
in vec4 v_edges;
uniform vec3 u_key_dir;
uniform vec3 u_key;
uniform vec3 u_fill_dir;
uniform vec3 u_fill;
uniform vec3 u_ambient;
uniform float u_unlit;
uniform vec3 u_glow;
out vec4 o;
vec3 encode(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c));
}
void main() {
    if (u_unlit > 0.5) {
        o = vec4(encode(v_color * u_glow), 1.0);
        return;
    }
    vec3 n = normalize(v_normal);
    float key = max(dot(n, u_key_dir), 0.0);
    vec3 direct = key * u_key + max(dot(n, u_fill_dir), 0.0) * u_fill;
    vec3 col = v_color * (u_ambient * v_ao + direct * mix(1.0, v_ao, 0.35));
    // Outer corners catch a little light, fading out where voxels get tiny.
    vec4 d = mix(vec4(9.0), vec4(v_uv.x, 1.0 - v_uv.x, v_uv.y, 1.0 - v_uv.y), v_edges);
    float edge = min(min(d.x, d.y), min(d.z, d.w));
    float px = fwidth(v_uv.x) + fwidth(v_uv.y);
    float bevel = (1.0 - smoothstep(0.0, 0.085, edge)) * (1.0 - smoothstep(0.08, 0.3, px));
    col += bevel * 0.1 * (v_color + 0.12) * (0.55 + 0.45 * key);
    o = vec4(encode(col), 1.0);
}
"#;

/// The card's sides: a rounded band between the two faces, lit like the faces
/// and catching a bright line where it meets the front.
pub const RIM_VS: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
uniform mat4 u_vp;
uniform mat4 u_model;
uniform vec2 u_half;
uniform float u_radius;
out vec3 v_n;
out vec3 v_world;
out float v_z;
void main() {
    vec2 c = clamp(a_pos.xy, -(u_half - u_radius), u_half - u_radius);
    vec2 d = a_pos.xy - c;
    vec3 n = vec3(d / max(length(d), 1e-5), 0.0);
    v_n = mat3(u_model) * n;
    vec4 w = u_model * vec4(a_pos, 1.0);
    v_world = w.xyz;
    v_z = a_pos.z;
    gl_Position = u_vp * w;
}
"#;

pub const RIM_FS: &str = r#"#version 300 es
precision highp float;
in vec3 v_n;
in vec3 v_world;
in float v_z;
uniform vec3 u_color;
uniform vec3 u_eye;
uniform float u_alpha;
uniform float u_thick;
uniform vec4 u_clip;
out vec4 o;
void main() {
    if (dot(vec4(v_world, 1.0), u_clip) < 0.0) discard;
    vec3 n = normalize(v_n);
    vec3 v = normalize(u_eye - v_world);
    vec3 l = normalize(vec3(-0.4, 0.8, 0.6));
    float diff = 0.5 + 0.5 * max(dot(n, l), 0.0);
    float spec = pow(max(dot(n, normalize(l + v)), 0.0), 28.0) * 0.5;
    float t = clamp(v_z / u_thick + 0.5, 0.0, 1.0);
    vec3 col = u_color * diff * mix(0.72, 1.04, t) + spec + smoothstep(0.75, 1.0, t) * 0.16;
    o = vec4(col, u_alpha);
}
"#;
