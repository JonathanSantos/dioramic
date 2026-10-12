# Dioramic — documentação de design

> Ferramenta local para **IAs desenharem diagramas de system design isométricos bonitos** via MCP,
> exportando PNG, SVG e GIF animado. Inclui um app desktop (100% Rust) para preview e ajuste fino.

![demo](assets/demo-light.gif)

*Demo gerada pelo binário da v1: `dioramic render examples/checkout.diorama.yaml -o demo-light.gif` (1600 px, 50 fps, 835 KB).*

**Estado:** a v1 (0.1.0) existe: motor, CLI (`render`, `watch`, `lint`, `icons`, `mcp`, `init`), servidor MCP e app desktop.
Os docs abaixo são o design; o processo de construção (quadro, tarefas, contratos entre crates e revisões) está em [work/](work/README.md).

## TL;DR das decisões

| Tema | Decisão | Doc |
|---|---|---|
| Nome | **Dioramic**: a sua infraestrutura como um diorama | [ADR-009](02-decisoes.md#adr-009), [10](10-marca.md) |
| Licença | **Open source**, MIT OR Apache-2.0 | [ADR-007](02-decisoes.md#adr-007) |
| Linguagem | **Rust** para tudo: core, MCP, CLI e app desktop | [ADR-001](02-decisoes.md#adr-001) |
| Formato intermediário | A cena vira **SVG**; PNG e GIF saem do SVG via **`resvg`** (Rust puro, sem browser) | [ADR-002](02-decisoes.md#adr-002) |
| Interface da IA | **Formato Diorama** declarativo (`*.diorama.yaml`): a IA descreve *o quê*, o motor decide *onde* | [04](04-spec.md) |
| MCP | **Poucas tools** (`render_diagram`, `validate_diagram`, `search_icons`, `get_guide`); o render devolve a **imagem** para a IA se autocorrigir | [05](05-mcp.md) |
| Projeção | **Isométrica verdadeira (30°)**, igual à referência | [06](06-renderizacao.md) |
| Ícones | **v1: 30 primitivas procedurais**; depois, pack MIT do Isoflow e **AWS, GCP, Azure e Kubernetes** (ícones planos projetados) | [ADR-006](02-decisoes.md#adr-006) |
| Animação | Cena em função do tempo; GIF **1600 px a 50 fps** para README (claro e escuro), SVG animado como complemento | [07](07-animacao.md), [ADR-010](02-decisoes.md#adr-010) |
| Setas | Terminam em **porta visível**, apontam para o centro e ficam **deitadas no chão** | [ADR-012](02-decisoes.md#adr-012) |
| Vista | **Rotação** 0/90/180/270/`auto` e **densidade** espaçada/compacta, sem mudar o spec | [ADR-013](02-decisoes.md#adr-013) |
| Clientes MCP | **Claude Code, Claude Desktop e VS Code Copilot** | [05](05-mcp.md#clientes-prioritários) |
| Desktop | **Tauri 2 + Leptos**, 100% Rust; o arquivo de spec é o contrato com o MCP | [08](08-app-desktop.md), [ADR-011](02-decisoes.md#adr-011) |
| Distribuição | **Um binário** `dioramic` (`render`, `watch`, `lint`, `icons`, `mcp`, `init`) + app desktop | [03](03-arquitetura.md) |

## Índice

1. [Visão e escopo](01-visao.md): problema, princípios, anatomia da imagem de referência
2. [Decisões (ADRs)](02-decisoes.md): Rust vs Go vs Node, SVG + resvg, licenças, nome, GIF, desktop
3. [Arquitetura](03-arquitetura.md): crates, pipeline, desempenho, determinismo
4. [Formato Diorama v1](04-spec.md): o que a IA escreve, com exemplo completo
5. [Servidor MCP](05-mcp.md): tools, contratos, loop de feedback visual
6. [Renderização isométrica](06-renderizacao.md): matemática, primitivas, arestas, labels, temas, ícones
7. [Animação e formatos](07-animacao.md): efeitos, timeline, **laboratório de GIF**, orçamentos
8. [App desktop](08-app-desktop.md): Tauri + Leptos, preview ao vivo, edição visual
9. [Roadmap](09-roadmap.md): fases, critérios de aceite, riscos, decisões tomadas
10. [Marca](10-marca.md): nome, voz, logo, cores, tipografia, estilo dos diagramas

## Números medidos da v1 (Mac M2, release)

Medidos com o binário `dioramic` em 2026-10-09, numa máquina com outros builds rodando (load ~60): por isso os tempos são de
**CPU** (`user`), não de relógio. Os números do protótipo (Fase 0) ficam no [roadmap](09-roadmap.md) e no [07](07-animacao.md).

| Medida | Valor |
|---|---|
| PNG @2x do checkout (2170×1526), processo inteiro | **0,28 s** de CPU (fontes embutidas carregadas uma vez) |
| PNG @2x do bank-branch (17 nós, `rotation: auto`) | **0,73 s** de CPU |
| SVG estático / SVG animado do checkout | **≈ 20 ms** de relógio · 100 KB / 104 KB (texto em contornos) |
| GIF do README: 1600×1178, 50 fps, 100 frames, 2 s | **835 KB** claro / **885 KB** escuro (`midnight`), T-413 (T-396: 791/843 KB a 1600×1125); ~8,5 s de CPU cada, os dois juntos em 4,1 s de relógio com frames em paralelo (T-392) |
| `dioramic lint` do bank-branch | **≈ 60 ms** de relógio |
| `rotation: auto` (4 cenas, só geometria), bank-branch | **≈ 104 ms** (melhor de 9, T-396; era ≈ 19 ms antes da T-394; o teste de tempo em release, orçamento 20 ms, falha — [T-397](work/tasks/T-397-tempo-da-rotacao-auto.md)) |
| Colisão residual de labels, vista `auto` espaçada | **0 px²** nos 7 exemplos, sem label atribuído ao item errado (T-396; score com guias, badges e réplicas da T-381 e atribuição da T-394) |
| Colisão residual, vista `auto` compacta | **≤ 187 px²** (bank-branch a 270°; ml-pipeline 138, aws-serverless 40, kubernetes 3; os outros 0). Subiu porque o score passou a cobrar atribuição errada (T-394): antes cada uma dessas vistas escondia um label mal lido |
| Arestas atravessando nós (`W_EDGE_THROUGH_NODE`), vistas `auto` | **0** |
| Compacto vs espaçado (checkout) | 776×610 vs 1085×763 px: **−43%** de área |
| Catálogo de ícones | **30** procedurais em 11 categorias |
| Testes do workspace | **555** ok, 6 ignorados (`cargo test --workspace`, T-396) |
| Largura máxima do README no GitHub | **838 px** (por isso o GIF tem 1600 px: nítido em retina) |

Números de colisão: handoff da [T-370](work/tasks/T-370-integracao-motor.md); valores por exemplo e rotação saem de `cargo xtask gallery`
(`target/gallery/report.json`).

A referência visual original fica em [assets/referencia.webp](assets/referencia.webp); o primeiro spike, em [assets/spike.png](assets/spike.png).
