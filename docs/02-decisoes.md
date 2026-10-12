# 02 — Decisões de arquitetura (ADRs)

Cada decisão registra contexto, alternativas e consequências. Status: **Aceita**, **Proposta** ou **Em aberto**.

---

<a id="adr-001"></a>

## ADR-001 — Linguagem: Rust

**Status:** Aceita (decisão do autor: "MCP em Rust e app desktop em Rust").

### Contexto

O coração da ferramenta é: *spec → layout → cena → SVG → raster → encoders*. A parte mais difícil de fazer bem
em qualquer linguagem é **rasterizar SVG com qualidade** (texto, degradês, blur, marcadores de seta), sem browser.

### Comparação

| Critério | **Rust** | Go | TypeScript/Node | Python |
|---|---|---|---|---|
| Raster de SVG de alta qualidade | ✅ **`resvg` é nativo** (o mesmo motor usado pelo resvg-js e pelo Typst) | ⚠️ Sem renderizador SVG completo em Go puro: `oksvg` não tem texto nem filtros; `resvg` só via CGO | ✅ `resvg-js` (binding do Rust) | ⚠️ `cairosvg` exige cairo nativo |
| SDK MCP oficial | ✅ `rmcp` 3.5 (18M downloads recentes) | ✅ `go-sdk` v1.7 | ✅ referência (1.32) | ✅ FastMCP |
| Layout de grafos | ⚠️ `petgraph` + layout próprio | ⚠️ próprio ou Graphviz via WASM | ✅ `elkjs` | ⚠️ Graphviz nativo |
| Distribuição | ✅ binário único estático | ✅ binário único | ⚠️ precisa de Node (`npx`) | ❌ venv/pip |
| App desktop | ✅ **Tauri 2** (nativo do ecossistema) | ✅ Wails | ⚠️ Electron (pesado) | ❌ |
| Mesmo core no frontend do app | ✅ compila para WASM | ⚠️ WASM grande | ✅ nativo | ❌ |
| Frames em paralelo | ✅ `rayon` | ✅ goroutines | ⚠️ worker threads | ❌ GIL |
| Velocidade de iteração | ⚠️ compilação mais lenta | ✅ | ✅✅ | ✅✅ |

### Sobre Go (avaliado a pedido)

Go **tem estrutura** para MCP (SDK oficial maduro) e para servir de backend. O problema é só o raster:
ou desenhamos a cena duas vezes (SVG para vetor e `tdewolff/canvas` para PNG, que não tem blur), ou usamos `resvg` via CGO,
e aí perdemos a vantagem do binário Go puro. Em Rust esse problema não existe.

### Decisão

**Rust** para core, MCP, CLI e desktop. O `resvg` é a peça mais crítica e é Rust nativo. O app Tauri usa os mesmos crates
(num workspace Cargo próprio, por caminho), e o core compila para WASM se o frontend do app precisar renderizar sem IPC (fora da v1, ver ADR-011).

### Consequências

- Layout automático é **implementação própria** (não há ELK em Rust). Isso é aceitável: nosso layout é específico
  (zonas em grid isométrico), e um layout genérico de grafos não resolveria sozinho.
- Ritmo inicial mais lento que em TS; compensado por binário único e desempenho.
- Pré-requisito: instalar o toolchain (`rustup`), que ainda **não está na máquina** de desenvolvimento.

---

<a id="adr-002"></a>

## ADR-002 — SVG como formato intermediário + `resvg` para raster

**Status:** Aceita (validada no [spike](spike/)).

### Decisão

Toda saída passa por **SVG**: `Scene → SVG (string)`. PNG e frames de animação saem do SVG via `usvg`/`resvg`/`tiny-skia`.

### Por quê

- O SVG vetorial sai de graça, e o app desktop **exibe SVG nativamente** no webview, com zoom nítido.
- Depurável: dá para abrir o SVG no browser e inspecionar.
- Testes de snapshot em texto (diff legível) além de diff de pixels.
- O SVG animado (CSS `@keyframes`) é praticamente o mesmo SVG com uma camada a mais.

### Alternativas descartadas

| Alternativa | Por que não |
|---|---|
| Desenhar direto com `tiny-skia` | Duplicaria o renderer para gerar SVG vetorial |
| Chrome headless (Playwright/`chromiumoxide`) | Cerca de 150 MB de dependência, inicialização lenta, resultado varia entre versões |
| Canvas/GPU (`wgpu`, `vello`) | Overkill para imagens estáticas; fica como opção futura para o preview do desktop |

### Restrições que isso impõe

- Só usar o **subconjunto de SVG suportado pelo resvg**: filtros (`feGaussianBlur` etc.), degradês, máscaras, marcadores e texto.
  Sem `foreignObject`, sem CSS complexo.
- **O resvg ignora animações CSS e SMIL.** Frames raster são gerados avaliando a cena no tempo `t` em Rust
  (ver [07](07-animacao.md)). A camada CSS só existe no export "SVG animado".

---

<a id="adr-003"></a>

## ADR-003 — Spec declarativo; JSON canônico, YAML para arquivos

**Status:** Aceita.

- A IA envia **JSON** nas chamadas MCP: o schema da tool sai automaticamente de `schemars` e o modelo valida enquanto escreve.
- Humanos e git usam **YAML** (`*.diorama.yaml`), mais legível e com comentários.
- Parser YAML: **`serde-saphyr`**. **Não usar `serde_yml`** (aviso de unsoundness RUSTSEC-2025-0068) nem `serde_yaml` (descontinuado).
- Atalhos para economizar tokens: arestas como string (`"fw -> core: 1Gbps trunk"`), ícone inferido do id quando óbvio.
- Detalhes em [04](04-spec.md).

---

<a id="adr-004"></a>

## ADR-004 — Um binário para MCP + CLI; desktop separado; o arquivo é o contrato

**Status:** Aceita.

```
dioramic mcp                       # servidor MCP (stdio), é o que o Claude Code chama
dioramic render x.diorama.yaml -o x.png
dioramic watch  x.diorama.yaml         # re-render a cada mudança
dioramic icons search postgres
dioramic lint   x.diorama.yaml
```

- **MCP e CLI compartilham 100% do core.** O que a IA consegue fazer, o humano consegue pelo terminal, e o contrário também.
- O **app desktop é outro binário** (Tauri) que **observa arquivos `*.diorama.yaml`**. Quando a IA escreve o spec via MCP,
  o app atualiza ao vivo. Não há acoplamento de processo entre os dois. Ver [08](08-app-desktop.md).

---

<a id="adr-005"></a>

## ADR-005 — Projeção isométrica verdadeira (30°)

**Status:** Aceita.

Medindo a referência, as bordas das zonas têm inclinação de cerca de 0,57–0,58, que é `tan 30°`. Então é isometria verdadeira, e não a dimétrica 2:1 (0,5) usada em pixel art.
A dimétrica fica como opção de tema (`projection: dimetric`) para quem quiser linhas mais nítidas em resoluções baixas.

---

<a id="adr-006"></a>

## ADR-006 — Estratégia de ícones em três camadas

**Status:** Aceita. **Ícones de marca (AWS, GCP, Azure, Kubernetes) entram na v1.**

1. **Primitivas procedurais** (caixa, laje, cilindro, rack, nuvem, cúpula, monitor…), parametrizadas por cor. São infinitas variações, sempre
   no estilo do tema e sem questões de licença. O spike já desenha servidor, switch, banco, cache e nuvem assim.
2. **Pack isométrico do Isoflow** (37 ícones, **MIT**): servidor, firewall, roteador, fila, load balancer, laptop, impressora…
3. **Ícones planos projetados no topo de um bloco**: qualquer ícone 2D (AWS, GCP, Azure, Kubernetes, simple-icons) vira isométrico
   aplicando a matriz do plano do chão. É o que o Isoflow faz com mais de 1000 ícones de nuvem, porque só 37 dos seus ícones são realmente isométricos.

Na v1, a camada 3 inclui os packs oficiais de AWS, GCP, Azure e Kubernetes, instalados pelo CLI (ver ADR-007).

Detalhes em [06](06-renderizacao.md#ícones).

---

<a id="adr-007"></a>

## ADR-007 — Open source e licenças

**Status:** Aceita. O projeto é **open source**.

- Projeto: **MIT OR Apache-2.0** (padrão do ecossistema Rust), com `LICENSE-MIT` e `LICENSE-APACHE` na raiz.
- **Evitar dependências GPL/AGPL** que contaminariam o binário: `imagequant` (GPL-3) e `gifski` (AGPL-3).
  Usar `quantette` ou `color_quant` (MIT/Apache) para quantização de GIF.
- Fontes: **Inter** (SIL OFL), embutida no binário.
- Ícones de provedores (AWS/GCP/Azure/Kubernetes): os termos de cada um permitem uso em diagramas de arquitetura, mas **redistribuir dentro do binário**
  ainda não foi validado. Decisão para a v1: `dioramic icons install aws|gcp|azure|k8s` **baixa o pacote oficial direto da fonte do provedor**
  para `~/.local/share/dioramic/icons/`. O projeto não redistribui os arquivos; o MCP só avisa quando um pack não está instalado.
- Marca Dioramic (logo, wordmark): os arquivos ficam no repositório sob a mesma licença, mas o **nome e o logo** não devem ser usados
  para sugerir endosso de forks (nota no README, padrão de projetos open source).

---

<a id="adr-008"></a>

## ADR-008 — Fontes embutidas e banco de fontes carregado uma vez

**Status:** Aceita (lição do spike).

No spike, deixar o renderer **varrer as fontes do sistema a cada frame custou 275 ms/frame**. Com uma fonte fixa, o custo caiu para **43 ms/frame**.

- Embutir Inter (regular, semibold, bold) com `include_bytes!` e montar um `fontdb::Database` **uma vez** (`Arc`), compartilhado entre renders e threads.
- A medição de texto para o layout (largura de labels) usa **a mesma fonte** via `rustybuzz`, então o que o layout mede é o que o resvg desenha.
- Nunca depender de fontes do sistema: isso quebraria o determinismo entre máquinas.

---

<a id="adr-009"></a>

## ADR-009 — Nome: Dioramic

**Status:** Aceita.

- **Dioramic**, de *diorama*: uma cena em miniatura montada sobre uma base. É exatamente o que a ferramenta entrega: a sua arquitetura como uma pequena cena isométrica.
- Disponibilidade verificada em out/2026: crates.io, npm e org `dioramic` no GitHub livres; `dioramic.dev` e `dioramic.io` sem registro (consulta RDAP).
- Nomes de artefatos: binário e crate `dioramic`; crates internos `dioramic-*`; arquivos `*.diorama.yaml`; chave de versão `diorama: 1`; URIs `dioramic://`.
- Identidade visual em [10-marca.md](10-marca.md).

---

<a id="adr-010"></a>

## ADR-010 — Demo do README em GIF de alta qualidade

**Status:** Aceita (medida em [07](07-animacao.md#gif-de-alta-qualidade-e-fluido-medido)).

- **1600 px, 50 fps (delay de 2 cs), loop de 2 s**, paleta global, frames delta: cerca de 1,1 MB com fidelidade de cerca de 52 dB.
- Versões **clara e escura** servidas com `<picture>` + `prefers-color-scheme`.
- O SVG animado (30 KB) fica como formato complementar; para isso, o texto precisa sair como contornos.

---

<a id="adr-011"></a>

## ADR-011 — App desktop 100% Rust: Tauri 2 + Leptos

**Status:** Aceita.

- Backend Tauri 2 em Rust e frontend **Leptos** (CSR, compilado para WASM). Sem TypeScript na UI.
- **Na v1 o core não roda no frontend.** O app é um workspace Cargo separado (`apps/desktop`: `ipc`, `src-tauri`, `ui`); o backend chama
  `dioramic_render::Engine` por **comandos Tauri** (preview em SVG, export, pin, scan/watch) e a UI recebe o SVG pronto.
  Com debounce de 150 ms o preview fica ao vivo sem precisar do core em WASM.
- O `dioramic-core` continua sem I/O e compilando para `wasm32`: levá-lo ao frontend fica para o modo editor (arrastar nós com re-render local).
- Detalhes em [08](08-app-desktop.md).

> Nota (2026-10-09, T-371): a versão anterior deste ADR previa o core em WASM no frontend desde já; o T-203 mostrou que o IPC basta para a fase *Viewer*.

---

<a id="adr-012"></a>

## ADR-012 — Setas terminam em portas visíveis e ficam deitadas no chão

**Status:** Aceita (defeito encontrado e corrigido no protótipo).

- O fim da aresta é uma **porta visível** do destino. Se a borda natural estiver atrás de um nó alto, a porta desliza para o canto lateral visível.
- A ponta **aponta para o centro** do componente, e a curva chega tangente a essa direção.
- O triângulo da ponta é construído no plano e **projetado**, como tudo que está no chão. Não se usa `marker` de SVG.
- Medição antes/depois em [06](06-renderizacao.md#setas-e-portas): erro de até 21,8° → 0,0°, e nenhuma ponta escondida.

---

<a id="adr-013"></a>

## ADR-013 — Rotação de câmera e densidade são opções de vista, não de layout

**Status:** Aceita.

- `view.rotation: 0 | 90 | 180 | 270 | auto` gira o mundo antes da projeção; o spec não muda. `auto` escolhe o ângulo de menor colisão residual (3 a 6 ms no protótipo).
- `layout.density: spaced | compact`: o espaçado (padrão) põe as zonas em linha; o compacto as dobra em bloco serpenteando e reduz os espaçamentos (−58% de área na demo).
- Ambos estão no spec, nos parâmetros de `render_diagram`, no CLI (`--rotate`, `--density`) e na barra do app desktop.
- Detalhes em [06](06-renderizacao.md#rotação-da-câmera) e [04](04-spec.md#view).
