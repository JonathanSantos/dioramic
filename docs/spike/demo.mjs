// Gera as demos do README: GIF HQ (1600px, 50fps, loop 2s) claro/escuro + PNG estático + SVG animado.
import { Resvg } from '@resvg/resvg-js';
import gifenc from 'gifenc';
import { writeFileSync, mkdirSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { renderScene, HERO } from './scene.mjs';
const { GIFEncoder, quantize, applyPalette } = gifenc;

const OUT = process.argv[2] ?? './demo-out';
mkdirSync(OUT, { recursive: true });
const FONT = { font: { loadSystemFonts: false, fontFiles: ['400', '500', '600', '700'].map((w) => `../fonts/Inter-${w}.ttf`), defaultFontFamily: 'Inter' } };

const SPEC = HERO; // layout automático, densidade 'spaced', rotação 0°

const WIDTH = 1600, FPS = 50, SECONDS = 2, N = FPS * SECONDS, DELAY = 1000 / FPS;

for (const theme of ['blueprint', 'midnight']) {
  const tag = theme === 'blueprint' ? 'light' : 'dark';
  // PNG estático @2x
  const st = renderScene(SPEC, { theme });
  writeFileSync(`${OUT}/demo-${tag}.png`, new Resvg(st.svg, { ...FONT, fitTo: { mode: 'width', value: WIDTH } }).render().asPng());
  // SVG animado (CSS + SMIL)
  writeFileSync(`${OUT}/demo-${tag}.svg`, renderScene(SPEC, { theme, svgAnim: true, loopSec: SECONDS }).svg);
  // GIF: frames → paleta global (amostras) → frames delta com índice transparente
  const t0 = performance.now();
  const frames = [];
  for (let f = 0; f < N; f++) {
    const { svg } = renderScene(SPEC, { theme, animated: true, t: f / N, loopSec: SECONDS });
    const r = new Resvg(svg, { ...FONT, fitTo: { mode: 'width', value: WIDTH } }).render();
    frames.push({ rgba: r.pixels, w: r.width, h: r.height });
  }
  const { w, h } = frames[0];
  const sample = [0, 0.25, 0.5, 0.75].map((k) => frames[Math.floor(k * N)].rgba);
  const all = new Uint8Array(sample.reduce((s, a) => s + a.length, 0));
  let o = 0; for (const a of sample) { all.set(a, o); o += a.length; }
  const palette = quantize(all, 255), TR = palette.length;
  const gif = GIFEncoder();
  let prev = null;
  frames.forEach((fr, i) => {
    const idx = applyPalette(fr.rgba, palette);
    const out = prev ? idx.map((v, k) => (v === prev[k] ? TR : v)) : idx;
    const opts = { delay: DELAY, dispose: 1 };
    if (i === 0) opts.palette = [...palette, [255, 0, 255]];
    else Object.assign(opts, { transparent: true, transparentIndex: TR });
    gif.writeFrame(out, w, h, opts);
    prev = idx;
  });
  gif.finish();
  writeFileSync(`${OUT}/demo-${tag}.gif`, gif.bytes());
  console.log(tag, `${w}x${h}`, N, 'frames', Math.round(gif.bytes().length / 1024), 'KB', Math.round(performance.now() - t0), 'ms');
}
