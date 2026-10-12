// Motor de cena do spike, parametrizado (temas, modo animado, SVG animado). Descartável.
const C = Math.cos(Math.PI / 6), S = Math.sin(Math.PI / 6);
const f1 = (n) => +n.toFixed(1);

export const THEMES = {
  blueprint: {
    bg: '#f2f4f8', grid: '#dde2ec', ink: '#1c2130', sub: '#5b6478', stroke: '#252b3b',
    zone: { violet: ['#ded5f6', '#7a5bd3'], blue: ['#cdd6f6', '#5468d4'], orange: ['#fbe2c8', '#d5843a'], teal: ['#c9e9e4', '#2c9a8c'] },
    zoneOpacity: 0.75,
    edge: { primary: '#d27a2c', secondary: '#3e50c8' },
    label: { bg: '#ffffff', fg: '#1c2130', border: '#c8cfdc', leader: '#252b3b' },
    pill: { bg: '#1f2434', fg: '#ffffff' },
    node: { server: '#56607a', led: '#7ef0a8', lb: '#3d4fd1', port: '#b9c3ff', db: '#a3acc0', cache: '#e0534f', cache2: '#ef6b5f', cloudA: '#ffffff', cloudB: '#d9dee8' },
    shadow: ['#1b2440', 0.22], glow: 0.28, vignette: '#c9d1e0',
  },
  midnight: {
    bg: '#0c111d', grid: '#182033', ink: '#eef1f8', sub: '#8d97ae', stroke: '#05070d',
    zone: { violet: ['#272048', '#8f73f2'], blue: ['#1b2652', '#6f86f5'], orange: ['#3a2818', '#f2a05a'], teal: ['#123432', '#38c2ae'] },
    zoneOpacity: 0.8,
    edge: { primary: '#ff9f4a', secondary: '#7d8cff' },
    label: { bg: '#141b2c', fg: '#eef1f8', border: '#2b3654', leader: '#55617f' },
    pill: { bg: '#eef1f8', fg: '#0c111d' },
    node: { server: '#3b4563', led: '#5cffa1', lb: '#5468ff', port: '#c8d0ff', db: '#8f9ab4', cache: '#ff5a54', cache2: '#ff7a66', cloudA: '#f4f6fb', cloudB: '#aab4c8' },
    shadow: ['#000000', 0.45], glow: 0.45, vignette: '#000000',
  },
};

const shade = (hex, k) => {
  const n = parseInt(hex.slice(1), 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => Math.max(0, Math.min(255, Math.round(c * k))));
  return '#' + ch.map((c) => c.toString(16).padStart(2, '0')).join('');
};
const textW = (str, size, bold) => str.length * size * (bold ? 0.6 : 0.55);

const HEIGHT = { cloud: 0.9, lb: 0.28, server: 1.0, db: 0.95, cache: 0.6 };

// Bézier cúbica + tabela de comprimento de arco (velocidade uniforme dos pacotes)
const bez = (p, t) => {
  const m = 1 - t;
  return [0, 1].map((i) => m * m * m * p[0][i] + 3 * m * m * t * p[1][i] + 3 * m * t * t * p[2][i] + t * t * t * p[3][i]);
};
function arcLUT(p, n = 200) {
  const lut = [0];
  let prev = bez(p, 0), acc = 0;
  for (let i = 1; i <= n; i++) {
    const q = bez(p, i / n);
    acc += Math.hypot(q[0] - prev[0], q[1] - prev[1]);
    lut.push(acc);
    prev = q;
  }
  return lut;
}
function atDistance(p, lut, frac) {
  const target = frac * lut[lut.length - 1];
  let lo = 0, hi = lut.length - 1;
  while (lo < hi) { const mid = (lo + hi) >> 1; if (lut[mid] < target) lo = mid + 1; else hi = mid; }
  const i = Math.max(1, lo), seg = lut[i] - lut[i - 1] || 1;
  return bez(p, (i - 1 + (target - lut[i - 1]) / seg) / (lut.length - 1));
}

// ---------- footprints no plano (para portas e visibilidade) ----------
const FOOT = {
  server: { rect: [0.42, 0.42], tall: true },
  lb: { rect: [0.55, 0.4], tall: false },
  db: { r: 0.48, tall: true },
  cache: { r: 0.48, tall: true },
  cloud: { r: 0.58, tall: true },
};

// ---------- layout mínimo: zonas em linha (espaçado) ou em bloco serpenteado (compacto) ----------
export const DENSITY = {
  spaced: { gap: 2.4, pad: 1.45, zoneGap: 1.0, lift: 52, wrap: (n) => n },
  compact: { gap: 1.7, pad: 0.95, zoneGap: 0.6, lift: 38, wrap: (n) => Math.ceil(Math.sqrt(n)) },
};
export function layoutScene(spec, density = 'spaced') {
  const D = DENSITY[density];
  const zones = spec.zones.map((z) => {
    const ns = spec.nodes.filter((n) => n.zone === z.id);
    const cols = Math.max(...ns.map((n) => n.col ?? 0)) + 1, rows = Math.max(...ns.map((n) => n.row ?? 0)) + 1;
    return { ...z, ns, w: 2 * D.pad + (cols - 1) * D.gap, d: 2 * D.pad + (rows - 1) * D.gap };
  });
  const per = D.wrap(zones.length), nRows = Math.ceil(zones.length / per);
  // coluna de cada zona (serpenteando: linhas ímpares invertem a ordem → o fluxo "dobra" em U)
  zones.forEach((z, i) => { const r = Math.floor(i / per), k = i % per; z.r = r; z.c = r % 2 ? per - 1 - k : k; });
  const colW = Array.from({ length: per }, (_, c) => Math.max(0, ...zones.filter((z) => z.c === c).map((z) => z.w)));
  const rowD = Array.from({ length: nRows }, (_, r) => Math.max(0, ...zones.filter((z) => z.r === r).map((z) => z.d)));
  const colX = colW.map((_, c) => colW.slice(0, c).reduce((a, b) => a + b + D.zoneGap, 0));
  const rowY = rowD.map((_, r) => rowD.slice(0, r).reduce((a, b) => a + b + D.zoneGap, 0));
  const nodes = [];
  for (const z of zones) {
    z.u0 = colX[z.c]; z.u1 = z.u0 + colW[z.c]; z.v0 = rowY[z.r]; z.v1 = z.v0 + rowD[z.r];
    const ox = (colW[z.c] - z.w) / 2, oy = (rowD[z.r] - z.d) / 2;
    for (const n of z.ns) nodes.push({ ...n, u: z.u0 + ox + D.pad + (n.col ?? 0) * D.gap, v: z.v0 + oy + D.pad + (n.row ?? 0) * D.gap });
  }
  return { ...spec, zones, nodes, lift: D.lift };
}

// rotação da câmera em passos de 90°: gira o mundo no plano em torno do centro das zonas
export function rotateScene(spec, deg = 0) {
  const k = (((Math.round(deg / 90)) % 4) + 4) % 4;
  if (!k) return spec;
  const zs = spec.zones ?? [];
  const cx = (Math.min(...zs.map((z) => z.u0)) + Math.max(...zs.map((z) => z.u1))) / 2;
  const cy = (Math.min(...zs.map((z) => z.v0)) + Math.max(...zs.map((z) => z.v1))) / 2;
  const rot = (u, v) => { let x = u - cx, y = v - cy; for (let i = 0; i < k; i++) [x, y] = [-y, x]; return [x + cx, y + cy]; };
  return {
    ...spec,
    zones: zs.map((z) => { const [a, b] = rot(z.u0, z.v0), [c, d] = rot(z.u1, z.v1); return { ...z, u0: Math.min(a, c), u1: Math.max(a, c), v0: Math.min(b, d), v1: Math.max(b, d) }; }),
    nodes: spec.nodes.map((n) => { const [u, v] = rot(n.u, n.v); return { ...n, u, v, swap: k % 2 === 1 }; }),
  };
}

/**
 * spec: { title?, subtitle?, zones, nodes, edges }
 * opts: { theme, t (0..1), animated, svgAnim (CSS/SMIL), loopSec, vignette, T (px/tile), pad, titleSpace }
 */
export function renderScene(spec, opts = {}) {
  if (opts.rotation === 'auto') {
    const tries = [0, 90, 180, 270].map((rotation) => ({ rotation, r: renderScene(spec, { ...opts, rotation }) }));
    tries.sort((a, b) => a.r.score - b.r.score || a.r.w * a.r.h - b.r.w * b.r.h);
    return { ...tries[0].r, rotation: tries[0].rotation, tried: tries.map((x) => ({ rotation: x.rotation, score: x.r.score })) };
  }
  if (spec.nodes[0].u === undefined) spec = layoutScene(spec, opts.density ?? 'spaced');
  spec = rotateScene(spec, opts.rotation ?? 0);
  const th = THEMES[opts.theme ?? 'blueprint'];
  const T = opts.T ?? 56, t = opts.t ?? 0, loop = opts.loopSec ?? 2;
  const animated = !!opts.animated, svgAnim = !!opts.svgAnim;
  const BB = { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity };
  let NB = null; // caixa de tela do nó sendo desenhado (para colisões)
  const grow = (b, x, y) => { b.x0 = Math.min(b.x0, x); b.y0 = Math.min(b.y0, y); b.x1 = Math.max(b.x1, x); b.y1 = Math.max(b.y1, y); };
  const track = (x, y) => { grow(BB, x, y); if (NB) grow(NB, x, y); };
  const PR = (u, v, z = 0) => [(u - v) * C * T, (u + v) * S * T - z * T];
  const P = (u, v, z = 0) => { const p = PR(u, v, z); track(p[0], p[1]); return p; };
  const pts = (a) => a.map(([x, y]) => `${f1(x)},${f1(y)}`).join(' ');
  const poly = (c, attrs) => `<polygon points="${pts(c)}" ${attrs}/>`;
  const defs = new Map();
  const byId = Object.fromEntries(spec.nodes.map((n) => [n.id, n]));
  const sa = `stroke="${th.stroke}" stroke-width="1.1" stroke-linejoin="round"`;

  const box = (u, v, w, d, h, color, z0 = 0) =>
    poly([P(u, v + d, z0), P(u + w, v + d, z0), P(u + w, v + d, z0 + h), P(u, v + d, z0 + h)], `fill="${shade(color, 0.8)}" ${sa}`) +
    poly([P(u + w, v, z0), P(u + w, v + d, z0), P(u + w, v + d, z0 + h), P(u + w, v, z0 + h)], `fill="${shade(color, 0.62)}" ${sa}`) +
    poly([P(u, v, z0 + h), P(u + w, v, z0 + h), P(u + w, v + d, z0 + h), P(u, v + d, z0 + h)], `fill="${color}" ${sa}`);

  const cylinder = (cu, cv, r, h, color, z0 = 0) => {
    const id = `cg${color.slice(1)}`;
    defs.set(id, `<linearGradient id="${id}" x1="0" x2="1"><stop offset="0" stop-color="${shade(color, 0.95)}"/><stop offset="0.55" stop-color="${shade(color, 0.78)}"/><stop offset="1" stop-color="${shade(color, 0.55)}"/></linearGradient>`);
    const [cx, yb] = P(cu, cv, z0), [, yt] = P(cu, cv, z0 + h);
    const rx = Math.SQRT2 * C * r * T, ry = Math.SQRT2 * S * r * T;
    track(cx - rx, yt - ry); track(cx + rx, yb + ry);
    return `<path d="M${f1(cx - rx)},${f1(yt)} L${f1(cx - rx)},${f1(yb)} A${f1(rx)},${f1(ry)} 0 0 0 ${f1(cx + rx)},${f1(yb)} L${f1(cx + rx)},${f1(yt)} Z" fill="url(#${id})" ${sa}/>` +
      `<ellipse cx="${f1(cx)}" cy="${f1(yt)}" rx="${f1(rx)}" ry="${f1(ry)}" fill="${shade(color, 1.12)}" ${sa}/>`;
  };

  const shadow = (u, v, r = 0.55) => {
    const [x, y] = PR(u + 0.12, v - 0.05, 0);
    return `<ellipse cx="${f1(x)}" cy="${f1(y)}" rx="${f1(Math.SQRT2 * C * r * T)}" ry="${f1(Math.SQRT2 * S * r * T)}" fill="${th.shadow[0]}" opacity="${th.shadow[1]}" filter="url(#blur)"/>`;
  };

  const drawNode = (n) => {
    const { u, v } = n, N = th.node;
    let s = shadow(u, v);
    switch (n.kind) {
      case 'server':
        for (let i = 0; i < 3; i++) {
          const z = i * 0.34;
          s += box(u - 0.42, v - 0.42, 0.84, 0.84, 0.3, N.server, z);
          const [a, b] = [P(u - 0.3, v + 0.42, z + 0.15), P(u + 0.05, v + 0.42, z + 0.15)];
          s += `<line x1="${f1(a[0])}" y1="${f1(a[1])}" x2="${f1(b[0])}" y2="${f1(b[1])}" stroke="${N.led}" stroke-width="2.2" stroke-linecap="round"/>`;
        }
        return s;
      case 'lb':
        { const [hw, hd] = n.swap ? [0.4, 0.55] : [0.55, 0.4]; s += box(u - hw, v - hd, 2 * hw, 2 * hd, 0.28, N.lb); }
        for (let i = 0; i < 5; i++) {
          const hw = n.swap ? 0.4 : 0.55, hd = n.swap ? 0.55 : 0.4;
          const [x, y] = P(u - hw + 0.13 + i * ((2 * hw - 0.26) / 4), v + hd, 0.14);
          s += `<circle cx="${f1(x)}" cy="${f1(y)}" r="2.2" fill="${N.port}"/>`;
        }
        return s;
      case 'db':
        for (let i = 0; i < 3; i++) s += cylinder(u, v, 0.48, 0.28, N.db, i * 0.33);
        return s;
      case 'cache':
        return s + cylinder(u, v, 0.48, 0.3, N.cache, 0) + cylinder(u, v, 0.48, 0.24, N.cache2, 0.36);
      case 'cloud': {
        defs.set('cloud', `<radialGradient id="cloud" cx="0.35" cy="0.3" r="0.8"><stop offset="0" stop-color="${N.cloudA}"/><stop offset="1" stop-color="${N.cloudB}"/></radialGradient>`);
        const [x, y] = P(u, v, 0.45);
        const puffs = [[-22, 6, 17], [0, -6, 23], [22, 4, 18], [-6, 12, 16], [12, 13, 15]];
        for (const [dx, dy, r] of puffs) { track(x + dx - r, y + dy - r); s += `<circle cx="${f1(x + dx)}" cy="${f1(y + dy)}" r="${r}" fill="none" stroke="${th.stroke}" stroke-width="2.2"/>`; }
        for (const [dx, dy, r] of puffs) s += `<circle cx="${f1(x + dx)}" cy="${f1(y + dy)}" r="${r}" fill="url(#cloud)"/>`;
        return s;
      }
    }
    return s;
  };

  const nodeLabel = (n) => {
    if (!n.label) return '';
    const [x, y0] = P(n.u, n.v, HEIGHT[n.kind] ?? 0.6);
    const y = y0 - (n.lift ?? spec.lift ?? 52), w = textW(n.label, 14) + 22;
    track(x - w / 2, y - 28); track(x + w / 2, y);
    return `<line x1="${f1(x)}" y1="${f1(y0)}" x2="${f1(x)}" y2="${f1(y)}" stroke="${th.label.leader}" stroke-width="1.2"/>` +
      `<rect x="${f1(x - w / 2)}" y="${f1(y - 28)}" width="${f1(w)}" height="28" rx="7" fill="${th.label.bg}" stroke="${th.label.border}" stroke-width="1.2"/>` +
      `<text x="${f1(x)}" y="${f1(y - 9.5)}" font-size="14" font-weight="500" text-anchor="middle" fill="${th.label.fg}">${n.label}</text>`;
  };
  const pill = (x, y, label) => {
    const w = textW(label, 11, true) + 14;
    return `<rect x="${f1(x - w / 2)}" y="${f1(y - 10)}" width="${f1(w)}" height="20" rx="5" fill="${th.pill.bg}"/>` +
      `<text x="${f1(x)}" y="${f1(y + 4)}" font-size="11" font-weight="700" text-anchor="middle" fill="${th.pill.fg}">${label}</text>`;
  };
  const zoneLabel = (z) => {
    const [x, y] = P(z.u0 + 0.3, z.v1 - 0.5, 0);
    const w = textW(z.label, 12, true) + 16;
    return `<g transform="matrix(${C},${S},${-C},${S},${f1(x)},${f1(y)})"><rect width="${f1(w)}" height="20" rx="4" fill="${th.pill.bg}"/>` +
      `<text x="8" y="14.5" font-size="12" font-weight="700" letter-spacing="0.6" fill="${th.pill.fg}">${z.label}</text></g>`;
  };

  // footprint do nó (respeita rotação do LB)
  const foot = (n) => { const f = FOOT[n.kind] ?? { r: 0.5, tall: false }; return f.rect && n.swap ? { ...f, rect: [f.rect[1], f.rect[0]] } : f; };
  const unit = (x, y) => { const l = Math.hypot(x, y) || 1; return [x / l, y / l]; };
  // ponto da borda do footprint na direção d (a partir do centro)
  const exitAt = (n, d) => {
    const f = foot(n);
    if (f.r) return { p: [n.u + d[0] * f.r, n.v + d[1] * f.r], back: d[0] + d[1] < -1e-6 };
    const [hw, hd] = f.rect, t = Math.min(hw / (Math.abs(d[0]) || 1e-9), hd / (Math.abs(d[1]) || 1e-9));
    const xFace = Math.abs(d[0]) * hd >= Math.abs(d[1]) * hw;
    return { p: [n.u + d[0] * t, n.v + d[1] * t], back: xFace ? d[0] < 0 : d[1] < 0 };
  };
  // porta de chegada: se a borda natural estiver atrás de um nó alto (escondida), desliza para o canto lateral visível
  const inPort = (n, d) => {
    const f = foot(n), e = exitAt(n, d);
    let p = e.p;
    if (f.tall && e.back) {
      const left = d[0] - d[1] < 0; // canto esquerdo da tela (−x,+y) ou direito (+x,−y)
      p = f.r ? [n.u + (left ? -1 : 1) * f.r / Math.SQRT2, n.v + (left ? 1 : -1) * f.r / Math.SQRT2]
              : [n.u + (left ? -f.rect[0] : f.rect[0]), n.v + (left ? f.rect[1] : -f.rect[1])];
    }
    const aim = unit(n.u - p[0], n.v - p[1]); // a ponta sempre aponta para o centro do componente
    return { p: [p[0] - aim[0] * 0.08, p[1] - aim[1] * 0.08], aim };
  };
  const proj = (d) => { const q = [(d[0] - d[1]) * C, (d[0] + d[1]) * S]; const l = Math.hypot(...q); return [q[0] / l, q[1] / l]; };
  const AL = 0.3, AW = 0.13, ZE = 0.04; // comprimento/meia-largura da ponta (tiles) e altura do traço
  const edgeGeom = (e) => {
    const a = byId[e.from], b = byId[e.to];
    const out = exitAt(a, unit(b.u - a.u, b.v - a.v));
    const inn = inPort(b, unit(a.u - b.u, a.v - b.v));
    const outDir = unit(out.p[0] - a.u, out.p[1] - a.v);
    // ponta da seta no plano do chão: triângulo em coordenadas de tiles, depois projetado
    const tip = inn.p, base = [tip[0] - inn.aim[0] * AL, tip[1] - inn.aim[1] * AL], perp = [-inn.aim[1], inn.aim[0]];
    const head = [P(tip[0], tip[1], ZE), P(base[0] + perp[0] * AW, base[1] + perp[1] * AW, ZE), P(base[0] - perp[0] * AW, base[1] - perp[1] * AW, ZE)];
    const p0 = P(out.p[0], out.p[1], ZE), p3 = P(base[0], base[1], ZE);
    const k = 0.42 * Math.hypot(p3[0] - p0[0], p3[1] - p0[1]);
    const so = proj(outDir), si = proj(inn.aim);
    return { bz: [p0, [p0[0] + so[0] * k, p0[1] + so[1] * k], [p3[0] - si[0] * k, p3[1] - si[1] * k], p3], head };
  };

  // ---- corpo ----
  // Ordem de cálculo ≠ ordem de pintura: nós → labels de nó → arestas/pílulas → labels de zona,
  // para cada etapa desviar do que já foi posicionado (colisão gulosa, como descrito no doc 06).
  const box0 = () => ({ x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity });
  const hit = (a, b, m = 3) => Math.max(0, Math.min(a.x1, b.x1) - Math.max(a.x0, b.x0) + m) * Math.max(0, Math.min(a.y1, b.y1) - Math.max(a.y0, b.y0) + m);
  const cost = (r, list) => list.reduce((acc, o) => acc + hit(r, o), 0);
  let score = 0; // área residual de colisão (px²): 0 = nada se sobrepõe
  const pick = (cands, obstacles) => { let best = null; for (const c of cands) { const k = cost(c.r, obstacles); if (k === 0) return c; if (!best || k < best.k) best = { ...c, k }; } score += best.k; return best; };

  // 1) nós (com caixa de tela de cada um)
  const sorted = [...spec.nodes].sort((a, b) => a.u + a.v - (b.u + b.v));
  const nodeBox = new Map();
  const ns = sorted.map((n) => { NB = box0(); const out = drawNode(n); nodeBox.set(n.id, NB); NB = null; return out; }).join('');
  const nodeBoxes = [...nodeBox.values()];

  // 2) labels de nó: altura da guia e deslocamento lateral escolhidos para não colidir
  const placed = [];
  let ls = '';
  for (const n of [...sorted].filter((n) => n.label).sort((a, b) => PR(a.u, a.v)[1] - PR(b.u, b.v)[1])) {
    const [x, y0] = PR(n.u, n.v, HEIGHT[n.kind] ?? 0.6), w = textW(n.label, 14) + 22, base = n.lift ?? spec.lift ?? 52;
    const cands = [];
    for (const lift of [base, base + 28, base + 56, base + 84, Math.max(16, base - 18)])
      for (const dx of [0, -0.32 * w, 0.32 * w]) {
        const y = y0 - lift, cx = x + dx;
        cands.push({ y, cx, r: { x0: cx - w / 2, y0: y - 28, x1: cx + w / 2, y1: y } });
      }
    const others = nodeBoxes.filter((b) => b !== nodeBox.get(n.id));
    const c = pick(cands, [...placed, ...others]);
    placed.push(c.r); track(c.r.x0, c.r.y0); track(c.r.x1, c.r.y1);
    ls += `<line x1="${f1(x)}" y1="${f1(y0)}" x2="${f1(x)}" y2="${f1(c.y)}" stroke="${th.label.leader}" stroke-width="1.2"/>` +
      `<rect x="${f1(c.r.x0)}" y="${f1(c.y - 28)}" width="${f1(w)}" height="28" rx="7" fill="${th.label.bg}" stroke="${th.label.border}" stroke-width="1.2"/>` +
      `<text x="${f1(c.cx)}" y="${f1(c.y - 9.5)}" font-size="14" font-weight="500" text-anchor="middle" fill="${th.label.fg}">${n.label}</text>`;
  }

  // 3) arestas; a pílula desliza ao longo da curva até achar espaço livre
  let es = '', packets = '', elabels = '';
  const arrows = [], pills = [];
  const DASH = 16;
  (spec.edges ?? []).forEach((e, i) => {
    const g = edgeGeom(e), p = g.bz, col = th.edge[e.style ?? 'primary'];
    { const b = byId[e.to], c = PR(b.u, b.v, ZE), m = [(g.head[1][0] + g.head[2][0]) / 2, (g.head[1][1] + g.head[2][1]) / 2];
      arrows.push({ edge: `${e.from}->${e.to}`, tip: g.head[0], mid: m, center: c, curveEnd: [p[3][0] - p[2][0], p[3][1] - p[2][1]] }); }
    const d = `M${p[0].map(f1)} C${p[1].map(f1)} ${p[2].map(f1)} ${p[3].map(f1)}`;
    const phase = (i * 0.37) % 1;
    let dash = '';
    if (svgAnim) dash = 'class="flow"';
    else if (animated) dash = `stroke-dasharray="10 6" stroke-dashoffset="${f1(-t * DASH * 2)}"`;
    es += `<path d="${d}" fill="none" stroke="${col}" stroke-width="2.6" stroke-linecap="round" ${dash}/>` +
      `<polygon points="${pts(g.head)}" fill="${col}" stroke="${col}" stroke-width="1" stroke-linejoin="round"/>`;
    const dot = (x, y) => `<circle cx="${x}" cy="${y}" r="9" fill="${col}" opacity="${th.glow}" filter="url(#glow)"/><circle cx="${x}" cy="${y}" r="4.2" fill="#fff" stroke="${col}" stroke-width="2.4"/>`;
    if (svgAnim) {
      packets += `<g>${dot(0, 0)}<animateMotion dur="${loop}s" repeatCount="indefinite" begin="${f1(-phase * loop)}s" calcMode="paced" path="${d}"/></g>`;
    } else if (animated) {
      const [x, y] = atDistance(p, arcLUT(p), (t + phase) % 1);
      packets += dot(f1(x), f1(y));
    }
    if (e.label) {
      const w = textW(e.label, 11, true) + 14, lut = arcLUT(p);
      const cands = [0.5, 0.4, 0.6, 0.32, 0.68, 0.25, 0.75].map((tt) => { const [x, y] = atDistance(p, lut, tt); return { x, y, r: { x0: x - w / 2, y0: y - 10, x1: x + w / 2, y1: y + 10 } }; });
      const c = pick(cands, [...placed, ...nodeBoxes, ...pills]);
      pills.push(c.r);
      elabels += pill(c.x, c.y, e.label);
    }
  });

  // 4) zonas e seus labels (deitados no chão), no canto/borda frontal que estiver livre
  let zs = '';
  for (const z of spec.zones ?? []) {
    const [fill, stroke] = th.zone[z.color];
    zs += poly([P(z.u0, z.v0), P(z.u1, z.v0), P(z.u1, z.v1), P(z.u0, z.v1)], `fill="${fill}" fill-opacity="${th.zoneOpacity}" stroke="${stroke}" stroke-width="1.4" stroke-dasharray="6 5"`);
    const w = textW(z.label, 12, true) + 16, wT = w / T, hT = 20 / T;
    const cand = (ua, va, alongU) => {
      const corners = alongU ? [[ua, va], [ua + wT, va], [ua + wT, va + hT], [ua, va + hT]] : [[ua, va], [ua, va - wT], [ua + hT, va - wT], [ua + hT, va]];
      const r = box0(); for (const [cu, cv] of corners) { const q = PR(cu, cv); grow(r, q[0], q[1]); }
      return { ua, va, alongU, r };
    };
    const c = pick([
      cand(z.u0 + 0.3, z.v1 - 0.5, true), cand(z.u1 - 0.3 - wT, z.v1 - 0.5, true),
      cand(z.u1 - 0.5, z.v1 - 0.3, false), cand(z.u1 - 0.5, z.v0 + 0.3 + wT, false),
    ], [...nodeBoxes, ...placed, ...pills]);
    const [x, y] = P(c.ua, c.va, 0);
    const m = c.alongU ? `${C},${S},${-C},${S}` : `${C},${-S},${C},${S}`;
    zs += `<g transform="matrix(${m},${f1(x)},${f1(y)})"><rect width="${f1(w)}" height="20" rx="4" fill="${th.pill.bg}"/>` +
      `<text x="8" y="14.5" font-size="12" font-weight="700" letter-spacing="0.6" fill="${th.pill.fg}">${z.label}</text></g>`;
  }

  // ---- enquadramento ----
  const pad = opts.pad ?? 60, top = spec.title ? (opts.titleSpace ?? 130) : pad;
  const vx = f1(BB.x0 - pad), vy = f1(BB.y0 - top), vw = f1(BB.x1 - BB.x0 + 2 * pad), vh = f1(BB.y1 - BB.y0 + top + pad);
  let grid = '';
  const gx0 = vx, gy0 = vy, k0 = -60, k1 = 80;
  for (let k = k0; k <= k1; k++) {
    const [a, b] = [PR(k, k0), PR(k, k1)], [c, d] = [PR(k0, k), PR(k1, k)];
    grid += `<line x1="${f1(a[0])}" y1="${f1(a[1])}" x2="${f1(b[0])}" y2="${f1(b[1])}"/><line x1="${f1(c[0])}" y1="${f1(c[1])}" x2="${f1(d[0])}" y2="${f1(d[1])}"/>`;
  }
  const marker = (name, col) => `<marker id="arr-${name}" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0,1 L9,5 L0,9 Z" fill="${col}"/></marker>`;
  const css = svgAnim ? `<style>.flow{stroke-dasharray:10 6;animation:flow ${loop}s linear infinite}@keyframes flow{to{stroke-dashoffset:-${DASH * 2}}}</style>` : '';
  const vignette = opts.vignette
    ? `<radialGradient id="vig" cx="0.5" cy="0.45" r="0.75"><stop offset="0.6" stop-color="${th.vignette}" stop-opacity="0"/><stop offset="1" stop-color="${th.vignette}" stop-opacity="0.5"/></radialGradient>` : '';
  const title = spec.title
    ? `<text x="${f1(gx0 + 48)}" y="${f1(gy0 + 66)}" font-size="26" font-weight="700" fill="${th.ink}">${spec.title}</text>` +
      (spec.subtitle ? `<text x="${f1(gx0 + 48)}" y="${f1(gy0 + 92)}" font-size="14" fill="${th.sub}">${spec.subtitle}</text>` : '') : '';

  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${vw}" height="${vh}" viewBox="${vx} ${vy} ${vw} ${vh}" font-family="Inter, sans-serif">
${css}<defs>
<filter id="blur" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="5"/></filter>
<filter id="glow" x="-100%" y="-100%" width="300%" height="300%"><feGaussianBlur stdDeviation="3"/></filter>
${marker('primary', th.edge.primary)}${marker('secondary', th.edge.secondary)}${vignette}${[...defs.values()].join('')}
</defs>
<rect x="${vx}" y="${vy}" width="${vw}" height="${vh}" fill="${th.bg}"/>
<g stroke="${th.grid}" stroke-width="0.7">${grid}</g>
${zs}${es}${packets}${ns}${ls}${elabels}
${opts.vignette ? `<rect x="${vx}" y="${vy}" width="${vw}" height="${vh}" fill="url(#vig)"/>` : ''}
${title}
</svg>`;
  return { svg, w: vw, h: vh, vb: [vx, vy, vw, vh], arrows, score: Math.round(score) };
}

export const CHECKOUT = {
  title: 'Checkout Service',
  subtitle: 'request path · rendered by dioramic',
  zones: [
    { label: 'INTERNET', color: 'violet', u0: 0, v0: 0.5, u1: 3, v1: 3.5 },
    { label: 'EDGE', color: 'blue', u0: 4, v0: 0.5, u1: 7, v1: 3.5 },
    { label: 'APP TIER', color: 'orange', u0: 8, v0: -0.5, u1: 11, v1: 4.5 },
    { label: 'DATA', color: 'teal', u0: 12, v0: -0.5, u1: 15, v1: 4.5 },
  ],
  nodes: [
    { id: 'users', kind: 'cloud', u: 1.5, v: 2, label: 'Users' },
    { id: 'lb', kind: 'lb', u: 5.5, v: 2, label: 'Load Balancer' },
    { id: 'api1', kind: 'server', u: 9.5, v: 0.8, label: 'API #1' },
    { id: 'api2', kind: 'server', u: 9.5, v: 3.2, label: 'API #2' },
    { id: 'db', kind: 'db', u: 13.5, v: 0.8, label: 'Postgres' },
    { id: 'cache', kind: 'cache', u: 13.5, v: 3.2, label: 'Redis', lift: 40 },
  ],
  edges: [
    { from: 'users', to: 'lb', label: 'HTTPS', style: 'primary' },
    { from: 'lb', to: 'api1', style: 'primary' },
    { from: 'lb', to: 'api2', label: 'round-robin', style: 'primary' },
    { from: 'api1', to: 'db', label: 'SQL', style: 'secondary' },
    { from: 'api2', to: 'db', style: 'secondary' },
    { from: 'api1', to: 'cache', style: 'secondary' },
    { from: 'api2', to: 'cache', label: 'GET / SET', style: 'secondary' },
  ],
};

// Cena da demo/banner no formato de layout (zona + linha), sem coordenadas
export const HERO = {
  title: 'Checkout Service',
  subtitle: 'request path · rendered by dioramic',
  zones: [
    { id: 'internet', label: 'INTERNET', color: 'violet' },
    { id: 'edge', label: 'EDGE', color: 'blue' },
    { id: 'app', label: 'APP TIER', color: 'orange' },
    { id: 'data', label: 'DATA', color: 'teal' },
  ],
  nodes: [
    { id: 'users', kind: 'cloud', label: 'Users', zone: 'internet' },
    { id: 'lb', kind: 'lb', label: 'Load Balancer', zone: 'edge' },
    { id: 'api1', kind: 'server', label: 'API #1', zone: 'app', row: 0 },
    { id: 'api2', kind: 'server', label: 'API #2', zone: 'app', row: 1 },
    { id: 'db', kind: 'db', label: 'Postgres', zone: 'data', row: 0 },
    { id: 'cache', kind: 'cache', label: 'Redis', zone: 'data', row: 1 },
  ],
  edges: [
    { from: 'users', to: 'lb', label: 'HTTPS', style: 'primary' },
    { from: 'lb', to: 'api1', style: 'primary' },
    { from: 'lb', to: 'api2', label: 'round-robin', style: 'primary' },
    { from: 'api1', to: 'db', label: 'SQL', style: 'secondary' },
    { from: 'api2', to: 'db', style: 'secondary' },
    { from: 'api2', to: 'cache', label: 'GET / SET', style: 'secondary' },
  ],
};
