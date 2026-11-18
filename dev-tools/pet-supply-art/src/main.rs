//! Renders EchoBird's original supply mascots, one layer at a time, so the web
//! renderer can blink and change expressions by swapping layers.
//!
//! cargo run -- --char echo --layer body --bg black --out frames/echo-body-black.png
//!
//! Characters: echo (pack), sprout (growth), twin (double), zip (haste).
//! Layers: body, eyes, joy, wince, smile, open.
//! Each layer is rendered on black and on white; `matte.py` turns the pair
//! into one image with exact transparency.

use bevy::{
    camera::ScalingMode,
    prelude::*,
    render::view::screenshot::{Capturing, Screenshot, save_to_disk},
    window::{WindowLevel, WindowResolution},
};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::PathBuf;

const SIZE: u32 = 1024;
const VIEW: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layer {
    Body,
    Eyes,
    Joy,
    Wince,
    Smile,
    Open,
}

#[derive(Component)]
#[allow(dead_code)]
struct Tag(Layer);

#[derive(Resource)]
struct Job {
    out: PathBuf,
    update: u32,
    requested: bool,
}

fn arg(flag: &str) -> Option<String> {
    std::env::args().skip_while(|a| a != flag).nth(1)
}

fn main() {
    let character = arg("--char").unwrap_or_else(|| "echo".into());
    let layer = match arg("--layer").as_deref().unwrap_or("body") {
        "eyes" => Layer::Eyes,
        "joy" => Layer::Joy,
        "wince" => Layer::Wince,
        "smile" => Layer::Smile,
        "open" => Layer::Open,
        _ => Layer::Body,
    };
    let white = arg("--bg").as_deref() == Some("white");
    let out = PathBuf::from(arg("--out").unwrap_or_else(|| "frame.png".into()));

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "EchoBird supply art".into(),
                resolution: WindowResolution::new(SIZE, SIZE).with_scale_factor_override(1.0),
                resizable: false,
                window_level: WindowLevel::AlwaysOnTop,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(if white { Color::WHITE } else { Color::BLACK }))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb(0.85, 0.9, 1.0),
            brightness: 700.0,
            ..default()
        })
        .insert_resource(Job {
            out,
            update: 0,
            requested: false,
        })
        .add_systems(
            Startup,
            move |mut commands: Commands,
                  mut meshes: ResMut<Assets<Mesh>>,
                  mut materials: ResMut<Assets<StandardMaterial>>| {
                setup(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &character,
                    layer,
                );
            },
        )
        .add_systems(Last, capture)
        .run();
}

fn capture(
    mut commands: Commands,
    mut job: ResMut<Job>,
    pending: Query<(), Or<(With<Screenshot>, With<Capturing>)>>,
    mut exit: MessageWriter<AppExit>,
) {
    job.update += 1;
    if job.update < 40 {
        return;
    }
    if !job.requested {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(job.out.clone()));
        job.requested = true;
    } else if pending.is_empty() {
        exit.write(AppExit::Success);
    }
}

/// A soft holographic tint: hue drifts with the surface direction.
fn holo(n: Vec3, base: f32, spread: f32, sat: f32, light: f32) -> [f32; 4] {
    let h = (base + spread * (0.55 * n.x + 0.35 * n.y + 0.2 * n.z)).rem_euclid(1.0);
    let c = Color::hsl(h * 360.0, sat, light).to_linear();
    [c.red, c.green, c.blue, 1.0]
}

fn tinted_sphere(
    meshes: &mut Assets<Mesh>,
    base: f32,
    spread: f32,
    sat: f32,
    light: f32,
    shape: impl Fn(Vec3) -> Vec3,
) -> Handle<Mesh> {
    let mut mesh = Sphere::new(1.0).mesh().uv(128, 64);
    let normals: Vec<Vec3> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => {
            v.iter().map(|n| Vec3::from(*n)).collect()
        }
        _ => unreachable!(),
    };
    let colors: Vec<[f32; 4]> = normals
        .iter()
        .map(|n| holo(*n, base, spread, sat, light))
        .collect();
    if let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            *p = shape(Vec3::from(*p)).into();
        }
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.compute_smooth_normals();
    meshes.add(mesh)
}

fn glossy(materials: &mut Assets<StandardMaterial>, color: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.14,
        reflectance: 0.75,
        ..default()
    })
}

/// Where a face feature sits on an ellipsoid body, facing out of the surface.
struct Body {
    center: Vec3,
    radii: Vec3,
}

impl Body {
    fn surface(&self, x: f32, y: f32, lift: f32) -> Transform {
        let r = self.radii;
        let zz = (1.0 - (x / r.x).powi(2) - (y / r.y).powi(2)).max(0.0);
        let z = r.z * zz.sqrt();
        let normal = Vec3::new(x / (r.x * r.x), y / (r.y * r.y), z / (r.z * r.z)).normalize();
        let p = self.center + Vec3::new(x, y, z) + normal * lift;
        Transform::from_translation(p).with_rotation(Quat::from_rotation_arc(Vec3::Z, normal))
    }
}

struct Face {
    eye_x: f32,
    eye_y: f32,
    mouth_y: f32,
    scale: f32,
    mouth: bool,
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    commands: &mut Commands,
    show: Layer,
    tag: Layer,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
) {
    commands.spawn((
        Tag(tag),
        Mesh3d(mesh),
        MeshMaterial3d(material),
        transform,
        if tag == show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
    ));
}

fn face(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    show: Layer,
    body: &Body,
    f: &Face,
) {
    let s = f.scale;
    // Eyes a little larger than life, like the glossy toy look of the cards.
    let e = s * 1.22;
    let ink = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.035, 0.06),
        perceptual_roughness: 0.08,
        reflectance: 0.9,
        ..default()
    });
    let shine = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    });
    let blush = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.45, 0.55, 0.55),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let dot = meshes.add(Sphere::new(1.0).mesh().uv(48, 24));
    let stroke = meshes.add(Capsule3d::new(0.028 * s, 0.15 * s));

    for side in [-1.0f32, 1.0] {
        let x = side * f.eye_x;
        // Open eyes: glossy dark ovals with two catch-lights.
        let base = body.surface(x, f.eye_y, 0.0);
        spawn(
            commands,
            show,
            Layer::Eyes,
            dot.clone(),
            ink.clone(),
            base.with_scale(Vec3::new(0.105, 0.15, 0.05) * e),
        );
        spawn(
            commands,
            show,
            Layer::Eyes,
            dot.clone(),
            shine.clone(),
            body.surface(x + 0.035 * e, f.eye_y + 0.055 * e, 0.05 * e)
                .with_scale(Vec3::splat(0.045 * e)),
        );
        spawn(
            commands,
            show,
            Layer::Eyes,
            dot.clone(),
            shine.clone(),
            body.surface(x - 0.035 * e, f.eye_y - 0.05 * e, 0.05 * e)
                .with_scale(Vec3::splat(0.02 * e)),
        );
        // Happy ^ ^ eyes and wincing > < eyes: two short strokes each.
        for (layer, a, b) in [
            (Layer::Joy, 0.6f32, -0.6f32),
            (Layer::Wince, 0.45 * side, 0.45 * side),
        ] {
            let flip = if layer == Layer::Wince { 1.0 } else { 0.0 };
            for (k, angle) in [(-1.0f32, a), (1.0, b)] {
                let (dx, dy) = if flip > 0.0 {
                    (0.0, k * 0.05 * s)
                } else {
                    (k * 0.052 * s, 0.0)
                };
                let t = body.surface(x + dx, f.eye_y + dy, 0.01 * s);
                let tilt = if flip > 0.0 {
                    // > for the left eye, < for the right: strokes meet toward the nose.
                    Quat::from_rotation_z(FRAC_PI_2 + k * side * 0.7)
                } else {
                    Quat::from_rotation_z(FRAC_PI_2 - k * 0.0 + angle)
                };
                spawn(
                    commands,
                    show,
                    layer,
                    stroke.clone(),
                    ink.clone(),
                    t.with_rotation(t.rotation * tilt),
                );
            }
        }
        // Cheeks belong to the body layer.
        spawn(
            commands,
            show,
            Layer::Body,
            dot.clone(),
            blush.clone(),
            body.surface(side * (f.eye_x + 0.17 * s), f.eye_y - 0.15 * s, 0.01)
                .with_scale(Vec3::new(0.11, 0.065, 0.02) * s),
        );
    }

    if f.mouth {
        // Smile: a row of small beads along a shallow arc reads as one stroke.
        for i in 0..=14 {
            let a = PI * (1.15 + 0.7 * i as f32 / 14.0);
            let x = 0.085 * s * a.cos();
            let y = f.mouth_y + 0.05 * s + 0.085 * s * a.sin();
            spawn(
                commands,
                show,
                Layer::Smile,
                dot.clone(),
                ink.clone(),
                body.surface(x, y, 0.0).with_scale(Vec3::splat(0.022 * s)),
            );
        }
        // Open "o" mouth for surprise.
        spawn(
            commands,
            show,
            Layer::Open,
            dot.clone(),
            ink.clone(),
            body.surface(0.0, f.mouth_y, 0.0)
                .with_scale(Vec3::new(0.05, 0.06, 0.03) * s),
        );
    }
}

fn setup(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    character: &str,
    show: Layer,
) {
    commands.spawn((
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: VIEW,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Key light from the upper left, a cool fill from the right, and a rim from behind.
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            ..default()
        },
        Transform::from_xyz(-3.0, 4.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2_600.0,
            color: Color::srgb(0.75, 0.85, 1.0),
            ..default()
        },
        Transform::from_xyz(5.0, -1.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 3_500.0,
            ..default()
        },
        Transform::from_xyz(1.0, 3.0, -6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let white = glossy(materials, Color::WHITE);
    let body_layer = Layer::Body;

    match character {
        // Echo: a round chick for the pack front, pastel holographic.
        "echo" => {
            let radii = Vec3::new(1.0, 0.93, 0.92);
            let mesh = tinted_sphere(meshes, 0.85, 0.55, 0.75, 0.80, |p| p * radii);
            spawn(
                commands,
                show,
                body_layer,
                mesh,
                white.clone(),
                Transform::default(),
            );
            let wing = tinted_sphere(meshes, 0.72, 0.4, 0.7, 0.74, |p| {
                p * Vec3::new(0.26, 0.42, 0.18)
            });
            for side in [-1.0f32, 1.0] {
                spawn(
                    commands,
                    show,
                    body_layer,
                    wing.clone(),
                    white.clone(),
                    Transform::from_xyz(side * 0.93, -0.12, 0.12)
                        .with_rotation(Quat::from_rotation_z(side * 0.55)),
                );
            }
            let tuft = tinted_sphere(meshes, 0.6, 0.35, 0.8, 0.72, |p| {
                p * Vec3::new(0.09, 0.24, 0.09)
            });
            for (dx, rz) in [(-0.12f32, 0.5f32), (0.0, 0.0), (0.12, -0.5)] {
                spawn(
                    commands,
                    show,
                    body_layer,
                    tuft.clone(),
                    white.clone(),
                    Transform::from_xyz(dx, 1.0, 0.05).with_rotation(Quat::from_rotation_z(rz)),
                );
            }
            let beak = meshes.add(Cone {
                radius: 0.12,
                height: 0.2,
            });
            let beak_mat = glossy(materials, Color::srgb(1.0, 0.62, 0.22));
            let body = Body {
                center: Vec3::ZERO,
                radii,
            };
            let t = body.surface(0.0, -0.08, 0.05);
            spawn(
                commands,
                show,
                body_layer,
                beak,
                beak_mat,
                t.with_rotation(t.rotation * Quat::from_rotation_x(FRAC_PI_2)),
            );
            face(
                commands,
                meshes,
                materials,
                show,
                &body,
                &Face {
                    eye_x: 0.3,
                    eye_y: 0.14,
                    mouth_y: -0.2,
                    scale: 1.0,
                    mouth: false,
                },
            );
        }
        // Sprout (growth): a green bud with two leaves on top.
        "sprout" => {
            let radii = Vec3::new(0.92, 0.96, 0.86);
            let mesh = tinted_sphere(meshes, 0.36, 0.22, 0.7, 0.58, |p| {
                let taper = if p.y > 0.0 {
                    1.0 - 0.32 * p.y.powi(3)
                } else {
                    1.0
                };
                Vec3::new(p.x * taper, p.y, p.z * taper) * radii
            });
            spawn(
                commands,
                show,
                body_layer,
                mesh,
                white.clone(),
                Transform::from_xyz(0.0, -0.12, 0.0),
            );
            let leaf = tinted_sphere(meshes, 0.33, 0.08, 0.95, 0.5, |p| {
                p * Vec3::new(0.34, 0.16, 0.24)
            });
            for side in [-1.0f32, 1.0] {
                spawn(
                    commands,
                    show,
                    body_layer,
                    leaf.clone(),
                    white.clone(),
                    Transform::from_xyz(side * 0.24, 0.98, 0.05).with_rotation(
                        Quat::from_rotation_z(side * 0.55) * Quat::from_rotation_x(0.65),
                    ),
                );
            }
            let stem = meshes.add(Capsule3d::new(0.04, 0.12));
            spawn(
                commands,
                show,
                body_layer,
                stem,
                glossy(materials, Color::srgb(0.3, 0.7, 0.35)),
                Transform::from_xyz(0.0, 0.86, 0.0),
            );
            let body = Body {
                center: Vec3::new(0.0, -0.12, 0.0),
                radii,
            };
            face(
                commands,
                meshes,
                materials,
                show,
                &body,
                &Face {
                    eye_x: 0.29,
                    eye_y: 0.12,
                    mouth_y: -0.12,
                    scale: 1.0,
                    mouth: true,
                },
            );
        }
        // Twin (double): two merged bubbles, warm gold to pink.
        "twin" => {
            let back = tinted_sphere(meshes, 0.92, 0.25, 0.8, 0.72, |p| p * 0.62);
            spawn(
                commands,
                show,
                body_layer,
                back,
                white.clone(),
                Transform::from_xyz(0.62, 0.55, -0.45),
            );
            let radii = Vec3::new(0.95, 0.9, 0.88);
            let mesh = tinted_sphere(meshes, 0.1, 0.22, 0.85, 0.66, |p| p * radii);
            spawn(
                commands,
                show,
                body_layer,
                mesh,
                white.clone(),
                Transform::from_xyz(-0.18, -0.18, 0.0),
            );
            let body = Body {
                center: Vec3::new(-0.18, -0.18, 0.0),
                radii,
            };
            face(
                commands,
                meshes,
                materials,
                show,
                &body,
                &Face {
                    eye_x: 0.28,
                    eye_y: 0.12,
                    mouth_y: -0.12,
                    scale: 1.0,
                    mouth: true,
                },
            );
        }
        // Puff: a little cloud of merged puffs, sky blue to lilac.
        "puff" => {
            for (x, y, z, r, hue) in [
                (-0.62f32, -0.18f32, -0.1f32, 0.5f32, 0.6f32),
                (0.64, -0.16, -0.1, 0.52, 0.72),
                (-0.3, 0.42, -0.25, 0.5, 0.64),
                (0.32, 0.46, -0.28, 0.55, 0.7),
            ] {
                let lump = tinted_sphere(meshes, hue, 0.2, 0.7, 0.8, |p| p * r);
                spawn(
                    commands,
                    show,
                    body_layer,
                    lump,
                    white.clone(),
                    Transform::from_xyz(x, y, z),
                );
            }
            let radii = Vec3::new(0.86, 0.74, 0.8);
            let mesh = tinted_sphere(meshes, 0.63, 0.22, 0.72, 0.8, |p| p * radii);
            spawn(
                commands,
                show,
                body_layer,
                mesh,
                white.clone(),
                Transform::from_xyz(0.0, -0.12, 0.08),
            );
            let body = Body {
                center: Vec3::new(0.0, -0.12, 0.08),
                radii,
            };
            face(
                commands,
                meshes,
                materials,
                show,
                &body,
                &Face {
                    eye_x: 0.27,
                    eye_y: 0.1,
                    mouth_y: -0.14,
                    scale: 1.0,
                    mouth: true,
                },
            );
        }
        // Zip (haste): a blue comet with a swept tail.
        _ => {
            let tail = tinted_sphere(meshes, 0.55, 0.18, 0.75, 0.72, |p| {
                // A teardrop stretched along -x.
                let t = ((-p.x) * 0.5 + 0.5).clamp(0.0, 1.0);
                Vec3::new(
                    p.x * 0.9 - t * 0.9,
                    p.y * (1.0 - 0.75 * t),
                    p.z * (1.0 - 0.75 * t),
                ) * 0.85
            });
            for (dy, s, rz) in [
                (0.18f32, 0.8f32, -0.25f32),
                (-0.12, 1.0, -0.15),
                (-0.38, 0.7, -0.05),
            ] {
                spawn(
                    commands,
                    show,
                    body_layer,
                    tail.clone(),
                    white.clone(),
                    Transform::from_xyz(-0.15, dy * 0.9, -0.35)
                        .with_rotation(Quat::from_rotation_z(rz + 0.35))
                        .with_scale(Vec3::splat(s * 0.72)),
                );
            }
            let radii = Vec3::new(0.86, 0.84, 0.82);
            let mesh = tinted_sphere(meshes, 0.56, 0.24, 0.8, 0.62, |p| p * radii);
            spawn(
                commands,
                show,
                body_layer,
                mesh,
                white.clone(),
                Transform::from_xyz(0.25, 0.0, 0.0),
            );
            let body = Body {
                center: Vec3::new(0.25, 0.0, 0.0),
                radii,
            };
            face(
                commands,
                meshes,
                materials,
                show,
                &body,
                &Face {
                    eye_x: 0.26,
                    eye_y: 0.1,
                    mouth_y: -0.13,
                    scale: 0.95,
                    mouth: true,
                },
            );
        }
    }
    let _ = TAU;
}
