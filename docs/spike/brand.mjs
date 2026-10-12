// Gera a identidade visual do Dioramic: marca, wordmark, lockups, ícone, banner/social preview.
import { Resvg } from '@resvg/resvg-js';
import { writeFileSync, mkdirSync } from 'node:fs';
import { renderScene, HERO } from './scene.mjs';

const OUT = process.argv[2] ?? './brand-out';
for (const d of ['logo', 'icon', 'banner']) mkdirSync(`${OUT}/${d}`, { recursive: true });
const FONTS = ['../fonts/Inter-400.ttf', '../fonts/Inter-500.ttf', '../fonts/Inter-600.ttf', '../fonts/Inter-700.ttf', '../fonts/SpaceGrotesk-500.ttf', '../fonts/SpaceGrotesk-700.ttf'];
const FONT = { font: { loadSystemFonts: false, fontFiles: FONTS, defaultFontFamily: 'Inter' } };
const png = (svg, width) => new Resvg(svg, { ...FONT, fitTo: { mode: 'width', value: width } }).render().asPng();

// ---------------- paleta da marca ----------------
export const BRAND = {
  indigo: '#3E50C8', indigoDeep: '#2C3A9E', indigoLight: '#6F86F5',
  ember: '#E8833A', emberLight: '#F5A25A', emberDeep: '#C2621F',
  ink: '#1C2130', paper: '#F2F4F8', night: '#0C111D',
  slate: '#56607A', teal: '#2C9A8C', mint: '#7EF0A8', mist: '#DDE2EC',
};
const B = BRAND;
const C = Math.cos(Math.PI / 6), S = 0.5;
const f = (n) => +n.toFixed(2);
const shade = (hex, k) => {
  const n = parseInt(hex.slice(1), 16);
  return '#' + [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => Math.max(0, Math.min(255, Math.round(c * k))).toString(16).padStart(2, '0')).join('');
};

// ---------------- marca: um diorama sobre uma base ----------------
function mark({ simple = false, dark = false } = {}) {
  const T = 40, sw = simple ? 3.2 : 2.2;
  const ink = dark ? '#05070D' : B.ink;
  const bb = { x0: 1e9, y0: 1e9, x1: -1e9, y1: -1e9 };
  const P = (u, v, z = 0) => { const p = [(u - v) * C * T, (u + v) * S * T - z * T]; bb.x0 = Math.min(bb.x0, p[0]); bb.y0 = Math.min(bb.y0, p[1]); bb.x1 = Math.max(bb.x1, p[0]); bb.y1 = Math.max(bb.y1, p[1]); return p; };
  const poly = (pts, fill) => `<polygon points="${pts.map((p) => p.map(f).join(',')).join(' ')}" fill="${fill}" stroke="${ink}" stroke-width="${sw}" stroke-linejoin="round"/>`;
  const box = (u, v, w, d, h, top, left, right, z0 = 0) =>
    poly([P(u, v + d, z0), P(u + w, v + d, z0), P(u + w, v + d, z0 + h), P(u, v + d, z0 + h)], left) +
    poly([P(u + w, v, z0), P(u + w, v + d, z0), P(u + w, v + d, z0 + h), P(u + w, v, z0 + h)], right) +
    poly([P(u, v, z0 + h), P(u + w, v, z0 + h), P(u + w, v + d, z0 + h), P(u, v + d, z0 + h)], top);

  const Z = 0.7; // altura da base
  let s = box(0, 0, 4, 4, Z, dark ? '#E9EDF8' : '#EEF1FB', B.indigo, B.indigoDeep);
  if (!simple) {
    // grid no tampo da base
    for (const k of [1, 2, 3]) {
      const [a, b] = [P(k, 0, Z), P(k, 4, Z)], [c, d] = [P(0, k, Z), P(4, k, Z)];
      s += `<path d="M${a.map(f)} L${b.map(f)} M${c.map(f)} L${d.map(f)}" stroke="#C9D1EA" stroke-width="1.2"/>`;
    }
  }
  // aresta + pacote (desenhados antes dos nós, ficam no "chão" do diorama)
  const p0 = P(1.55, 2.35, Z), p3 = P(2.62, 1.8, Z);
  const c1 = [p0[0] + 4, p0[1] + 30], c2 = [p3[0] - 14, p3[1] + 30];
  s += `<path d="M${p0.map(f)} C${c1.map(f)} ${c2.map(f)} ${p3.map(f)}" fill="none" stroke="${B.ember}" stroke-width="${simple ? 5 : 4}" stroke-linecap="round"/>`;
  if (!simple) {
    const t = 0.5, m = 1 - t;
    const q = [0, 1].map((i) => m * m * m * p0[i] + 3 * m * m * t * c1[i] + 3 * m * t * t * c2[i] + t * t * t * p3[i]);
    s += `<circle cx="${f(q[0])}" cy="${f(q[1])}" r="5.2" fill="#fff" stroke="${B.ember}" stroke-width="2.6"/>`;
  }
  // torre de servidor (atrás)
  s += box(0.5, 1.35, 1.25, 1.25, 2.2, '#6B7592', shade(B.slate, 0.95), shade(B.slate, 0.72), Z);
  if (!simple) {
    for (const k of [0.45, 1.05, 1.65]) {
      const [a, b] = [P(0.7, 2.6, Z + k + 0.25), P(1.2, 2.6, Z + k + 0.25)];
      s += `<line x1="${f(a[0])}" y1="${f(a[1])}" x2="${f(b[0])}" y2="${f(b[1])}" stroke="${B.mint}" stroke-width="3" stroke-linecap="round"/>`;
    }
  }
  // cilindro (banco de dados, à frente)
  const [cx, yb] = P(3.0, 1.45, Z), [, yt] = P(3.0, 1.45, Z + 1.15);
  const r = 0.66, rx = Math.SQRT2 * C * r * T, ry = Math.SQRT2 * S * r * T;
  bb.x1 = Math.max(bb.x1, cx + rx); bb.y0 = Math.min(bb.y0, yt - ry);
  s += `<defs><linearGradient id="cyl" x1="0" x2="1"><stop offset="0" stop-color="${shade(B.teal, 1.12)}"/><stop offset="0.55" stop-color="${B.teal}"/><stop offset="1" stop-color="${shade(B.teal, 0.72)}"/></linearGradient></defs>`;
  s += `<path d="M${f(cx - rx)},${f(yt)} L${f(cx - rx)},${f(yb)} A${f(rx)},${f(ry)} 0 0 0 ${f(cx + rx)},${f(yb)} L${f(cx + rx)},${f(yt)} Z" fill="url(#cyl)" stroke="${ink}" stroke-width="${sw}"/>`;
  if (!simple) {
    const ym = (yt + yb) / 2;
    s += `<path d="M${f(cx - rx)},${f(ym)} A${f(rx)},${f(ry)} 0 0 0 ${f(cx + rx)},${f(ym)}" fill="none" stroke="${ink}" stroke-width="${sw * 0.7}"/>`;
  }
  s += `<ellipse cx="${f(cx)}" cy="${f(yt)}" rx="${f(rx)}" ry="${f(ry)}" fill="${shade(B.teal, 1.35)}" stroke="${ink}" stroke-width="${sw}"/>`;
  const pad = sw * 2;
  return { body: s, vb: [bb.x0 - pad, bb.y0 - pad, bb.x1 - bb.x0 + 2 * pad, bb.y1 - bb.y0 + 2 * pad] };
}
const svgOf = (m, extra = '') => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${m.vb.map(f).join(' ')}" width="${f(m.vb[2])}" height="${f(m.vb[3])}">${extra}${m.body}</svg>`;

// ---------------- wordmark: "dıoramıc" com cubos isométricos no lugar dos pingos ----------------
function wordmark({ dark = false } = {}) {
  const ink = dark ? '#EEF1F8' : B.ink;
  const text = `<svg xmlns="http://www.w3.org/2000/svg" width="420" height="100"><text x="0" y="80" font-family="Space Grotesk" font-weight="700" font-size="100" letter-spacing="-2" fill="${ink}">dıoramıc</text></svg>`;
  const outlined = new Resvg(text, FONT).toString();
  const paths = outlined.replace(/^[\s\S]*?<g>/, '<g>').replace(/<\/svg>\s*$/, '');
  // centros das hastes dos "ı" (medidos com getBBox: bordas direitas 81,4 e 340,8; haste de 12,6)
  const cube = (cx, cy, s, top, left, right) => {
    const pt = (x, y) => `${f(x)},${f(y)}`;
    const T0 = [cx, cy - s], UR = [cx + s * C, cy - s / 2], LR = [cx + s * C, cy + s / 2], Bt = [cx, cy + s], LL = [cx - s * C, cy + s / 2], UL = [cx - s * C, cy - s / 2], M = [cx, cy];
    const pg = (ps, fill) => `<polygon points="${ps.map((p) => pt(...p)).join(' ')}" fill="${fill}"/>`;
    return pg([T0, UR, M, UL], top) + pg([UL, M, Bt, LL], left) + pg([M, UR, LR, Bt], right);
  };
  const i1 = dark ? [B.indigoLight, '#5A70E6', '#4458C8'] : ['#5D70E0', B.indigo, B.indigoDeep];
  const i2 = [B.emberLight, B.ember, B.emberDeep];
  const cubes = cube(75.1, 14.5, 10.5, ...i1) + cube(334.5, 14.5, 10.5, ...i2);
  return { body: paths + cubes, vb: [0, 2, 402, 80] };
}

// ---------------- lockup horizontal: marca + wordmark ----------------
function lockup({ dark = false } = {}) {
  const m = mark({ dark }), w = wordmark({ dark });
  const mh = 118, ms = mh / m.vb[3], mw = m.vb[2] * ms;
  const gap = 26, x2 = mw + gap;
  const body = `<g transform="translate(0 ${f(44 - mh / 2 - 6)}) scale(${f(ms)}) translate(${f(-m.vb[0])} ${f(-m.vb[1])})">${m.body}</g>` +
    `<g transform="translate(${f(x2)} 0)">${w.body}</g>`;
  const top = Math.min(44 - mh / 2 - 6, 2), bottom = Math.max(44 + mh / 2 - 6, 82);
  return { body, vb: [-4, top - 4, x2 + 402 + 8, bottom - top + 8] };
}

// ---------------- banner / social preview 1280x640 ----------------
const HERO_BANNER = { ...HERO, title: undefined, subtitle: undefined };

function banner({ dark = false } = {}) {
  const W = 1280, H = 640;
  const bg = dark ? B.night : B.paper, grid = dark ? '#161D2E' : '#E1E6EF';
  const ink = dark ? '#EEF1F8' : B.ink, sub = dark ? '#98A2B9' : '#566077';
  const scene = renderScene(HERO_BANNER, { theme: dark ? 'midnight' : 'blueprint', T: 44, pad: 10 });
  const inner = scene.svg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '')
    .replace(/<rect x="[^"]+" y="[^"]+" width="[^"]+" height="[^"]+" fill="#[0-9a-f]{6}"\/>\s*<g stroke="[^"]+" stroke-width="0.7">[\s\S]*?<\/g>/, ''); // sem fundo/grid próprios
  const vb = scene.svg.match(/viewBox="([^"]+)"/)[1];
  // grid isométrico do banner inteiro
  let g = '';
  for (let k = -40; k <= 40; k++) {
    const x = k * 44 * C * 2;
    g += `<line x1="${f(x)}" y1="0" x2="${f(x + H / Math.tan(Math.PI / 6))}" y2="${H}"/><line x1="${f(x + H / Math.tan(Math.PI / 6))}" y1="0" x2="${f(x)}" y2="${H}"/>`;
  }
  const L = lockup({ dark });
  const lw = 430, ls = lw / L.vb[2];
  const chip = (x, label) => {
    const w = label.length * 8.2 + 28;
    return { w, s: `<rect x="${x}" y="472" width="${f(w)}" height="34" rx="17" fill="${dark ? '#151C2E' : '#FFFFFF'}" stroke="${dark ? '#2B3654' : '#CBD2E0'}"/><text x="${f(x + w / 2)}" y="494" font-family="Inter" font-weight="600" font-size="14" text-anchor="middle" fill="${ink}">${label}</text>` };
  };
  let cx = 80, chips = '';
  for (const label of ['MCP', 'Rust', 'PNG · SVG · GIF', 'Open source']) { const c = chip(cx, label); chips += c.s; cx += c.w + 10; }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
<defs><linearGradient id="fade" x1="0" x2="1"><stop offset="0" stop-color="${bg}" stop-opacity="1"/><stop offset="1" stop-color="${bg}" stop-opacity="0"/></linearGradient></defs>
<rect width="${W}" height="${H}" fill="${bg}"/>
<g stroke="${grid}" stroke-width="1">${g}</g>
<svg x="520" y="70" width="740" height="520" viewBox="${vb}" preserveAspectRatio="xMidYMid meet" font-family="Inter">${inner}</svg>
<rect x="470" y="0" width="160" height="${H}" fill="url(#fade)"/>
<g transform="translate(80 150) scale(${f(ls)}) translate(${f(-L.vb[0])} ${f(-L.vb[1])})">${L.body}</g>
<text x="80" y="345" font-family="Inter" font-weight="700" font-size="36" letter-spacing="-0.6" fill="${ink}">Sua infraestrutura, em diorama.</text>
<text x="80" y="392" font-family="Inter" font-weight="400" font-size="19" fill="${sub}">Diagramas isométricos de system design que a sua IA</text>
<text x="80" y="420" font-family="Inter" font-weight="400" font-size="19" fill="${sub}">desenha via MCP, prontos para README, slides e posts.</text>
${chips}
</svg>`;
}

// ---------------- ícone de app (quadrado arredondado) ----------------
function appIcon({ simple = false } = {}) {
  const m = mark({ simple, dark: true });
  const S = 1024, inset = simple ? 150 : 170, size = S - 2 * inset;
  const k = size / Math.max(m.vb[2], m.vb[3]);
  const ox = inset + (size - m.vb[2] * k) / 2, oy = inset + (size - m.vb[3] * k) / 2 + 10;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${S}" height="${S}" viewBox="0 0 ${S} ${S}">
<defs><linearGradient id="bgi" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1A2240"/><stop offset="1" stop-color="#0C111D"/></linearGradient></defs>
<rect width="${S}" height="${S}" rx="228" fill="url(#bgi)"/>
<g transform="translate(${f(ox)} ${f(oy)}) scale(${f(k)}) translate(${f(-m.vb[0])} ${f(-m.vb[1])})">${m.body}</g>
</svg>`;
}

// ---------------- saída ----------------
const files = {
  'logo/dioramic-mark.svg': svgOf(mark()),
  'logo/dioramic-mark-dark.svg': svgOf(mark({ dark: true })),
  'logo/dioramic-mark-simple.svg': svgOf(mark({ simple: true })),
  'logo/dioramic-wordmark.svg': svgOf(wordmark()),
  'logo/dioramic-wordmark-dark.svg': svgOf(wordmark({ dark: true })),
  'logo/dioramic-logo.svg': svgOf(lockup()),
  'logo/dioramic-logo-dark.svg': svgOf(lockup({ dark: true })),
  'icon/dioramic-icon.svg': appIcon(),
  'icon/dioramic-favicon.svg': svgOf(mark({ simple: true })),
};
for (const [p, s] of Object.entries(files)) writeFileSync(`${OUT}/${p}`, s);
writeFileSync(`${OUT}/logo/dioramic-logo.png`, png(files['logo/dioramic-logo.svg'], 1200));
writeFileSync(`${OUT}/logo/dioramic-logo-dark.png`, png(files['logo/dioramic-logo-dark.svg'], 1200));
writeFileSync(`${OUT}/logo/dioramic-mark.png`, png(files['logo/dioramic-mark.svg'], 512));
writeFileSync(`${OUT}/icon/dioramic-icon-1024.png`, png(files['icon/dioramic-icon.svg'], 1024));
writeFileSync(`${OUT}/icon/dioramic-icon-256.png`, png(appIcon({ simple: true }), 256));
writeFileSync(`${OUT}/icon/dioramic-favicon-32.png`, png(files['icon/dioramic-favicon.svg'], 32));
for (const dark of [false, true]) {
  const s = banner({ dark });
  writeFileSync(`${OUT}/banner/banner-${dark ? 'dark' : 'light'}.png`, png(s, 2560));
}
writeFileSync(`${OUT}/banner/social-preview.png`, png(banner({ dark: true }), 1280));
console.log('ok');
