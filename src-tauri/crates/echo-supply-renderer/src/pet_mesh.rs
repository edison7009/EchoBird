//! Turns voxel maps into indexed triangle meshes for the pet scene.
//!
//! Faces between voxels of the same map are culled, exactly like the
//! original three.js geometry. Two refinements on top of the approved look:
//! per-vertex ambient occlusion darkens creases and corners, and each face
//! records which of its edges are real outer corners of the shape, so the
//! shader can catch a thin highlight there instead of on every voxel seam.

use crate::pet_models::{Cell, Definition, VoxelMap};

/// Floats per vertex: position 3, normal 3, face uv 2, colour a, colour b,
/// occlusion, then four edge flags (u = 0, u = 1, v = 0, v = 1).
pub const STRIDE: usize = 15;

#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub vertices: Vec<f32>,
    pub indices: Vec<u16>,
    /// (first index, index count) per part, in definition order.
    pub ranges: Vec<(u32, u32)>,
}

/// The six faces, as in the original geometry: normal and four corners,
/// triangulated (0, 1, 2) and (2, 1, 3).
const FACES: [([i32; 3], [[i32; 3]; 4]); 6] = [
    ([1, 0, 0], [[1, 0, 1], [1, 0, 0], [1, 1, 1], [1, 1, 0]]),
    ([-1, 0, 0], [[0, 0, 0], [0, 0, 1], [0, 1, 0], [0, 1, 1]]),
    ([0, 1, 0], [[0, 1, 1], [1, 1, 1], [0, 1, 0], [1, 1, 0]]),
    ([0, -1, 0], [[0, 0, 0], [1, 0, 0], [0, 0, 1], [1, 0, 1]]),
    ([0, 0, 1], [[0, 0, 1], [1, 0, 1], [0, 1, 1], [1, 1, 1]]),
    ([0, 0, -1], [[1, 0, 0], [0, 0, 0], [1, 1, 0], [0, 1, 0]]),
];

/// Brightness for 0..=3 open neighbours around a vertex.
const AO: [f32; 4] = [0.6, 0.76, 0.89, 1.0];

fn add(a: Cell, b: [i32; 3]) -> Cell {
    (a.0 + b[0], a.1 + b[1], a.2 + b[2])
}

fn unit(axis: usize, sign: i32) -> [i32; 3] {
    let mut v = [0; 3];
    v[axis] = sign;
    v
}

impl MeshData {
    /// Appends one voxel map as a new range. `faces_of` decides which faces
    /// exist (culling), `solid` drives occlusion and edges, `alt` gives the
    /// second colour per cell (the cracked egg), and positions are relative
    /// to `origin`.
    pub fn push(
        &mut self,
        map: &VoxelMap,
        solid: &VoxelMap,
        alt: Option<&VoxelMap>,
        origin: [f64; 3],
    ) {
        let first = self.indices.len() as u32;
        for v in map.iter() {
            let cell = v.cell();
            let color_b = alt.and_then(|m| m.get(cell)).map_or(v.color, |w| w.color);
            for (normal, corners) in FACES {
                if map.contains(add(cell, normal)) {
                    continue;
                }
                let axis = normal.iter().position(|n| *n != 0).unwrap_or(0);
                let (ta, tb) = ((axis + 1) % 3, (axis + 2) % 3);
                let above = add(cell, normal);
                let filled = |c: Cell| solid.contains(c);
                // A side of the face is a real corner when the surface does
                // not continue flat past it and does not fold up into a crease.
                let edge = |dir: [i32; 3]| {
                    let side = add(cell, dir);
                    f32::from(u8::from(!filled(side) && !filled(add(side, normal))))
                };
                let edges = [
                    edge(unit(ta, -1)),
                    edge(unit(ta, 1)),
                    edge(unit(tb, -1)),
                    edge(unit(tb, 1)),
                ];
                let base = (self.vertices.len() / STRIDE) as u16;
                let mut ao = [0usize; 4];
                for (k, corner) in corners.iter().enumerate() {
                    let da = unit(ta, if corner[ta] == 1 { 1 } else { -1 });
                    let db = unit(tb, if corner[tb] == 1 { 1 } else { -1 });
                    let s1 = filled(add(above, da));
                    let s2 = filled(add(above, db));
                    let cr = filled(add(add(above, da), db));
                    ao[k] = if s1 && s2 {
                        0
                    } else {
                        3 - usize::from(s1) - usize::from(s2) - usize::from(cr)
                    };
                    let p = [
                        (v.x + corner[0]) as f64 - origin[0],
                        (v.y + corner[1]) as f64 - origin[1],
                        (v.z + corner[2]) as f64 - origin[2],
                    ];
                    self.vertices.extend_from_slice(&[
                        p[0] as f32,
                        p[1] as f32,
                        p[2] as f32,
                        normal[0] as f32,
                        normal[1] as f32,
                        normal[2] as f32,
                        corner[ta] as f32,
                        corner[tb] as f32,
                        v.color as f32,
                        color_b as f32,
                        AO[ao[k]],
                    ]);
                    self.vertices.extend_from_slice(&edges);
                }
                // Split along the brighter diagonal so occlusion shades evenly.
                if ao[0] + ao[3] > ao[1] + ao[2] {
                    self.indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 3,
                        base,
                        base + 3,
                        base + 2,
                    ]);
                } else {
                    self.indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base + 2,
                        base + 1,
                        base + 3,
                    ]);
                }
            }
        }
        self.ranges.push((first, self.indices.len() as u32 - first));
    }

    /// One range per part, each relative to its pivot. Occlusion and edges
    /// look at the whole pet so joints between parts read as one surface.
    pub fn for_definition(def: &Definition) -> MeshData {
        let mut mesh = MeshData::default();
        for part in &def.parts {
            mesh.push(&part.voxels, &def.voxels, None, part.pivot);
        }
        mesh
    }

    /// A unit cube centred on the origin, for growth cells.
    pub fn cube() -> MeshData {
        let mut map = VoxelMap::default();
        map.set(crate::pet_models::Voxel {
            x: 0,
            y: 0,
            z: 0,
            color: 0,
        });
        let mut mesh = MeshData::default();
        mesh.push(&map, &map, None, [0.5, 0.5, 0.5]);
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet_models::build;

    #[test]
    fn every_pet_mesh_fits_16_bit_indices_and_covers_each_part() {
        let m = build();
        for d in &m.definitions {
            let mesh = MeshData::for_definition(d);
            assert!(mesh.vertices.len() / STRIDE < u16::MAX as usize);
            assert_eq!(mesh.ranges.len(), d.parts.len());
            for (part, range) in d.parts.iter().zip(&mesh.ranges) {
                assert_eq!(
                    range.1 > 0,
                    !part.voxels.is_empty(),
                    "{} {}",
                    d.kind,
                    part.name
                );
            }
        }
    }

    #[test]
    fn a_lone_cube_has_six_open_faces_with_all_corners_lit() {
        let mesh = MeshData::cube();
        assert_eq!(mesh.indices.len(), 36);
        for v in mesh.vertices.chunks(STRIDE) {
            assert_eq!(v[10], 1.0);
            assert_eq!(&v[11..15], &[1.0; 4]);
        }
    }
}
