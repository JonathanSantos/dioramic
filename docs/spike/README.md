# Protótipos e geradores (descartáveis)

Scripts Node que validam o pipeline **cena → SVG → resvg → PNG/GIF** antes do código Rust existir.
O raster é o **`resvg`** (Rust) via `@resvg/resvg-js`, o mesmo motor que o Dioramic vai usar nativamente.
A geometria porta 1:1 para Rust e serve de referência executável para o [06](../06-renderizacao.md).
**Não é código de produção.**

| Script | O que faz | Saída |
|---|---|---|
| `spike-resvg.mjs` | Primeiro spike: projeção, primitivas, arestas, PNG e GIF | `out/spike.{svg,png,gif}` |
| `scene.mjs` | Motor de cena: temas `blueprint`/`midnight`, layout `spaced`/`compact`, rotação 0/90/180/270/`auto`, setas com portas visíveis projetadas no chão, colisão gulosa de labels, pacotes com velocidade uniforme, modo SVG animado | (módulo) |
| `lab.mjs` | Laboratório de GIF: resoluções, fps, frames delta, paleta global, WebP, SVG animado | `lab/*.gif`, `lab/results.json` ([resultado](gif-lab-results.json)) |
| `demo.mjs` | Demos do README: GIF 1600 px a 50 fps, PNG e SVG animado, claro e escuro | `demo-out/demo-{light,dark}.*` |
| `brand.mjs` | Identidade visual: marca, wordmark, lockups, ícones e banners | `brand-out/{logo,icon,banner}/` |
| `gallery.mjs` | Mesma cena nas 4 rotações × 2 densidades | `gallery/*.png`, `gallery/meta.json` |
| `arrows-check.mjs` | Mede o ângulo de cada ponta de seta vs. o centro do destino e a tangente da curva (deve dar 0°) | stdout |

## Como rodar

Em uma pasta temporária, fora do repositório, com os scripts e uma pasta irmã `fonts/`:

```bash
npm i @resvg/resvg-js@2.6.2 gifenc@1.0.3 sharp@0.34
```

```bash
node demo.mjs ./demo-out
```

- **Fontes:** os scripts esperam `../fonts/Inter-{400,500,600,700}.ttf` e `../fonts/SpaceGrotesk-{500,700}.ttf` (SIL OFL, do Google Fonts).
  `spike-resvg.mjs` usa a Helvetica Neue do macOS; com `SYSFONTS=1` ele varre as fontes do sistema e reproduz a lentidão do [ADR-008](../02-decisoes.md#adr-008).
- `sharp` só é necessário para a comparação com WebP no `lab.mjs`.
