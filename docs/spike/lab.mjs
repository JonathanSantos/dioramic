// Laboratório de GIF: qualidade x fluidez x tamanho. Descartável.
import { Resvg } from '@resvg/resvg-js';
import gifenc from 'gifenc';
import { writeFileSync, mkdirSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { renderScene, CHECKOUT } from './scene.mjs';
const { GIFEncoder, quantize, applyPalette } = gifenc;

const OUT = './lab';
mkdirSync(OUT, { recursive: true });
const FONTDIR = '../fonts';
const FONT = { font: { loadSystemFonts: false, fontFiles: ['400', '500', '600', '700'].map((w) => `${FONTDIR}/Inter-${w}.ttf`), defaultFontFamily: 'Inter' } };

function frames({ theme, width, fps, seconds }) {
  const n = Math.round(fps * seconds);
  const out = [];
  const t0 = performance.now();
  for (let f = 0; f < n; f++) {
    const { svg } = renderScene(CHECKOUT, { theme, animated: true, t: f / n, loopSec: seconds });
    const r = new Resvg(svg, { ...FONT, fitTo: { mode: 'width', value: width } }).render();
    out.push({ rgba: r.pixels, w: r.width, h: r.height });
  }
  return { list: out, n, delay: Math.round(1000 / fps), renderMs: performance.now() - t0 };
}

function encodeGif(fr, { globalPalette = true, delta = true }) {
  const t0 = performance.now();
  const { w, h } = fr.list[0];
  let palette;
  if (globalPalette) {
    // amostra 4 frames espalhados → paleta única e estável (sem flicker)
    const pick = [0, 0.25, 0.5, 0.75].map((k) => fr.list[Math.floor(k * fr.n)].rgba);
    const all = new Uint8Array(pick.reduce((s, a) => s + a.length, 0));
    let o = 0; for (const a of pick) { all.set(a, o); o += a.length; }
    palette = quantize(all, 255);
  } else palette = quantize(fr.list[0].rgba, 255);
  const TR = palette.length;
  const full = [...palette, [255, 0, 255]];
  const gif = GIFEncoder();
  let prev = null, psnr = 0;
  fr.list.forEach((f, i) => {
    const idx = applyPalette(f.rgba, palette);
    if (i === Math.floor(fr.n / 2)) psnr = PSNR(f.rgba, idx, palette);
    const out = delta && prev ? idx.map((v, k) => (v === prev[k] ? TR : v)) : idx;
    const opts = { delay: fr.delay, dispose: 1 };
    if (i === 0) opts.palette = full;
    if (delta && prev) Object.assign(opts, { transparent: true, transparentIndex: TR });
    gif.writeFrame(out, w, h, opts);
    prev = idx;
  });
  gif.finish();
  return { bytes: gif.bytes(), encMs: performance.now() - t0, psnr, w, h };
}

function PSNR(rgba, idx, pal) {
  let se = 0;
  for (let i = 0; i < idx.length; i++) {
    const c = pal[idx[i]];
    for (let k = 0; k < 3; k++) { const d = rgba[i * 4 + k] - c[k]; se += d * d; }
  }
  const mse = se / (idx.length * 3);
  return 10 * Math.log10((255 * 255) / mse);
}

const variants = [
  { name: 'a-960-20fps', theme: 'blueprint', width: 960, fps: 20, seconds: 2 },
  { name: 'b-1280-33fps', theme: 'blueprint', width: 1280, fps: 33.333, seconds: 1.98 },
  { name: 'c-1600-50fps', theme: 'blueprint', width: 1600, fps: 50, seconds: 2 },
  { name: 'd-1600-50fps-midnight', theme: 'midnight', width: 1600, fps: 50, seconds: 2 },
];
const results = [];
for (const v of variants) {
  const fr = frames(v);
  const g = encodeGif(fr, {});
  writeFileSync(`${OUT}/${v.name}.gif`, g.bytes);
  results.push({ variant: v.name, px: `${g.w}x${g.h}`, frames: fr.n, delayMs: fr.delay, renderMsPerFrame: Math.round(fr.renderMs / fr.n), encodeMs: Math.round(g.encMs), KB: Math.round(g.bytes.length / 1024), psnrDb: +g.psnr.toFixed(1) });
  if (v.name.startsWith('c-')) globalThis.C_FRAMES = fr;
  if (v.name.startsWith('d-')) globalThis.D_FRAMES = fr;
  console.error('ok', v.name);
}

// mesma variante C sem delta e com paleta só do 1º frame, para medir o ganho de cada técnica
const noDelta = encodeGif(globalThis.C_FRAMES, { delta: false });
const firstPal = encodeGif(globalThis.C_FRAMES, { globalPalette: false });
results.push({ variant: 'c sem frames delta', KB: Math.round(noDelta.bytes.length / 1024), psnrDb: +noDelta.psnr.toFixed(1) });
results.push({ variant: 'c paleta só do 1º frame', KB: Math.round(firstPal.bytes.length / 1024), psnrDb: +firstPal.psnr.toFixed(1) });

// SVG animado (CSS + SMIL), claro e escuro
for (const theme of ['blueprint', 'midnight']) {
  const { svg } = renderScene(CHECKOUT, { theme, svgAnim: true, loopSec: 2 });
  writeFileSync(`${OUT}/anim-${theme}.svg`, svg);
  results.push({ variant: `svg animado (${theme})`, KB: +(Buffer.byteLength(svg) / 1024).toFixed(1) });
}

// WebP animado (libwebp via sharp), mesma fonte de frames da variante C
try {
  const sharp = (await import('sharp')).default;
  for (const [tag, fr] of [['blueprint', globalThis.C_FRAMES], ['midnight', globalThis.D_FRAMES]]) {
    const { w, h } = fr.list[0];
    const raw = Buffer.concat(fr.list.map((f) => Buffer.from(f.rgba)));
    const t0 = performance.now();
    const webp = await sharp(raw, { raw: { width: w, height: h * fr.n, channels: 4, pageHeight: h } })
      .webp({ quality: 90, effort: 4, loop: 0, delay: Array(fr.n).fill(fr.delay) }).toBuffer();
    writeFileSync(`${OUT}/e-1600-50fps-${tag}.webp`, webp);
    results.push({ variant: `webp 1600 50fps q90 (${tag})`, encodeMs: Math.round(performance.now() - t0), KB: Math.round(webp.length / 1024) });
  }
} catch (e) { results.push({ variant: 'webp', erro: String(e).slice(0, 80) }); }

console.table(results);
writeFileSync(`${OUT}/results.json`, JSON.stringify(results, null, 2));
