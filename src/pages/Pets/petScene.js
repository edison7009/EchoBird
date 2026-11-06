import * as THREE from 'three';
import { createVoxelModels } from './voxelModels.js';
import { PET_TINTS } from './petConstants';

export function createPetScene(canvas) {
  const {
    families,
    P,
    definitions,
    egg,
    cracked,
    shells,
    smooth,
    growthPhase,
    transitions,
    growthCell,
    key,
  } = createVoxelModels();
  const tints = PET_TINTS;
  let current = { id: 1, kind: 'mecha', stage: 2, color: 0 };
  const originalColors = new WeakMap(),
    tintMaps = new Map();
  let lastTint = '';
  const faceDefs = [
    {
      n: [1, 0, 0],
      v: [
        [1, 0, 1],
        [1, 0, 0],
        [1, 1, 1],
        [1, 1, 0],
      ],
    },
    {
      n: [-1, 0, 0],
      v: [
        [0, 0, 0],
        [0, 0, 1],
        [0, 1, 0],
        [0, 1, 1],
      ],
    },
    {
      n: [0, 1, 0],
      v: [
        [0, 1, 1],
        [1, 1, 1],
        [0, 1, 0],
        [1, 1, 0],
      ],
    },
    {
      n: [0, -1, 0],
      v: [
        [0, 0, 0],
        [1, 0, 0],
        [0, 0, 1],
        [1, 0, 1],
      ],
    },
    {
      n: [0, 0, 1],
      v: [
        [0, 0, 1],
        [1, 0, 1],
        [0, 1, 1],
        [1, 1, 1],
      ],
    },
    {
      n: [0, 0, -1],
      v: [
        [1, 0, 0],
        [0, 0, 0],
        [1, 1, 0],
        [0, 1, 0],
      ],
    },
  ];
  const material = new THREE.MeshLambertMaterial({ vertexColors: true }),
    geometries = [];
  function geometry(voxels, origin = [0, 0, 0]) {
    const vertices = [],
      normals = [],
      colors = [],
      indices = [];
    for (const v of voxels.values()) {
      const color = new THREE.Color(v.color);
      for (const face of faceDefs) {
        if (voxels.has(key(v.x + face.n[0], v.y + face.n[1], v.z + face.n[2]))) continue;
        const base = vertices.length / 3;
        for (const p of face.v) {
          vertices.push(v.x + p[0] - origin[0], v.y + p[1] - origin[1], v.z + p[2] - origin[2]);
          normals.push(...face.n);
          colors.push(color.r, color.g, color.b);
        }
        indices.push(base, base + 1, base + 2, base + 2, base + 1, base + 3);
      }
    }
    const result = new THREE.BufferGeometry();
    result.setAttribute('position', new THREE.Float32BufferAttribute(vertices, 3));
    result.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
    result.setAttribute('color', new THREE.Float32BufferAttribute(colors, 3));
    result.setIndex(indices);
    geometries.push(result);
    return result;
  }
  function makeRig(def) {
    const outer = new THREE.Group(),
      rootGroup = new THREE.Group(),
      nodes = new Map();
    outer.add(rootGroup);
    for (const part of def.parts) {
      const group = new THREE.Group(),
        parent = part.parent ? def.parts.find((p) => p.name === part.parent) : null;
      const bind = part.pivot.map((v, i) => v - (parent ? parent.pivot[i] : 0));
      group.position.set(...bind);
      if (!part.geometry) part.geometry = geometry(part.voxels, part.pivot);
      group.add(new THREE.Mesh(part.geometry, material));
      nodes.set(part.name, { group, bind });
      (part.parent ? nodes.get(part.parent).group : rootGroup).add(group);
    }
    return { def, outer, root: rootGroup, nodes, motion: 0 };
  }
  const rigs = definitions.map(makeRig),
    hatchRigs = families.map((f) =>
      makeRig(definitions.find((d) => d.kind === f.id && d.age === 0))
    );
  function poseRig(rig, t, action) {
    for (const n of rig.nodes.values()) {
      n.group.position.set(...n.bind);
      n.group.rotation.set(0, 0, 0);
      n.group.scale.set(1, 1, 1);
    }
    rig.root.position.set(0, 0, 0);
    rig.root.rotation.set(0, 0, 0);
    rig.root.scale.set(1, 1, 1);
    const node = (name) => rig.nodes.get(name)?.group;
    const s = rig.def.age,
      e = t <= 0 || t >= 1 ? 0 : Math.sin(Math.PI * t),
      wave = Math.sin(t * Math.PI * 2),
      bounce = Math.sin(t * Math.PI * 4);
    const body = node('body'),
      head = node('head');
    if (body) body.scale.y = 1 + 0.018 * wave;
    if (head) {
      head.rotation.y = 0.09 * wave;
      head.rotation.z = 0.025 * wave;
    }
    const blink = t > 0.36 && t < 0.46 ? Math.sin(((t - 0.36) / 0.1) * Math.PI) : 0;
    if (node('eyes') && rig.def.kind !== 'mecha') node('eyes').scale.y = 1 - 0.88 * blink;
    if (node('tail')) node('tail').rotation.y = 0.1 * wave;
    for (let i = 0; i < (rig.def.tailCount || 0); i++)
      node('tail-' + i).rotation.y = 0.045 * Math.sin(t * Math.PI * 2 + i * 0.5) * e;
    if (action === 'special') {
      if (rig.def.kind === 'unicorn') {
        node('right-front').rotation.x = -0.65 * e;
        head.rotation.x = 0.22 * e;
        node('tail').rotation.y = 0.35 * wave;
      } else if (rig.def.kind === 'mecha') {
        head.rotation.y = 0.35 * wave;
        node('right-arm').rotation.z = 1.15 * e;
        node('right-arm').rotation.x = -0.35 * e;
        node('left-arm').rotation.z = -0.16 * e;
        body.rotation.y = 0.08 * wave;
      } else if (rig.def.kind === 'kaiju') {
        head.rotation.x = -0.2 * e;
        node('jaw').rotation.x = 0.65 * e;
        node('right-arm').rotation.x = -0.45 * e;
        node('left-arm').rotation.x = -0.45 * e;
        node('tail').rotation.y = 0.28 * wave;
        body.rotation.x = -0.08 * e;
      } else if (rig.def.kind === 'fox') {
        head.rotation.z = 0.22 * e;
        node('right-front').rotation.x = -0.5 * e;
        for (let i = 0; i < rig.def.tailCount; i++) {
          const tail = node('tail-' + i);
          tail.rotation.z = -Math.sign(i - (rig.def.tailCount - 1) / 2) * 0.13 * e;
          tail.rotation.y = 0.12 * Math.sin(t * Math.PI * 2 + i * 0.4) * e;
        }
      } else if (rig.def.kind === 'alien') {
        head.rotation.z = 0.15 * wave;
        node('right-arm').rotation.z = 1.7 * e;
        node('right-arm').rotation.x = 0.2 * Math.sin(t * Math.PI * 8) * e;
        body.rotation.y = 0.1 * e;
      } else if (rig.def.kind === 'kangaroo') {
        rig.root.position.y = Math.max(0, Math.sin(t * Math.PI * 2)) * [1, 1.4, 1.8][s];
        body.rotation.x = 0.2 * e;
        head.rotation.x = 0.16 * e;
        node('right-arm').rotation.z = 1.15 * e;
        if (node('hat')) node('hat').rotation.z = 0.22 * e;
        node('tail').rotation.x = 0.12 * e;
      }
    }
    rig.motion =
      Array.from(rig.nodes.values()).reduce(
        (n, { group: g }) =>
          n +
          Math.abs(g.rotation.x) +
          Math.abs(g.rotation.y) +
          Math.abs(g.rotation.z) +
          Math.abs(g.scale.y - 1),
        0
      ) + Math.abs(rig.root.position.y);
    return rig;
  }
  const cubeGeometry = new THREE.BoxGeometry(1, 1, 1),
    growthMaterial = new THREE.MeshLambertMaterial(),
    growthGroup = new THREE.Group(),
    growthMeshes = new Map();
  geometries.push(cubeGeometry);
  const matrix = new THREE.Matrix4(),
    cubePosition = new THREE.Vector3(),
    cubeScale = new THREE.Vector3(),
    cubeRotation = new THREE.Quaternion(),
    cubeColor = new THREE.Color();
  function poseGrowth(t, kind) {
    const phase = growthPhase(t),
      id = kind + '-' + phase.step;
    if (!growthMeshes.has(id)) {
      const def = transitions.find((d) => d.kind === kind && d.step === phase.step),
        mesh = new THREE.InstancedMesh(cubeGeometry, growthMaterial, def.tracks.length);
      mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
      mesh.frustumCulled = false;
      growthGroup.add(mesh);
      growthMeshes.set(id, {
        mesh,
        def,
        colors: def.tracks.map((v) => [paletteColor(v.ca), paletteColor(v.cb)]),
      });
    }

    for (const [key, value] of growthMeshes) {
      const { mesh, def, colors } = value;
      mesh.visible = key === id;
      if (!mesh.visible) continue;
      def.tracks.forEach((track, i) => {
        const cell = growthCell(track, phase.amount);
        cubePosition.set(...cell.p);
        cubeScale.setScalar(Math.max(0.000001, cell.scale));
        matrix.compose(cubePosition, cubeRotation, cubeScale);
        mesh.setMatrixAt(i, matrix);
        mesh.setColorAt(i, cubeColor.copy(colors[i][0]).lerp(colors[i][1], cell.u));
      });
      mesh.instanceMatrix.needsUpdate = true;
      mesh.instanceColor.needsUpdate = true;
    }
    const from = definitions.find((d) => d.kind === kind && d.age === phase.step),
      to = definitions.find((d) => d.kind === kind && d.age === phase.step + 1);
    growthGroup.position.y = -(from.height + (to.height - from.height) * phase.amount) / 2;
  }
  const eggGeometry = geometry(egg),
    crackGeometry = geometry(cracked),
    shellGeometry = shells.map((m) => geometry(m));
  const eggGroups = [eggGeometry].map((g) => {
    const group = new THREE.Group(),
      mesh = new THREE.Mesh(g, material);
    mesh.position.y = -7;
    group.add(mesh);
    return group;
  });
  const hatchGroup = new THREE.Group(),
    wobble = new THREE.Group();
  hatchGroup.add(wobble);
  const hatchEggGeometry = geometry(egg),
    hatchEgg = new THREE.Mesh(hatchEggGeometry, material);
  hatchEgg.position.y = -7;
  wobble.add(hatchEgg);
  const shellBaseColors = eggGeometry.attributes.color.array,
    shellCrackColors = crackGeometry.attributes.color.array;
  const shellOrigins = [
    [-3, 4, 0],
    [3, 4, 0],
    [0, 10, 0],
  ];
  const pieces = shellGeometry.map((g, i) => {
    const group = new THREE.Group(),
      mesh = new THREE.Mesh(g, material),
      p = shellOrigins[i];
    mesh.position.set(-p[0], -p[1], -p[2]);
    group.add(mesh);
    wobble.add(group);
    return group;
  });
  const coreVoxels = new Map();
  for (let x = -2; x <= 2; x++)
    for (let y = -2; y <= 2; y++)
      for (let z = -2; z <= 2; z++) {
        if (x * x + y * y + z * z > 4) continue;
        coreVoxels.set(key(x, y, z), { x, y, z, color: y + z > 0 ? '#ffffff' : '#d4e3ec' });
      }
  const coreMaterial = new THREE.MeshBasicMaterial({ vertexColors: true });
  const core = new THREE.Mesh(geometry(coreVoxels, [0.5, 0.5, 0.5]), coreMaterial);
  core.name = 'egg-core';
  core.position.set(0, 1.5, 1);
  core.visible = false;
  wobble.add(core);
  for (const rig of hatchRigs) hatchGroup.add(rig.outer);
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const crackedEggProgress = 0.46;
  function poseHatch(t, kind) {
    const opening = smooth(0.34, 0.78, t),
      birth = smooth(0.38, 0.84, t),
      crack = smooth(0.12, 0.34, t);
    wobble.rotation.z = reduced
      ? 0
      : Math.sin(t * 52) * 0.11 * Math.sin(Math.PI * smooth(0, 0.44, t)) ** 2;
    wobble.scale.setScalar(1);
    const coreScale = 1 - smooth(crackedEggProgress, 0.72, t);
    core.visible = t >= 0.34 && coreScale > 0.001;
    core.scale.setScalar(coreScale);
    hatchEgg.visible = t < 0.34;
    const colors = hatchEggGeometry.attributes.color,
      positions = hatchEggGeometry.attributes.position;
    for (let i = 0; i < colors.count; i++) {
      const spread = smooth(
        (1 - positions.getY(i) / 14) * 0.3,
        0.7 + (1 - positions.getY(i) / 14) * 0.3,
        crack
      );
      for (let a = 0; a < 3; a++)
        colors.array[i * 3 + a] =
          shellBaseColors[i * 3 + a] +
          (shellCrackColors[i * 3 + a] - shellBaseColors[i * 3 + a]) * spread;
    }
    colors.needsUpdate = true;
    pieces.forEach((piece, i) => {
      const p = shellOrigins[i];
      piece.scale.setScalar(1);
      piece.visible = t >= 0.34;
      piece.rotation.set(0, 0, 0);
      piece.position.set(p[0], p[1] - 7, p[2]);
      if (i < 2) {
        piece.position.x += (i === 0 ? -8 : 8) * opening;
        piece.position.y += 1.3 * Math.sin(opening * Math.PI) - 0.6 * opening;
        piece.position.z += opening * 2;
        piece.rotation.z = (i === 0 ? 1 : -1) * opening * 0.9;
      } else {
        piece.position.y += 5.5 * Math.sin(opening * Math.PI) - 6 * opening;
        piece.position.z -= 8 * opening;
        piece.rotation.x = -1.1 * opening;
      }
    });
    for (const rig of hatchRigs) {
      poseRig(rig, 0, 'idle');
      rig.outer.visible = rig.def.kind === kind;
      const size = Math.min(0.65, 6 / (rig.def.max[2] - rig.def.min[2]), 8 / rig.def.height),
        settle = Math.sin(Math.PI * smooth(0.82, 1, t));
      rig.outer.scale.setScalar(size + (1 - size) * birth);
      rig.outer.position.set(
        0,
        -7 + 2 * (1 - birth) + (reduced ? 0 : Math.sin(birth * Math.PI) * 1.1),
        0
      );
      rig.outer.rotation.set(0, 0, 0);
      rig.root.position.set(-rig.def.center[0], 0, -rig.def.center[2]);
      rig.root.scale.y = 1 - 0.12 * (1 - birth) - 0.05 * settle;
      rig.nodes.get('head').group.rotation.x = 0.2 * (1 - birth) * smooth(0.3, 0.5, t);
    }
  }
  const scene = new THREE.Scene();
  const ambient = new THREE.AmbientLight(0xffffff, 1.65);
  scene.add(ambient);
  const light = new THREE.DirectionalLight(0xfff5e7, 2.5);
  light.position.set(-30, 55, 60);
  scene.add(light);
  const fill = new THREE.DirectionalLight(0xd9ecff, 0.6);
  fill.position.set(30, 10, -35);
  scene.add(fill);
  for (const r of rigs) scene.add(r.outer);
  for (const e of eggGroups) scene.add(e);
  scene.add(hatchGroup, growthGroup);

  const rgbKey = (r, g, b) => [r, g, b].map((v) => Math.round(v * 4096)).join(',');
  function tintMap(item) {
    const id = item.kind + ':' + item.color;
    if (tintMaps.has(id)) return tintMaps.get(id);
    const map = new Map(),
      tint = new THREE.Color(tints[item.color].hex),
      cream = new THREE.Color(P.shell);
    const set = (hex, color) => {
      const c = new THREE.Color(hex);
      map.set(rgbKey(c.r, c.g, c.b), color);
    };
    set(P.shell, tint.clone().lerp(cream, 0.48));
    set(P.speckle, tint.clone().multiplyScalar(0.8));
    set(P.crack, tint.clone().multiplyScalar(0.35));
    if (item.kind === 'mecha') set(P.blue, tint.clone());
    if (item.kind === 'unicorn') set(P.white, tint.clone().lerp(new THREE.Color(P.white), 0.7));
    if (item.kind === 'kaiju') {
      set(P.beast, tint.clone().multiplyScalar(0.7));
      set(P.beastDark, tint.clone().multiplyScalar(0.42));
      set(P.spine, tint.clone().lerp(cream, 0.35));
    }
    if (item.kind === 'fox') {
      set(P.fox, tint.clone());
      set(P.foxDark, tint.clone().multiplyScalar(0.5));
    }
    if (item.kind === 'alien') {
      set(P.green, tint.clone());
      set(P.greenDark, tint.clone().multiplyScalar(0.55));
    }
    if (item.kind === 'kangaroo') {
      set(P.roo, tint.clone().multiplyScalar(0.85));
      set(P.rooDark, tint.clone().multiplyScalar(0.5));
    }
    tintMaps.set(id, map);
    return map;
  }
  function paletteColor(hex, item = current) {
    const c = new THREE.Color(hex);
    return (tintMap(item).get(rgbKey(c.r, c.g, c.b)) || c).clone();
  }
  function paintPalette(item) {
    const id = item.kind + ':' + item.color;
    if (id === lastTint) return;
    lastTint = id;
    const map = tintMap(item);
    coreMaterial.color.set(tints[item.color].hex).offsetHSL(0, 0.45, 0.1);
    for (const geometry of geometries) {
      const attribute = geometry.attributes.color;
      if (!attribute) continue;
      if (!originalColors.has(geometry))
        originalColors.set(geometry, new Float32Array(attribute.array));
      const base = originalColors.get(geometry);
      attribute.array.set(base);
      for (let i = 0; i < base.length; i += 3) {
        const color = map.get(rgbKey(base[i], base[i + 1], base[i + 2]));
        if (color) {
          attribute.array[i] = color.r;
          attribute.array[i + 1] = color.g;
          attribute.array[i + 2] = color.b;
        }
      }
      attribute.needsUpdate = true;
    }
    for (const value of growthMeshes.values())
      value.colors = value.def.tracks.map((v) => [
        paletteColor(v.ca, item),
        paletteColor(v.cb, item),
      ]);
  }

  const renderer = new THREE.WebGLRenderer({
    canvas,
    alpha: true,
    antialias: true,
    preserveDrawingBuffer: true,
    powerPreference: 'low-power',
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.75));
  renderer.setClearColor(0, 0);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  let disposed = false,
    raf = 0,
    progress = 0,
    mode = 'still',
    angle = 28,
    tilt = 0;
  const point = new THREE.Vector3();
  const thumbnailCache = new Map();

  function hideObjects() {
    for (const rig of rigs) rig.outer.visible = false;
    for (const group of eggGroups) group.visible = false;
    hatchGroup.visible = false;
    growthGroup.visible = false;
  }
  function shellFrame(t) {
    const u = smooth(0.34, 0.78, t);
    return new THREE.Box3(
      new THREE.Vector3(-6 - 12 * u, -7 - u, -6 - 9 * u),
      new THREE.Vector3(6 + 12 * u, 7 + 2 * u, 6 + 4 * u)
    );
  }
  function draw(width, height, pet = current, miniature = false) {
    if (disposed || width < 1 || height < 1) return;
    paintPalette(pet);
    // A bright surrounding interface needs softer, fuller illumination. Keep
    // thumbnails in their fixed studio light so cached collection icons stay stable.
    const daylight = !miniature && canvas.ownerDocument?.documentElement?.dataset.theme === 'light';
    ambient.intensity = daylight ? 2.8 : 1.65;
    light.intensity = daylight ? 2.7 : 2.5;
    fill.intensity = daylight ? 1.25 : 0.6;
    hideObjects();
    let object, frame;
    if (!miniature && (mode === 'hatch' || mode === 'crack')) {
      const t = mode === 'crack' ? progress * crackedEggProgress : progress;
      poseHatch(t, pet.kind);
      if (mode === 'crack') {
        wobble.rotation.z *= Math.sin(Math.PI * progress);
        for (const rig of hatchRigs) rig.outer.visible = false;
      }
      object = hatchGroup;
      frame = shellFrame(t);
      if (mode === 'hatch') {
        const settle = smooth(0.84, 1, t),
          rig = hatchRigs.find((r) => r.def.kind === pet.kind),
          def = rig.def;
        rig.outer.position.y += (7 - def.height / 2) * settle;
        for (const piece of pieces) piece.scale.setScalar(1 - settle);
        frame.min.lerp(
          new THREE.Vector3(
            def.min[0] - def.center[0],
            -def.height / 2,
            def.min[2] - def.center[2]
          ),
          settle
        );
        frame.max.lerp(
          new THREE.Vector3(def.max[0] - def.center[0], def.height / 2, def.max[2] - def.center[2]),
          settle
        );
      }
    } else if (!miniature && mode === 'grow') {
      poseGrowth(progress, pet.kind);
      object = growthGroup;
      const phase = growthPhase(progress),
        from = definitions.find((d) => d.kind === pet.kind && d.age === phase.step),
        to = definitions.find((d) => d.kind === pet.kind && d.age === phase.step + 1);
      const height = from.height + (to.height - from.height) * phase.amount;
      const bound = (side, axis) =>
        (from[side][axis] - from.center[axis]) * (1 - phase.amount) +
        (to[side][axis] - to.center[axis]) * phase.amount;
      frame = new THREE.Box3(
        new THREE.Vector3(bound('min', 0), -height / 2, bound('min', 2)),
        new THREE.Vector3(bound('max', 0), height / 2, bound('max', 2))
      );
      frame.translate(new THREE.Vector3(0, -growthGroup.position.y, 0));
    } else if (pet.stage === 1) {
      poseHatch(crackedEggProgress, pet.kind);
      for (const rig of hatchRigs) rig.outer.visible = false;
      wobble.rotation.z =
        !miniature && !reduced && mode === 'interact'
          ? Math.sin(progress * Math.PI * 6) * 0.05 * Math.sin(progress * Math.PI)
          : 0;
      wobble.scale.setScalar(
        !miniature && mode === 'level' ? 1 + 0.045 * Math.sin(progress * Math.PI) : 1
      );
      object = hatchGroup;
      frame = shellFrame(crackedEggProgress);
    } else if (pet.stage === 0) {
      object = eggGroups[0];
      object.children[0].scale.setScalar(
        !miniature && mode === 'level' ? 1 + 0.045 * Math.sin(progress * Math.PI) : 1
      );
      object.children[0].rotation.z =
        !miniature && !reduced && mode === 'interact'
          ? Math.sin(progress * Math.PI * 6) * 0.05 * Math.sin(progress * Math.PI)
          : 0;
      frame = new THREE.Box3(new THREE.Vector3(-6, -7, -6), new THREE.Vector3(6, 7, 6));
    } else {
      const rig = rigs.find((r) => r.def.kind === pet.kind && r.def.age === pet.stage - 2);
      poseRig(rig, !miniature && mode === 'interact' ? progress : 0, 'special');
      rig.root.position.set(
        -rig.def.center[0],
        rig.root.position.y - rig.def.height / 2 - rig.def.min[1],
        -rig.def.center[2]
      );
      if (!miniature && mode === 'level') {
        const pulse = Math.sin(progress * Math.PI);
        rig.root.position.y += pulse * 0.6;
        rig.root.scale.multiplyScalar(1 + 0.04 * pulse);
      }
      object = rig.outer;
      const ref = rig.def;
      frame = new THREE.Box3(
        new THREE.Vector3(ref.min[0] - ref.center[0], -ref.height / 2, ref.min[2] - ref.center[2]),
        new THREE.Vector3(ref.max[0] - ref.center[0], ref.height / 2, ref.max[2] - ref.center[2])
      );
    }
    object.visible = true;
    object.rotation.set(
      miniature ? 0 : tilt,
      miniature ? 0.45 : THREE.MathUtils.degToRad(angle),
      0
    );
    object.updateMatrixWorld(true);
    frame.applyMatrix4(object.matrixWorld);
    const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 250);
    camera.position.set(0, 12, 80);
    camera.lookAt(0, 0, 0);
    camera.updateMatrixWorld(true);
    let half = 1;
    for (const x of [frame.min.x, frame.max.x])
      for (const y of [frame.min.y, frame.max.y])
        for (const z of [frame.min.z, frame.max.z]) {
          point.set(x, y, z).applyMatrix4(camera.matrixWorldInverse);
          half = Math.max(half, Math.abs(point.y), Math.abs(point.x) / (width / height));
        }
    half *= miniature ? 1.15 : 1.12;
    camera.left = (-half * width) / height;
    camera.right = (half * width) / height;
    camera.top = half;
    camera.bottom = -half;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height, false);
    renderer.render(scene, camera);
  }
  function render() {
    draw(canvas.clientWidth, canvas.clientHeight);
  }
  function stop() {
    cancelAnimationFrame(raf);
    raf = 0;
    mode = 'still';
    progress = 0;
  }
  function animate(nextMode, from, to, duration, done = () => {}, onProgress) {
    stop();
    mode = nextMode;
    progress = from;
    onProgress?.(0);
    if (reduced) {
      progress = to;
      onProgress?.(1);
      render();
      stop();
      done();
      render();
      return;
    }
    const start = performance.now();
    const tick = (now) => {
      if (disposed) return;
      progress = Math.min(to, from + (now - start) / duration);
      onProgress?.((progress - from) / (to - from));
      render();
      if (progress < to) raf = requestAnimationFrame(tick);
      else {
        stop();
        done();
        render();
      }
    };
    render();
    raf = requestAnimationFrame(tick);
  }
  const observer = new ResizeObserver(render);
  observer.observe(canvas);
  const root = canvas.ownerDocument?.documentElement;
  const appearanceObserver = root ? new MutationObserver(render) : null;
  appearanceObserver?.observe(root, { attributes: true, attributeFilter: ['data-theme'] });
  return {
    show(pet) {
      stop();
      current = { ...pet };
      render();
    },
    upgrade(next, done, onProgress) {
      if (next.stage === current.stage) {
        animate(
          'level',
          0,
          1,
          600,
          () => {
            current = { ...next };
            done();
          },
          onProgress
        );
        return;
      }
      const stage = current.stage;
      animate(
        stage === 0 ? 'crack' : stage === 1 ? 'hatch' : 'grow',
        stage === 1 ? crackedEggProgress : stage === 3 ? 0.5 : 0,
        stage === 2 ? 0.5 : 1,
        stage === 0 ? 1400 : 2600,
        () => {
          current = { ...next };
          done();
        },
        onProgress
      );
    },
    interact() {
      if (mode !== 'still') return;
      animate('interact', 0, 1, 1400);
    },
    rotate(dx, dy) {
      angle = ((angle + dx * 0.6 + 540) % 360) - 180;
      tilt = Math.max(-0.65, Math.min(0.65, tilt + dy * 0.007));
      render();
    },
    thumbnail(pet) {
      const id = [pet.kind, pet.stage, pet.color].join(':');
      if (!thumbnailCache.has(id)) {
        draw(96, 96, pet, true);
        thumbnailCache.set(id, canvas.toDataURL('image/png'));
        render();
      }
      return thumbnailCache.get(id);
    },
    dispose() {
      if (disposed) return;
      stop();
      disposed = true;
      observer.disconnect();
      appearanceObserver?.disconnect();
      for (const geometry of geometries) geometry.dispose();
      for (const { mesh } of growthMeshes.values()) mesh.dispose();
      growthMaterial.dispose();
      coreMaterial.dispose();
      material.dispose();
      renderer.dispose();
      thumbnailCache.clear();
      scene.clear();
    },
  };
}
