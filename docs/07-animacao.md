# 07 — Animação e formatos

![demo animada](assets/demo-light.gif)

*GIF de alta qualidade: 1600×1178, 50 fps, 100 frames, loop perfeito, 835 KB (escuro: 885 KB, em [assets/demo-dark.gif](assets/demo-dark.gif)). Setas já com a correção do [06](06-renderizacao.md#setas-e-portas).*

## Modelo

A cena é uma **função do tempo**: `scene(t)`. Animar é avaliar a cena em vários instantes.

- **Raster** (GIF, APNG, WebP, MP4): o Rust avalia `scene(t)` para cada frame, emite SVG estático, o `resvg` rasteriza e o encoder junta tudo.
  Os frames são independentes, então rodam em paralelo (`rayon`).
- **SVG animado:** emite **uma** cena com animações declarativas (CSS `@keyframes` para tracejado e opacidade, SMIL `<animateMotion>` para pacotes).
  O arquivo fica com poucos KB e resolução infinita.

Internamente, o preset vira um `AnimationPlan` (`dioramic-core::animation`, contrato [C-005](work/contracts/C-005-scene-svg.md)) com funções puras
de `t` por aresta, nó e zona (tracejado, pacote, opacidade, queda do `build`). As duas saídas (raster e CSS/SMIL)
são geradas **do mesmo plano**, então o GIF e o SVG animado ficam idênticos.

**Na v1:** `flow` e `build` estão implementados; `story` e `pulse` ainda caem no `flow`.

## Presets

| Preset | Loop | O que acontece | Melhor para |
|---|---|---|---|
| `flow` | sim | Tracejado andando + pacotes nas arestas com `flow: true` (ou todas) | README, "o sistema está vivo" |
| `build` | não (segura no fim) | Zonas surgem → nós "caem" com bounce, por ordem de camada do layout → arestas se desenham → labels aparecem | Abertura de apresentação, posts |
| `story` | opcional | Passo a passo: destaca um caminho, escurece o resto, pacote percorre o caminho, legenda embaixo, número em cada passo | **Explicar system design** (fluxo de requisição, falha, failover) |
| `pulse` | sim | Anel pulsando em nós marcados (`alert: true`) | Incidente, hotspot, gargalo |
| `camera` (v2) | — | Pan/zoom animando o `viewBox` | Diagramas grandes |

### Spec

```yaml
animation:
  preset: story
  fps: 20
  loop: true
  steps:
    - { path: [users, lb, api1, db], caption: "1. Checkout chega ao LB e é gravado no Postgres", duration: 2.5s }
    - { edges: [api1->cache],        caption: "2. Carrinho é lido do Redis" }
    - { nodes: [db], effect: pulse,  caption: "3. Primary sobrecarregado" }
```

- `path` é açúcar sintático: uma sequência de nós vira as arestas consecutivas.
- `icons: false` desliga o movimento dos ícones (padrão `true`, ver [Ícones animados](#ícones-animados)).
- Sem `duration`, cada passo dura 2 s; `flow` dura 2 s por loop.

## Detalhes que fazem a diferença

- **Loop sem emenda:** o tracejado tem período `P`, e em um loop o offset anda **um número inteiro de períodos** (no spike, 2 × 16 px).
  Os pacotes usam `t mod 1`, com fase escalonada por aresta, e aparecem/somem com um *fade* de 8 % do trajeto em cada ponta:
  sem isso o pacote "salta" do fim para o começo na emenda (no checkout, a emenda passou de ~2× para **1,06×** a mudança média entre frames).
- **Velocidade uniforme:** a Bézier não é parametrizada por comprimento, então um pacote em `t` linear acelera nas curvas.
  Usar uma tabela de comprimento de arco (LUT) para converter distância em `t`.
- **Easing:** `build` usa `easeOutBack` (bounce leve) nos nós; o resto usa `easeInOutCubic`.
- **Ordem do `build`:** segue o *rank* do layout (origens primeiro), então a animação conta a história do fluxo.

## Ícones animados

![ícones animados](assets/icones-animados.gif)

*1200×712, 50 fps, 100 frames, 366 KB. Gerado pelo próprio `dioramic` (T-402).*

Com animação, os ícones procedurais ganham um movimento leve, sempre em **ciclos inteiros por loop** (sem emenda), e com amplitude pequena:

| Ícone | Movimento |
|---|---|
| `server`, `rack`, `nas` | LEDs piscando em sequência (escurecem para 30 % por um instante) |
| `switch`, `loadbalancer` | Luzes de atividade acendendo sobre as portas (em ordem no LB, espalhadas no switch); LEDs do switch alternando |
| `router` | Setas de saída e de entrada "respirando" alternadas |
| `access-point` | Onda saindo do domo: os arcos acendem de dentro para fora |
| `database`, `cache` | Anel se abrindo e sumindo no topo (escrita); raio do cache acende |
| `queue` | Mensagens andando sobre o tubo, com *fade* nas pontas (como os pacotes) |
| `monitoring` | Barras do gráfico subindo e descendo em onda; ponto do gráfico pisca |
| `k8s-cluster` | Hélice girando devagar: um raio (360°/7) por ciclo, então o loop fecha certinho |
| `function` | O λ acende de vez em quando |
| `cloud` | Deriva lateral de ±1,6 px |
| `cdn`, `voip-phone`, `printer` | LEDs/pontos piscando |

Ícones de marca e os demais procedurais ficam parados. Com `replicas`, só a cópia da frente se move.

**Como funciona** (`dioramic-core::icons::motion`, contrato [C-003](work/contracts/C-003-icons.md)): o corpo do nó é desenhado uma vez,
com cada parte móvel dentro de um *gancho* `<g data-icon-motion="…">` que descreve o movimento (piscar, anel, giro no plano do chão,
translação). O escritor de SVG resolve os ganchos por modo: **estático** remove os ganchos e as partes que só existem animadas (o SVG fica
idêntico byte a byte ao sem animação), **frame em `t`** escreve opacidade e `transform` daquele instante, e o **SVG animado** gera SMIL
(`<animate>`, `<animateTransform>`) a partir dos mesmos *keyframes* lineares por partes. Período natural de 1 s (luzes) ou 2 s
(anéis, giro, deriva); o número de ciclos é `round(duração / período)`, no mínimo 1.

Custo no GIF do checkout (1600 px, 50 fps, 2 s; `gif_quality` em release): blueprint **642 → 833 KB**, midnight **705 → 887 KB**,
emenda 0 px, sem cintilação, PSNR 49,0 / 49,6 dB.

## Formatos

**Na v1** saem PNG, SVG, SVG animado e GIF. APNG, WebP e MP4/WebM continuam no plano: o MCP responde "not supported yet".

| Formato | Como gerar | Prós | Contras | Uso ideal |
|---|---|---|---|---|
| **SVG animado** | CSS + SMIL | Poucos KB, nítido em qualquer zoom | Várias plataformas (Slack, LinkedIn) não exibem SVG | README do GitHub, sites, docs |
| **GIF** | crate `gif` + `quantette` | Funciona em **todo lugar** | 256 cores, arquivos maiores | README, Slack, issues, PRs |
| **APNG** | crate `png` (suporta APNG) | Cor cheia + transparência | Suporte irregular fora de browsers | Sites, docs |
| **WebP animado** | `webp-animation` (feature opcional, libwebp) | Cor cheia | Dependência C; **ficou maior que o GIF** nesta arte (medido abaixo) | Fotos e degradês |
| **MP4 (H.264) / WebM** | frames RGBA via pipe para **ffmpeg** | Menor arquivo, ideal para redes sociais | Exige ffmpeg instalado | LinkedIn, X, slides |
| HTML interativo (v2) | SVG + JS embutido | Hover, play/pause por passo | — | Apresentar ao vivo |

### GIF de alta qualidade e fluido (medido)

Laboratório do spike em [spike/lab.mjs](spike/lab.mjs) (resultados em [spike/gif-lab-results.json](spike/gif-lab-results.json)).
Mesma cena, mesmo motor (`resvg`), variando resolução, fps e técnicas de encode:

| Variante | Resolução | fps (delay) | Frames | Tamanho | Fidelidade (PSNR) |
|---|---|---|---|---|---|
| Spike original | 960×708 | 20 (5 cs) | 40 | 263 KB | 48,5 dB |
| Intermediária | 1280×944 | 33 (3 cs) | 66 | 561 KB | 51,1 dB |
| **Alta qualidade** | **1600×1180** | **50 (2 cs)** | **100** | **1,1 MB** | **51,8 dB** |
| Alta qualidade, tema `midnight` | 1600×1180 | 50 (2 cs) | 100 | 1,06 MB | 49,8 dB |
| Alta qualidade **sem frames delta** | 1600×1180 | 50 | 100 | **12 MB** | 51,8 dB |
| WebP animado q90 (libwebp) | 1600×1180 | 50 | 100 | 1,37 MB | — |
| SVG animado (CSS + SMIL) | vetorial | do browser | — | **30 KB** | — |

> PSNR acima de 40 dB já é visualmente indistinguível da fonte. A tabela acima é do spike (Node + gifenc); os números do Rust estão abaixo.

**Receita adotada** (implementada em `dioramic-render::gif`, contrato [C-006](work/contracts/C-006-render-engine.md)):

1. **1600 px de largura**: a coluna do README no GitHub tem **no máximo 838 px** (medido em out/2026 com viewports de 1440 e 1920 px).
   Em telas retina isso dá 1676 px físicos, então o GIF fica praticamente 1:1, sem borrão de ampliação.
2. **50 fps (delay de 2 cs)** é o máximo que os browsers respeitam. Delays de 0–1 cs são forçados para 10 cs, e a animação fica **mais lenta**.
3. **Loop de 2 s com 100 frames**: os pacotes andam 3 a 6 px por frame, o que dá movimento contínuo. A 20 fps eram saltos de 8 a 15 px, visivelmente "travados".
4. **Velocidade uniforme:** posição do pacote por **comprimento de arco** (LUT), não pelo `t` da Bézier.
5. **Paleta global** de 255 cores + 1 índice transparente, de 4 frames de amostra (k-means em Oklab). **63 entradas ficam reservadas para
   rampas suaves** (sombras desfocadas, degradês de cilindro/nuvem), que o k-means ponderado por área deixaria quase sem cor.
6. **Dithering Bayer 4×4 só nas rampas**, entre a entrada mais próxima e uma parceira próxima e no mesmo eixo de cor. Bordas antialiased e
   chapados não são tocados; o limiar depende só de (x, y), então um pixel parado tem o mesmo índice em todo frame (**zero cintilação**).
7. **Frames delta pela cor exibida** (`dispose = keep`): o encoder guarda o que está na tela e só repinta um pixel quando a cor exibida não é
   intercambiável com a desejada (≤ 0,011 Oklab ≈ 3 níveis). Sem deriva, e recupera o tamanho que a paleta mais fina custaria.
8. **Recorte** de cada frame ao retângulo que mudou (`left/top` do crate `gif`); frame sem mudança = 1 pixel transparente; frame 0 cheio.
9. **Temas GIF-safe:** nada de degradês grandes (vinheta desligada no GIF).

Medido no `checkout` (1600×1125, 100 frames, release; `cargo test -p dioramic-render --release --test gif_quality -- --ignored --nocapture`):

| Tema | Tamanho | PSNR médio | Maior "degrau" na sombra | Cintilação |
|---|---|---|---|---|
| blueprint, antes (só k-means + delta + recorte) | 679 KB | 50,7 dB | 4,14 níveis | — |
| **blueprint, receita atual** | **807 KB** | **49,3 dB** | **1,47** | 0 px |
| midnight, antes | 801 KB | 51,5 dB | 3,39 | — |
| **midnight, receita atual** | **872 KB** | **49,9 dB** | **1,92** | 0 px |

O PSNR cai um pouco porque o dither troca anéis visíveis por grão fino; o critério que importa é o degrau (≤ 3 níveis) e a cintilação (0).
O recorte por frame tirou ~30 % em relação ao spike (gifenc, sem recorte).

**Claro e escuro no GitHub:** o README usa `<picture>` com `prefers-color-scheme`, servindo `demo-light.gif` ou `demo-dark.gif` conforme o tema de quem lê.

**WebP perdeu para o GIF** neste tipo de arte (cores chapadas e pouca mudança entre frames), porque os frames delta do GIF são extremamente eficientes.

**SVG animado** é o mais leve e nítido (30 KB, anima nativamente no browser) e foi **verificado rodando dentro de `<img>`**, que é como o GitHub exibe imagens.
Ressalva encontrada: sem a fonte instalada, o texto cai para uma fonte genérica. Por isso o export SVG **emite o texto como contornos**
(glifos da Inter embutida, via `rustybuzz` + `ttf-parser`), e não como `<text>` — inclusive pílulas `×N` e badges.

### MP4/WebM via ffmpeg

```bash
ffmpeg -f rawvideo -pix_fmt rgba -s 1920x1080 -r 25 -i - -c:v libx264 -pix_fmt yuv420p -crf 18 -movflags +faststart out.mp4
```

- Largura e altura **pares** (exigência do `yuv420p`).
- O binário detecta o ffmpeg no `PATH`. Se não achar, o erro diz como instalar (`brew install ffmpeg`) e sugere GIF/WebP.

## Orçamentos

| Destino | Formato | Tamanho | Duração | Meta de arquivo |
|---|---|---|---|---|
| README | GIF (claro + escuro) | 1600 px de largura, 50 fps | 2–4 s, loop | < 2 MB por GIF |
| Slack / PR | GIF | 800–960 px | 2–4 s | < 3 MB |
| LinkedIn / X | MP4 | 1080×1080 ou 1920×1080 | 6–15 s | < 10 MB |
| Slides | MP4 ou APNG | 1920 px | livre | — |

## Prévia para a IA

O modelo não "assiste" GIF. Para formatos animados, `render_diagram` devolve um **filmstrip** com 4 quadros-chave
(no `story`: um por passo). Validado no spike: 3 quadros lado a lado mostram claramente os pacotes andando.
