const VERTEX = `#version 300 es
in vec2 a_position;
uniform vec2 u_resolution;
out vec2 pixel;
void main() {
  pixel = (a_position * 0.5 + 0.5) * u_resolution;
  gl_Position = vec4(a_position, 0.0, 1.0);
}`;

const FRAGMENT = `#version 300 es
precision highp float;
in vec2 pixel;
out vec4 result;
uniform vec2 u_resolution;
uniform float u_time, u_seed, u_fade, u_pos;
uniform vec3 u_fg, u_fg2;
uniform vec4 u_charge0, u_charge1, u_tintA, u_tintB;
uniform float u_bedFill, u_chargeMax, u_chargeRamp, u_inkFloor, u_inkCeil, u_energyGain;
uniform float u_burstTime[8], u_burstGain[8];
uniform vec2 u_burstCenter[8];

float randomCell(vec2 position) {
  return fract(sin(dot(position, vec2(127.1, 311.7))) * 43758.5453);
}

vec2 pulseField(vec2 cell, float pitch) {
  float jitter = (randomCell(cell + 41.7 + u_seed) - 0.5) * 2.0 * 0.1;
  float variance = 0.35 + 0.65 * randomCell(cell + 13.7 + u_seed);
  vec2 field = vec2(0.0);
  for (int index = 0; index < 8; index++) {
    float age = u_time - u_burstTime[index];
    if (age < 0.0 || age > 5.0) continue;
    float strength = clamp(u_burstGain[index], 0.0, 1.0);
    vec2 distance = abs(cell - floor(u_burstCenter[index] / pitch));
    float travel = distance.x + distance.y;
    float radius = mix(10.0, 55.0, strength);
    float falloff = exp(-travel / (radius * 0.85));
    float reach = exp(-max(travel - radius, 0.0) * 0.12);
    float arrival = age - travel / mix(20.0, 36.0, strength) - jitter;
    field.y = max(field.y, smoothstep(0.0, 0.15, arrival) * reach);
    if (arrival < 0.0) continue;
    float recruitment = randomCell(cell + 7.3 + u_seed + u_burstTime[index]);
    if (recruitment > mix(0.3, 0.85, strength)) continue;
    float front = 1.25 * exp(-arrival * 5.0);
    float tail = 0.65 * exp(-arrival * mix(3.3, 0.77, strength));
    field.x += (front + tail) * mix(0.55, 1.0, strength) * falloff * reach * variance;
  }
  return field;
}

void main() {
  float pitch = u_resolution.y / max(round(u_resolution.y / 4.0), 1.0);
  float scale = pitch / 4.0;
  vec2 cell = floor(pixel / pitch);
  vec2 local = abs(pixel - (cell + 0.5) * pitch) - vec2((1.5 - 0.9) * scale);
  float distance = length(max(local, vec2(0.0))) + min(max(local.x, local.y), 0.0) - 0.9 * scale;
  float antialias = min(fwidth(pixel.x), 1.5);
  float coverage = 1.0 - smoothstep(-antialias, antialias, distance);
  if (coverage <= 0.0 && u_bedFill <= 0.0) {
    result = vec4(0.0);
    return;
  }
  vec2 field = pulseField(cell, pitch);
  float lotteryA = randomCell(cell + 8.8 + u_seed);
  float lotteryB = randomCell(cell + 88.8 + u_seed);
  float drift = 0.5 + 0.5 * sin(u_time * 0.35 + lotteryA * 6.2832);
  float lottery = pow(mix(lotteryA, lotteryB, drift), 1.9);
  float stream = 0.9 + 0.16 * sin(u_time * 1.3 + cell.x * 0.45 + lotteryB * 2.0);
  float energy = clamp(field.x, 0.0, 1.0) * u_fade * mix(0.16, 1.0, lottery) * stream * u_energyGain;
  float level = step(0.04, energy) + step(0.2, energy) + step(0.4, energy)
              + step(0.6, energy) + step(0.8, energy);
  float inkAlpha = level == 0.0 ? 0.0 : mix(u_inkFloor, u_inkCeil, (level - 1.0) / 4.0);
  if (level >= 5.0) {
    float period = 2.4 + 1.4 * randomCell(cell + 3.1 + u_seed);
    float phase = randomCell(cell + 5.5 + u_seed) * 6.2832;
    inkAlpha *= 1.0 - 0.28 * 0.5 * (1.0 + sin(u_time * 6.2832 / period + phase));
  }
  vec3 ink = mix(u_fg2, u_fg, pow(level / 5.0, 1.4));
  float hue = randomCell(cell + 27.9 + u_seed);
  ink = mix(ink, u_tintA.rgb, u_tintA.a * smoothstep(0.62, 0.95, hue));
  ink = mix(ink, u_tintB.rgb, u_tintB.a * (1.0 - smoothstep(0.05, 0.38, hue)));
  float position = clamp(pixel.x / max(u_pos * u_resolution.x, 1.0), 0.0, 1.0);
  vec3 base = mix(u_charge0.rgb, u_charge1.rgb, position);
  float baseAlpha = u_chargeMax * pow(position, u_chargeRamp) * field.y * u_fade
                  * mix(u_bedFill, 1.0, coverage) * mix(u_charge0.a, u_charge1.a, position);
  float lightAlpha = inkAlpha * coverage;
  result = vec4(ink * lightAlpha + base * baseAlpha * (1.0 - lightAlpha),
                lightAlpha + baseAlpha * (1.0 - lightAlpha));
}`;

export interface ReasoningEnergyRenderer {
  setActive: (active: boolean) => void;
  dispose: () => void;
}

export function createReasoningEnergyRenderer(
  canvas: HTMLCanvasElement
): ReasoningEnergyRenderer | null {
  const gl = canvas.getContext('webgl2', {
    alpha: true,
    antialias: false,
    depth: false,
    stencil: false,
  });
  if (!gl) return null;
  const compile = (type: number, source: string) => {
    const shader = gl.createShader(type);
    if (!shader) return null;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return shader;
    gl.deleteShader(shader);
    return null;
  };
  const vertex = compile(gl.VERTEX_SHADER, VERTEX);
  const fragment = compile(gl.FRAGMENT_SHADER, FRAGMENT);
  const program = gl.createProgram();
  if (vertex && fragment && program) {
    gl.attachShader(program, vertex);
    gl.attachShader(program, fragment);
    gl.linkProgram(program);
  }
  if (vertex) gl.deleteShader(vertex);
  if (fragment) gl.deleteShader(fragment);
  if (!vertex || !fragment || !program || !gl.getProgramParameter(program, gl.LINK_STATUS)) {
    if (program) gl.deleteProgram(program);
    return null;
  }
  const buffer = gl.createBuffer();
  if (!buffer) {
    gl.deleteProgram(program);
    return null;
  }
  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(
    gl.ARRAY_BUFFER,
    new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
    gl.STATIC_DRAW
  );
  const attribute = gl.getAttribLocation(program, 'a_position');
  gl.enableVertexAttribArray(attribute);
  gl.vertexAttribPointer(attribute, 2, gl.FLOAT, false, 0, 0);
  const uniform = Object.fromEntries(
    [
      'resolution',
      'time',
      'seed',
      'fade',
      'pos',
      'fg',
      'fg2',
      'charge0',
      'charge1',
      'tintA',
      'tintB',
      'bedFill',
      'chargeMax',
      'chargeRamp',
      'inkFloor',
      'inkCeil',
      'energyGain',
      'burstTime[0]',
      'burstCenter[0]',
      'burstGain[0]',
    ].map((name) => [name, gl.getUniformLocation(program, `u_${name}`)])
  );
  const times = new Float32Array(8).fill(-1000);
  const centers = new Float32Array(16);
  const gains = new Float32Array(8);
  let slot = 0;
  let width = 1;
  let height = 1;
  let clock = 0;
  let previous = -1;
  let charge = 0;
  let held = false;
  let holdTime = 0;
  let lastPulse = -Infinity;
  let lastPulseWall = -Infinity;
  let repeatDelay = 0.45;
  let frame = 0;
  let running = false;
  let disposed = false;

  const uploadPulses = () => {
    gl.uniform1fv(uniform['burstTime[0]'], times);
    gl.uniform2fv(uniform['burstCenter[0]'], centers);
    gl.uniform1fv(uniform['burstGain[0]'], gains);
  };
  const draw = () => {
    gl.useProgram(program);
    gl.uniform1f(uniform.time, clock);
    gl.uniform1f(uniform.fade, charge * charge * (3 - 2 * charge));
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
  };
  const emit = (strength: number, position: number) => {
    times[slot] = clock;
    centers[slot * 2] = position * width;
    centers[slot * 2 + 1] = height * (0.35 + Math.random() * 0.3);
    gains[slot] = strength;
    slot = (slot + 1) % 8;
    lastPulse = clock;
    lastPulseWall = performance.now();
    uploadPulses();
  };
  const tick = (now: number) => {
    if (disposed) return;
    const delta = previous < 0 ? 0 : Math.max(0, (now - previous) * 0.001);
    previous = now;
    clock += delta;
    charge = held ? Math.min(1, charge + delta / 0.7) : Math.max(0, charge - delta / 0.85);
    holdTime = held ? holdTime + delta : 0;
    if (held && clock - lastPulse > repeatDelay) {
      repeatDelay = 0.3 + Math.random() * 0.45;
      const strength = 0.85 * (0.45 + 0.55 * Math.min(holdTime / 1.6, 1));
      emit(strength * (0.85 + Math.random() * 0.3), 1 + (Math.random() - 0.5) * 0.08);
    }
    draw();
    if (held || (clock - lastPulse < 5 && charge > 0)) frame = requestAnimationFrame(tick);
    else running = false;
  };
  const start = () => {
    if (running) return;
    running = true;
    previous = -1;
    frame = requestAnimationFrame(tick);
  };
  const resize = (entry?: ResizeObserverEntry) => {
    if (disposed) return;
    const ratio = window.devicePixelRatio || 1;
    const pixels = entry?.devicePixelContentBoxSize?.[0];
    const box = entry?.borderBoxSize?.[0] ?? entry?.contentBoxSize?.[0];
    const nextWidth = Math.max(
      1,
      pixels ? pixels.inlineSize / ratio : (box?.inlineSize ?? canvas.clientWidth)
    );
    const nextHeight = Math.max(
      1,
      pixels ? pixels.blockSize / ratio : (box?.blockSize ?? canvas.clientHeight)
    );
    for (let index = 0; index < 8; index++) {
      centers[index * 2] *= nextWidth / width;
      centers[index * 2 + 1] *= nextHeight / height;
    }
    width = nextWidth;
    height = nextHeight;
    canvas.width = Math.max(1, Math.round(width * ratio));
    canvas.height = Math.max(1, Math.round(height * ratio));
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(uniform.resolution, width, height);
    uploadPulses();
    draw();
  };
  const palette = () => {
    if (disposed) return;
    const accent = (getComputedStyle(canvas).color.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number);
    if (accent.length !== 3) return;
    // The animation sits on a dark tip surface in both modes. Keep Coffee
    // CLI's night palette; the surrounding tip alone becomes lighter by day.
    const tint = (channels: number[]) => {
      const peak = Math.max(...channels.slice(0, 3));
      return channels.map((channel, index) =>
        index < 3 ? (peak * accent[index]) / (255 * 255) : channel / 255
      );
    };
    gl.useProgram(program);
    gl.uniform3fv(uniform.fg, tint([160, 150, 235]));
    gl.uniform3fv(uniform.fg2, tint([255, 255, 255]));
    gl.uniform4fv(uniform.charge0, tint([158, 128, 235, 255]));
    gl.uniform4fv(uniform.charge1, tint([89, 64, 153, 255]));
    gl.uniform4fv(uniform.tintA, [0, 0, 0, 0]);
    gl.uniform4fv(uniform.tintB, [0, 0, 0, 0]);
    gl.uniform1f(uniform.bedFill, 0);
    gl.uniform1f(uniform.chargeMax, 0.85);
    gl.uniform1f(uniform.chargeRamp, 1.1);
    gl.uniform1f(uniform.inkFloor, 0.08);
    gl.uniform1f(uniform.inkCeil, 0.85);
    gl.uniform1f(uniform.energyGain, 1);
    draw();
  };
  gl.clearColor(0, 0, 0, 0);
  gl.uniform1f(uniform.pos, 1);
  gl.uniform1f(uniform.seed, 0);
  resize();
  palette();
  const observer = new ResizeObserver((entries) => resize(entries[0]));
  observer.observe(canvas);
  const themeObserver = new MutationObserver(palette);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'style'],
  });

  return {
    setActive: (active) => {
      if (disposed || active === held) return;
      held = active;
      if (active) {
        palette();
        if (performance.now() - lastPulseWall > 5000 && charge <= 0) {
          times.fill(-1000);
          gl.uniform1f(uniform.seed, Math.random() * 512);
        }
        holdTime = 0;
        emit(0.85 * 0.45, 1);
      }
      start();
    },
    dispose: () => {
      if (disposed) return;
      disposed = true;
      cancelAnimationFrame(frame);
      observer.disconnect();
      themeObserver.disconnect();
      gl.deleteBuffer(buffer);
      gl.deleteProgram(program);
      gl.getExtension('WEBGL_lose_context')?.loseContext();
    },
  };
}
