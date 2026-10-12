import { renderScene, HERO } from './scene.mjs';
const deg = (v) => (Math.atan2(v[1], v[0]) * 180) / Math.PI;
const norm = (a) => ((a + 540) % 360) - 180;
for (const [density, rotation] of [['spaced', 0], ['spaced', 90], ['compact', 0], ['compact', 180]]) {
  const r = renderScene(HERO, { density, rotation });
  const rows = r.arrows.map((a) => {
    const head = deg([a.tip[0] - a.mid[0], a.tip[1] - a.mid[1]]);
    const toC = deg([a.center[0] - a.tip[0], a.center[1] - a.tip[1]]);
    const curve = deg(a.curveEnd);
    return `${a.edge.padEnd(11)} ponta→centro ${norm(toC - head).toFixed(1).padStart(5)}°  curva→ponta ${norm(curve - head).toFixed(1).padStart(5)}°`;
  });
  console.log(`\n[${density}, ${rotation}°]\n` + rows.join('\n'));
}
