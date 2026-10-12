# 09 — Roadmap

## O que a v1 entregou (0.1.0, out/2026)

Fechada na [T-392](work/tasks/T-392-release-v1.md); evidências na [Definição de pronto](work/README.md#definição-de-pronto-v1).

- **Motor em Rust:** spec JSON/YAML com atalhos de aresta e diagnósticos estáveis; layout automático com zonas aninhadas, `spaced`/`compact`
  e rotação 0/90/180/270/`auto`; setas com portas visíveis (erro 0,00°); colisão de labels que mede guias, badges e réplicas; desvio de
  arestas que atravessam ou correm rentes a nós; 30 ícones procedurais; 4 temas.
- **Saídas:** PNG, SVG (texto em contornos), SVG animado e GIF 1600 px a 50 fps (demo de 787 KB), com presets `flow` e `build`.
- **Ícones de marca:** packs AWS, GCP, Azure e Kubernetes instalados das fontes oficiais (`dioramic icons install`), desenhados planos
  projetados; ícones do projeto em `icons/`; sem o pack, aviso `W_ICON_PACK_MISSING` e ícone embutido.
- **CLI** (`render`, `watch`, `lint`, `icons`, `mcp`, `init`) e **servidor MCP** (4 tools, resources, prompts, só dentro das roots).
- **App desktop** (Tauri 2 + Leptos): árvore, editor YAML com diagnósticos, preview ao vivo, rotação/densidade/tema, Fixar, export e packs de
  ícones recarregados sem reiniciar.
- **Qualidade:** 555 testes no workspace + 148 no app, goldens, galeria com relatório de colisão, clippy `-D warnings` limpo.
- **Pendente de humano:** teste dentro dos 3 clientes MCP e uso da janela do app (ver o topo do [BOARD](work/BOARD.md)).

## Fase 0 — Validação ✅

- [x] Spike: cena → SVG → `resvg` → PNG e GIF animado ([spike/](spike/), [resultado](assets/spike.png))
- [x] Matemática da projeção, primitivas (servidor, switch, banco, cache, nuvem), zonas, labels, arestas curvas, pacotes animados
- [x] Medição: PNG @2x em 130 ms, 43 ms/frame, GIF de 40 frames com 284 KB
- [x] Armadilhas identificadas: fontes do sistema, banding em GIF, labels escondidos, enquadramento
- [x] Laboratório de GIF: 1600 px a 50 fps com 1,1 MB e cerca de 52 dB; frames delta reduzem 10,7× ([07](07-animacao.md#gif-de-alta-qualidade-e-fluido-medido))
- [x] SVG animado verificado rodando dentro de `<img>` (CSS + SMIL); largura do README no GitHub medida (838 px)
- [x] Nome e identidade visual: **Dioramic** ([10-marca.md](10-marca.md)), README do projeto, licenças MIT/Apache-2.0
- [x] **Setas corrigidas** (portas visíveis, ponta para o centro, projetada no chão): erro de até 21,8° → 0,0° ([06](06-renderizacao.md#setas-e-portas))
- [x] **Rotação de câmera** (0/90/180/270/auto) e **densidade** (espaçada/compacta) no protótipo, com colisão gulosa de labels, pílulas e zonas

## Fase 1 — MVP estático via MCP ✅ (v1)

**Entrega:** a IA gera um PNG/SVG bonito pelo MCP.

- [x] Workspace Cargo com os crates do [03](03-arquitetura.md) (+ `dioramic-packs`, T-390)
- [x] `dioramic-spec`: tipos v1, JSON e YAML (`serde-saphyr`), atalho de arestas, JSON Schema via `schemars`
- [x] `normalize` + `validate` com diagnósticos e "você quis dizer"
- [x] **Layout v1:** rank das zonas pelo grafo de arestas entre zonas (origens primeiro, ao longo de `flow`),
      empacotamento em grid dentro de cada zona, respeitando `at`, `arrange` e `size`
- [x] Primitivas procedurais (30 ícones); o pack Isoflow (MIT) ficou para depois
- [x] **Ícones de marca na v1:** ícones planos projetados (`mount: top | stand`) + `dioramic icons install aws|gcp|azure|k8s` + ícones do projeto (`icons/`) (T-390)
- [x] Arestas `curved` com **portas visíveis e pontas projetadas** (regras do [06](06-renderizacao.md#setas-e-portas)); labels com colisão gulosa; `viewBox` automático
- [x] `view.rotation` (incluindo `auto` por pontuação de colisão) e `layout.density` (`spaced`/`compact`)
- [x] Tema `blueprint`, Inter embutida, `fontdb` compartilhado
- [x] `dioramic-render`: SVG e PNG
- [x] `dioramic-mcp`: `render_diagram`, `validate_diagram`, `search_icons`, `get_guide`, instruções do servidor
- [x] `dioramic-cli`: `mcp`, `render` (com `--rotate` e `--density`), `lint`, `icons search`
- [ ] Teste de fumaça nos 3 clientes prioritários (handshake MCP por stdio verificado na T-380; falta rodar dentro de cada cliente): Claude Code, Claude Desktop e VS Code Copilot ([05](05-mcp.md#clientes-prioritários))
- [x] Golden test com o exemplo da agência bancária ([04](04-spec.md#exemplo-completo))

**Critérios de aceite**

- O exemplo da agência renderiza **sem nenhum aviso de sobreposição** e é visualmente comparável à referência.
- Em um eval com 10 pedidos ("desenha a arquitetura de X"), a IA chega a um diagrama sem avisos em **≤ 2 iterações** em pelo menos 8.
- PNG @2x em menos de 300 ms.

## Fase 2 — Qualidade de layout e ícones

- [x] Roteamento `orthogonal` (A* no grid); [ ] fallback automático quando há cruzamento
- [x] Lint completo (`W_EDGE_THROUGH_NODE`, `W_LABEL_OVERLAP` com sugestões concretas)
- [x] Zonas aninhadas, `replicas`, `badge`
- [x] Temas `midnight`, `paper` e `mono`
- [x] Testes de propriedade: nenhum spec válido gera sobreposição de nós

## Fase 3 — Animação

- [x] `Timeline` + preset `flow` (o README mostra); [ ] `story` e `pulse` completos
- [x] Saídas: GIF na receita do [07](07-animacao.md) (1600 px, 50 fps, paleta global, frames delta, recorte por frame), SVG animado com texto em contornos; [ ] APNG
- [x] Filmstrip como prévia para a IA
- [x] Frames em paralelo com `rayon`
- [ ] Opcionais: WebP (`webp-animation`) e MP4/WebM (ffmpeg)

**Aceite:** a demo do README (`flow`, 1600 px, 50 fps, 2 s) gerada pelo binário Rust com menos de 1,5 MB em menos de 2 s,
e um GIF `story` de 3 passos com menos de 3 MB.

## Fase 4 — App desktop

- [x] Tauri 2 + Leptos, 100% Rust ([ADR-011](02-decisoes.md#adr-011))
- [x] Viewer: watch, zoom/pan, diagnósticos, exportar (editor YAML com preview ao vivo)
- [ ] Editor: arrastar com snap, editar labels, seletor de ícones, edição cirúrgica do YAML
- [ ] Deep link `dioramic://` + tool `open_in_app`

## Fase 5 — Ecossistema

- [ ] Import/export JSON do Isoflow/FossFLOW (ponte para edição no navegador)
- [ ] `layers` (andares empilhados), `annotations`, export HTML interativo
- [ ] Distribuição: releases binárias (macOS arm64/x64, Linux, Windows), tap do Homebrew, `cargo install dioramic`
- [ ] `get_selection` (MCP HTTP no app)

## Riscos

| Risco | Impacto | Mitigação |
|---|---|---|
| **Qualidade do auto-layout** | Alto: é o que define se o resultado fica "bonito por padrão" | Começar simples (rank + grid), dicas opcionais, lint, loop visual da IA, crescer com exemplos reais como golden tests |
| Edição do YAML pelo app destruir comentários | Médio | Edições cirúrgicas por span |
| Licença de ícones de provedores | Médio | Packs baixados das fontes oficiais pelo CLI; o projeto não redistribui |
| Preview do webview ≠ PNG do resvg | Baixo | Mesmas fontes via `@font-face`; modo preview raster |
| GIFs grandes demais | Médio | Frames delta, recorte, fps menor no `build`, temas GIF-safe |
| API do `rmcp` mudar (está na 3.x) | Baixo | Fixar versão; camada MCP fina sobre o core |
| Tempo de compilação/curva do Rust | Médio | Crates pequenos; `dioramic watch` para iterar no visual sem recompilar specs |

## Decisões tomadas (out/2026)

| Pergunta | Decisão |
|---|---|
| Nome | **Dioramic** ([ADR-009](02-decisoes.md#adr-009)) |
| Open source | **Sim**, MIT OR Apache-2.0 ([ADR-007](02-decisoes.md#adr-007)) |
| Frontend do desktop | **100% Rust**: Tauri 2 + Leptos ([ADR-011](02-decisoes.md#adr-011)) |
| Ícones de marca na v1 | **Sim**: AWS, GCP, Azure e Kubernetes na Fase 1 |
| Destino da animação | **README**, com GIF de alta qualidade claro e escuro ([ADR-010](02-decisoes.md#adr-010)) |
| README | **Só em português** |
| Clientes MCP prioritários | **Claude Code, Claude Desktop e VS Code Copilot** ([05](05-mcp.md#clientes-prioritários)) |
| Org no GitHub e domínio | **Não por enquanto** (repositório na conta pessoal; sem domínio) |

## Ainda em aberto

Nada bloqueante. Revisitar a org `dioramic` e o domínio `dioramic.dev` perto da primeira release pública.

## Próximo passo concreto

A v1 está fechada no quadro ([work/BOARD.md](work/BOARD.md)). Falta o teste de fumaça dentro dos 3 clientes MCP e na janela do app (humano);
depois, a primeira release binária (Fase 5) e o pack isométrico do Isoflow.
