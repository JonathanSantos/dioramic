import { Resvg } from '@resvg/resvg-js';
import { writeFileSync, mkdirSync } from 'node:fs';
import { renderScene, HERO } from './scene.mjs';
mkdirSync('gallery', { recursive: true });
const FONT = { font: { loadSystemFonts: false, fontFiles: ['400', '500', '600', '700'].map((w) => `../fonts/Inter-${w}.ttf`), defaultFontFamily: 'Inter' } };
const meta = [];
for (const density of ['spaced', 'compact']) for (const rotation of [0, 90, 180, 270]) {
  const spec = { ...HERO, title: `${density} · ${rotation}°`, subtitle: 'dioramic prototype' };
  const r = renderScene(spec, { density, rotation });
  const name = `${density}-${rotation}`;
  writeFileSync(`gallery/${name}.svg`, r.svg);
  writeFileSync(`gallery/${name}.png`, new Resvg(r.svg, { ...FONT, fitTo: { mode: 'zoom', value: 2 } }).render().asPng());
  meta.push({ name, vb: r.vb, arrows: r.arrows, w: r.w, h: r.h });
}
writeFileSync('gallery/meta.json', JSON.stringify(meta));
console.log(meta.map((m) => `${m.name}: ${Math.round(m.w)}x${Math.round(m.h)} (aspecto ${(m.w / m.h).toFixed(2)})`).join('\n'));
