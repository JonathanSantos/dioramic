// Spike descartável: valida "cena → SVG → resvg → PNG/GIF" e o visual isométrico.
// O motor de raster é o resvg (Rust) via binding Node; a geometria abaixo porta 1:1 para Rust.
import { Resvg } from '@resvg/resvg-js';
import gifenc from 'gifenc';
const { GIFEncoder, quantize, applyPalette } = gifenc;
import { writeFileSync, mkdirSync } from 'node:fs';
import { performance } from 'node:perf_hooks';

const OUT = process.argv[2] ?? './out';
mkdirSync(OUT, { recursive: true });

// ---------- projeção isométrica verdadeira (30°) ----------
const T = 56;                     // px por tile
const C = Math.cos(Math.PI / 6);  // 0.866
const S = Math.sin(Math.PI / 6);  // 0.5
const OX = 400, OY = 190;
const PR = (u, v, z = 0) => [OX + (u - v) * C * T, OY + (u + v) * S * T - z * T];
// bounding box do conteúdo → viewBox automático (enquadramento)
let BB;
const resetBB = () => (BB = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity });
const track = (x, y) => { BB.x0 = Math.min(BB.x0, x); BB.y0 = Math.min(BB.y0, y); BB.x1 = Math.max(BB.x1, x); BB.y1 = Math.max(BB.y1, y); };
const P = (u, v, z = 0) => { const p = PR(u, v, z); track(p[0], p[1]); return p; };
const f1 = (n) => +n.toFixed(1);
const pts = (a) => a.map(([x, y]) => `${f1(x)},${f1(y)}`).join(' ');
const poly = (c, attrs) => `<polygon points="${pts(c)}" ${attrs}/>`;

const shade = (hex, k) => {
  const n = parseInt(hex.slice(1), 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => Math.max(0, Math.min(255, Math.round(c * k))));
  return '#' + ch.map((c) => c.toString(16).padStart(2, '0')).join('');
};

const theme = {
  bg: '#f2f4f8', grid: '#dde2ec', ink: '#1c2130', stroke: '#252b3b',
  zone: { violet: ['#ded5f6', '#7a5bd3'], blue: ['#cdd6f6', '#5468d4'], orange: ['#fbe2c8', '#d5843a'], teal: ['#c9e9e4', '#2c9a8c'] },
  edge: { primary: '#d27a2c', secondary: '#3e50c8' },
};

// ---------- cena (o que a IA descreveria no spec) ----------
const zones = [
  { label: 'INTERNET', color: 'violet', u0: 0, v0: 0.5, u1: 3, v1: 3.5 },
  { label: 'EDGE', color: 'blue', u0: 4, v0: 0.5, u1: 7, v1: 3.5 },
  { label: 'APP TIER', color: 'orange', u0: 8, v0: -0.5, u1: 11, v1: 4.5 },
  { label: 'DATA', color: 'teal', u0: 12, v0: -0.5, u1: 15, v1: 4.5 },
];
const nodes = [
  { id: 'users', kind: 'cloud', u: 1.5, v: 2, label: 'Users' },
  { id: 'lb', kind: 'lb', u: 5.5, v: 2, label: 'Load Balancer' },
  { id: 'api1', kind: 'server', u: 9.5, v: 0.8, label: 'API #1' },
  { id: 'api2', kind: 'server', u: 9.5, v: 3.2, label: 'API #2' },
  { id: 'db', kind: 'db', u: 13.5, v: 0.8, label: 'Postgres' },
  { id: 'cache', kind: 'cache', u: 13.5, v: 3.2, label: 'Redis' },
];
const edges = [
  { from: 'users', to: 'lb', label: 'HTTPS', style: 'primary' },
  { from: 'lb', to: 'api1', style: 'primary' },
  { from: 'lb', to: 'api2', label: 'round-robin', style: 'primary' },
  { from: 'api1', to: 'db', label: 'SQL', style: 'secondary' },
  { from: 'api2', to: 'db', style: 'secondary' },
  { from: 'api1', to: 'cache', style: 'secondary' },
  { from: 'api2', to: 'cache', label: 'GET / SET', style: 'secondary' },
];
const byId = Object.fromEntries(nodes.map((n) => [n.id, n]));
const HEIGHT = { cloud: 0.9, lb: 0.28, server: 1.0, db: 0.95, cache: 0.6 };

// ---------- primitivas ----------
function box(u, v, w, d, h, color, z0 = 0) {
  const top = [P(u, v, z0 + h), P(u + w, v, z0 + h), P(u + w, v + d, z0 + h), P(u, v + d, z0 + h)];
  const left = [P(u, v + d, z0), P(u + w, v + d, z0), P(u + w, v + d, z0 + h), P(u, v + d, z0 + h)];
  const right = [P(u + w, v, z0), P(u + w, v + d, z0), P(u + w, v + d, z0 + h), P(u + w, v, z0 + h)];
  const sa = `stroke="${theme.stroke}" stroke-width="1.1" stroke-linejoin="round"`;
  return poly(left, `fill="${shade(color, 0.8)}" ${sa}`) + poly(right, `fill="${shade(color, 0.62)}" ${sa}`) + poly(top, `fill="${color}" ${sa}`);
}

const gradients = new Map();
function cylGrad(color) {
  const id = `cg${color.slice(1)}`;
  if (!gradients.has(id))
    gradients.set(id, `<linearGradient id="${id}" x1="0" x2="1" y1="0" y2="0"><stop offset="0" stop-color="${shade(color, 0.95)}"/><stop offset="0.55" stop-color="${shade(color, 0.78)}"/><stop offset="1" stop-color="${shade(color, 0.55)}"/></linearGradient>`);
  return id;
}
function cylinder(cu, cv, r, h, color, z0 = 0) {
  const [cx, yb] = P(cu, cv, z0);
  const [, yt] = P(cu, cv, z0 + h);
  const rx = Math.SQRT2 * C * r * T, ry = Math.SQRT2 * S * r * T;
  const sa = `stroke="${theme.stroke}" stroke-width="1.1"`;
  const side = `M${f1(cx - rx)},${f1(yt)} L${f1(cx - rx)},${f1(yb)} A${f1(rx)},${f1(ry)} 0 0 0 ${f1(cx + rx)},${f1(yb)} L${f1(cx + rx)},${f1(yt)} Z`;
  return `<path d="${side}" fill="url(#${cylGrad(color)})" ${sa}/><ellipse cx="${f1(cx)}" cy="${f1(yt)}" rx="${f1(rx)}" ry="${f1(ry)}" fill="${shade(color, 1.12)}" ${sa}/>`;
}

function shadow(u, v, r = 0.55) {
  const [x, y] = P(u + 0.12, v - 0.05, 0);
  return `<ellipse cx="${f1(x)}" cy="${f1(y)}" rx="${f1(Math.SQRT2 * C * r * T)}" ry="${f1(Math.SQRT2 * S * r * T)}" fill="#1b2440" opacity="0.22" filter="url(#blur)"/>`;
}

function drawNode(n) {
  const { u, v } = n;
  let s = shadow(u, v);
  switch (n.kind) {
    case 'server':
      for (let i = 0; i < 3; i++) {
        const z = i * 0.34;
        s += box(u - 0.42, v - 0.42, 0.84, 0.84, 0.3, '#56607a', z);
        const [a, b] = [P(u - 0.3, v + 0.42, z + 0.15), P(u + 0.05, v + 0.42, z + 0.15)];
        s += `<line x1="${f1(a[0])}" y1="${f1(a[1])}" x2="${f1(b[0])}" y2="${f1(b[1])}" stroke="#7ef0a8" stroke-width="2.2" stroke-linecap="round"/>`;
      }
      return s;
    case 'lb':
      s += box(u - 0.55, v - 0.4, 1.1, 0.8, 0.28, '#3d4fd1');
      for (let i = 0; i < 5; i++) {
        const [x, y] = P(u - 0.42 + i * 0.2, v + 0.4, 0.14);
        s += `<circle cx="${f1(x)}" cy="${f1(y)}" r="2.2" fill="#b9c3ff"/>`;
      }
      return s;
    case 'db':
      for (let i = 0; i < 3; i++) s += cylinder(u, v, 0.48, 0.28, '#a3acc0', i * 0.33);
      return s;
    case 'cache':
      s += cylinder(u, v, 0.48, 0.3, '#e0534f', 0) + cylinder(u, v, 0.48, 0.24, '#ef6b5f', 0.36);
      return s;
    case 'cloud': {
      const [x, y] = P(u, v, 0.45);
      const puffs = [[-22, 6, 17], [0, -6, 23], [22, 4, 18], [-6, 12, 16], [12, 13, 15]];
      for (const [dx, dy, r] of puffs) s += `<circle cx="${x + dx}" cy="${y + dy}" r="${r}" fill="none" stroke="${theme.stroke}" stroke-width="2.2"/>`;
      for (const [dx, dy, r] of puffs) s += `<circle cx="${x + dx}" cy="${y + dy}" r="${r}" fill="url(#cloud)"/>`;
      return s;
    }
  }
}

// ---------- labels ----------
const textW = (str, size, bold) => str.length * size * (bold ? 0.6 : 0.56);
function nodeLabel(n) {
  const h = HEIGHT[n.kind];
  const [x, y0] = P(n.u, n.v, h);
  const y = y0 - 52;
  const w = textW(n.label, 14) + 22;
  track(x - w / 2, y - 28); track(x + w / 2, y);
  return `<line x1="${f1(x)}" y1="${f1(y0)}" x2="${f1(x)}" y2="${f1(y)}" stroke="${theme.stroke}" stroke-width="1.2"/>` +
    `<rect x="${f1(x - w / 2)}" y="${f1(y - 28)}" width="${f1(w)}" height="28" rx="6" fill="#fff" stroke="#c8cfdc" stroke-width="1.2"/>` +
    `<text x="${f1(x)}" y="${f1(y - 9.5)}" font-size="14" text-anchor="middle" fill="${theme.ink}">${n.label}</text>`;
}
function pill(x, y, label) {
  const w = textW(label, 11, true) + 14;
  return `<rect x="${f1(x - w / 2)}" y="${f1(y - 10)}" width="${f1(w)}" height="20" rx="5" fill="#1f2434"/>` +
    `<text x="${f1(x)}" y="${f1(y + 4)}" font-size="11" font-weight="700" text-anchor="middle" fill="#fff">${label}</text>`;
}
function zoneLabel(z) {
  // texto deitado no plano do chão: eixo x do texto → eixo u, eixo y → eixo v
  const [x, y] = P(z.u0 + 0.3, z.v1 - 0.5, 0);
  const w = textW(z.label, 12, true) + 16;
  return `<g transform="matrix(${C},${S},${-C},${S},${f1(x)},${f1(y)})"><rect x="0" y="0" width="${f1(w)}" height="20" rx="4" fill="#1f2434"/>` +
    `<text x="8" y="14.5" font-size="12" font-weight="700" letter-spacing="0.6" fill="#fff">${z.label}</text></g>`;
}

// ---------- arestas ----------
const bez = (p, t) => {
  const m = 1 - t;
  return [0, 1].map((i) => m * m * m * p[0][i] + 3 * m * m * t * p[1][i] + 3 * m * t * t * p[2][i] + t * t * t * p[3][i]);
};
function edgeGeom(e) {
  const a = byId[e.from], b = byId[e.to];
  const du = b.u - a.u, dv = b.v - a.v, len = Math.hypot(du, dv), r = 0.72;
  const p0 = P(a.u + (du / len) * r, a.v + (dv / len) * r, 0.08);
  const p3 = P(b.u - (du / len) * r, b.v - (dv / len) * r, 0.08);
  // controles ao longo do eixo u (fluxo principal) → curva suave estilo referência
  const k = 0.45 * Math.abs(du) * T;
  const p1 = [p0[0] + k * C, p0[1] + k * S], p2 = [p3[0] - k * C, p3[1] - k * S];
  return [p0, p1, p2, p3];
}

// ---------- montagem do SVG para um instante t∈[0,1) ----------
function scene(t, animated) {
  resetBB();
  let g = '';
  for (let k = -30; k <= 45; k++) {
    const [a, b] = [PR(k, -30), PR(k, 45)], [c, d] = [PR(-30, k), PR(45, k)];
    g += `<line x1="${f1(a[0])}" y1="${f1(a[1])}" x2="${f1(b[0])}" y2="${f1(b[1])}"/><line x1="${f1(c[0])}" y1="${f1(c[1])}" x2="${f1(d[0])}" y2="${f1(d[1])}"/>`;
  }
  let zs = '';
  for (const z of zones) {
    const [fill, stroke] = theme.zone[z.color];
    zs += poly([P(z.u0, z.v0), P(z.u1, z.v0), P(z.u1, z.v1), P(z.u0, z.v1)], `fill="${fill}" fill-opacity="0.75" stroke="${stroke}" stroke-width="1.4" stroke-dasharray="6 5"`);
    zs += zoneLabel(z);
  }
  let es = '', packets = '', elabels = '';
  const DASH = 16; // período do tracejado; o loop anda 2 períodos → GIF sem emenda
  edges.forEach((e, i) => {
    const p = edgeGeom(e), col = theme.edge[e.style];
    const d = `M${p[0].map(f1)} C${p[1].map(f1)} ${p[2].map(f1)} ${p[3].map(f1)}`;
    const dash = animated ? `stroke-dasharray="10 6" stroke-dashoffset="${f1(-t * DASH * 2)}"` : '';
    es += `<path d="${d}" fill="none" stroke="${col}" stroke-width="2.6" stroke-linecap="round" ${dash} marker-end="url(#arr-${e.style})"/>`;
    if (animated) {
      const [x, y] = bez(p, (t + i * 0.37) % 1);
      packets += `<circle cx="${f1(x)}" cy="${f1(y)}" r="9" fill="${col}" opacity="0.28" filter="url(#glow)"/><circle cx="${f1(x)}" cy="${f1(y)}" r="4.2" fill="#fff" stroke="${col}" stroke-width="2.4"/>`;
    }
    if (e.label) { const [x, y] = bez(p, 0.5); elabels += pill(x, y, e.label); }
  });
  const sorted = [...nodes].sort((a, b) => a.u + a.v - (b.u + b.v));
  const ns = sorted.map(drawNode).join('');
  const ls = sorted.map(nodeLabel).join('');
  const marker = (name, col) => `<marker id="arr-${name}" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,1 L9,5 L0,9 Z" fill="${col}"/></marker>`;

  const vx = f1(BB.x0 - 60), vy = f1(BB.y0 - 130), vw = f1(BB.x1 - BB.x0 + 120), vh = f1(BB.y1 - BB.y0 + 190);
  VIEW = { w: vw, h: vh };
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${vw}" height="${vh}" viewBox="${vx} ${vy} ${vw} ${vh}" font-family="Helvetica Neue, Helvetica, Arial, sans-serif">
<defs>
  <filter id="blur" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="5"/></filter>
  <filter id="glow" x="-100%" y="-100%" width="300%" height="300%"><feGaussianBlur stdDeviation="3"/></filter>
  <radialGradient id="cloud" cx="0.35" cy="0.3" r="0.8"><stop offset="0" stop-color="#ffffff"/><stop offset="1" stop-color="#d9dee8"/></radialGradient>
  <radialGradient id="vignette" cx="0.5" cy="0.45" r="0.75"><stop offset="0.6" stop-color="#ffffff" stop-opacity="0"/><stop offset="1" stop-color="#c9d1e0" stop-opacity="0.55"/></radialGradient>
  ${marker('primary', theme.edge.primary)}${marker('secondary', theme.edge.secondary)}
  ${[...gradients.values()].join('')}
</defs>
<rect x="${vx}" y="${vy}" width="${vw}" height="${vh}" fill="${theme.bg}"/>
<g stroke="${theme.grid}" stroke-width="0.7">${g}</g>
${zs}${es}${packets}${ns}${ls}${elabels}
<rect x="${vx}" y="${vy}" width="${vw}" height="${vh}" fill="url(#vignette)"/>
<text x="${vx + 48}" y="${vy + 66}" font-size="26" font-weight="700" fill="${theme.ink}">Checkout Service</text>
<text x="${vx + 48}" y="${vy + 92}" font-size="14" fill="#5b6478">request path · isometric spike</text>
</svg>`;
}

// gradientes são registrados durante a montagem → monta 2x para incluí-los no <defs>
let VIEW;
const build = (t, anim) => { scene(t, anim); return scene(t, anim); };

const FONT = process.env.SYSFONTS ? {} : { font: { loadSystemFonts: false, fontFiles: ['/System/Library/Fonts/HelveticaNeue.ttc'], defaultFontFamily: 'Helvetica Neue' } };

// ---------- PNG estático @2x ----------
let t0 = performance.now();
const svgStatic = build(0, false);
writeFileSync(`${OUT}/spike.svg`, svgStatic);
const png = new Resvg(svgStatic, { ...FONT, fitTo: { mode: 'width', value: Math.round(VIEW.w * 2) } }).render().asPng();
writeFileSync(`${OUT}/spike.png`, png);
const tPng = performance.now() - t0;

// ---------- GIF animado (fluxo + pacotes), frames delta com transparência ----------
const N = 40, DELAY = 50, GW = 960;
t0 = performance.now();
const frames = [];
for (let f = 0; f < N; f++) {
  const r = new Resvg(build(f / N, true), { ...FONT, fitTo: { mode: 'width', value: GW } }).render();
  frames.push({ rgba: r.pixels, w: r.width, h: r.height });
}
const tRender = performance.now() - t0;
t0 = performance.now();
const { w: gw, h: gh } = frames[0];
const palette = quantize(frames[0].rgba, 255);
const TRANSP = palette.length; // índice extra reservado para "pixel não mudou"
palette.push([255, 0, 255]);
const gif = GIFEncoder();
let prev = null;
for (const fr of frames) {
  const idx = applyPalette(fr.rgba, palette.slice(0, TRANSP));
  const out = idx.slice();
  if (prev) for (let i = 0; i < out.length; i++) if (idx[i] === prev[i]) out[i] = TRANSP;
  gif.writeFrame(out, gw, gh, prev ? { palette, delay: DELAY, transparent: true, transparentIndex: TRANSP, dispose: 1 } : { palette, delay: DELAY, dispose: 1 });
  prev = idx;
}
gif.finish();
writeFileSync(`${OUT}/spike.gif`, gif.bytes());
const tGif = performance.now() - t0;

console.log(JSON.stringify({
  png: { ms: Math.round(tPng), px: `${Math.round(VIEW.w * 2)}x${Math.round(VIEW.h * 2)}`, kb: Math.round(png.length / 1024) },
  gif: { frames: N, px: `${gw}x${gh}`, renderMsTotal: Math.round(tRender), renderMsPerFrame: Math.round(tRender / N), encodeMs: Math.round(tGif), kb: Math.round(gif.bytes().length / 1024) },
}, null, 2));
