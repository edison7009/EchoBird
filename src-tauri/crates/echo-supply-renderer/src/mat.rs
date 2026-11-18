//! A small column-major 4x4 matrix, as WebGL expects.

/// Column-major 4x4 matrix, as WebGL expects.
#[derive(Clone, Copy, Debug)]
pub struct Mat4(pub [f32; 16]);

impl Mat4 {
    pub fn identity() -> Self {
        let mut m = [0.0; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        Mat4(m)
    }
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let (a, b) = (&self.0, &o.0);
        let mut m = [0.0; 16];
        for c in 0..4 {
            for r in 0..4 {
                m[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
            }
        }
        Mat4(m)
    }
    pub fn translate(x: f32, y: f32, z: f32) -> Mat4 {
        let mut m = Mat4::identity();
        m.0[12] = x;
        m.0[13] = y;
        m.0[14] = z;
        m
    }
    pub fn scale(x: f32, y: f32, z: f32) -> Mat4 {
        let mut m = Mat4::identity();
        m.0[0] = x;
        m.0[5] = y;
        m.0[10] = z;
        m
    }
    pub fn rot_x(a: f32) -> Mat4 {
        let (s, c) = a.sin_cos();
        let mut m = Mat4::identity();
        m.0[5] = c;
        m.0[6] = s;
        m.0[9] = -s;
        m.0[10] = c;
        m
    }
    pub fn rot_y(a: f32) -> Mat4 {
        let (s, c) = a.sin_cos();
        let mut m = Mat4::identity();
        m.0[0] = c;
        m.0[2] = -s;
        m.0[8] = s;
        m.0[10] = c;
        m
    }
    pub fn rot_z(a: f32) -> Mat4 {
        let (s, c) = a.sin_cos();
        let mut m = Mat4::identity();
        m.0[0] = c;
        m.0[1] = s;
        m.0[4] = -s;
        m.0[5] = c;
        m
    }
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let f = 1.0 / (fov_y / 2.0).tan();
        let mut m = [0.0; 16];
        m[0] = f / aspect;
        m[5] = f;
        m[10] = (far + near) / (near - far);
        m[11] = -1.0;
        m[14] = 2.0 * far * near / (near - far);
        Mat4(m)
    }
    /// Transforms a point (w = 1).
    pub fn point(&self, p: [f32; 3]) -> [f32; 4] {
        let m = &self.0;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
            m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15],
        ]
    }

    /// Rotation in three.js's default Euler order (XYZ).
    pub fn euler(x: f32, y: f32, z: f32) -> Mat4 {
        Mat4::rot_x(x).mul(&Mat4::rot_y(y)).mul(&Mat4::rot_z(z))
    }
    /// Translation, rotation (XYZ Euler), scale: an Object3D's local matrix.
    pub fn trs(t: [f32; 3], r: [f32; 3], s: [f32; 3]) -> Mat4 {
        Mat4::translate(t[0], t[1], t[2])
            .mul(&Mat4::euler(r[0], r[1], r[2]))
            .mul(&Mat4::scale(s[0], s[1], s[2]))
    }
    /// A view matrix for a camera at `eye` looking at `target`, y up.
    pub fn look_at(eye: [f32; 3], target: [f32; 3]) -> Mat4 {
        let norm = |v: [f32; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            [v[0] / l, v[1] / l, v[2] / l]
        };
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let z = norm([eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]]);
        let x = norm(cross([0.0, 1.0, 0.0], z));
        let y = cross(z, x);
        Mat4([
            x[0],
            y[0],
            z[0],
            0.0,
            x[1],
            y[1],
            z[1],
            0.0,
            x[2],
            y[2],
            z[2],
            0.0,
            -dot(x, eye),
            -dot(y, eye),
            -dot(z, eye),
            1.0,
        ])
    }
    pub fn ortho(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
        let mut m = Mat4::identity();
        m.0[0] = 2.0 / (right - left);
        m.0[5] = 2.0 / (top - bottom);
        m.0[10] = -2.0 / (far - near);
        m.0[12] = -(right + left) / (right - left);
        m.0[13] = -(top + bottom) / (top - bottom);
        m.0[14] = -(far + near) / (far - near);
        m
    }
}
