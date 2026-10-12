# 01 — Visão e escopo

## Problema

Diagramas de arquitetura bonitos (isométricos, estilo Cloudcraft/Isoflow) hoje são feitos **à mão** em editores visuais.
As ferramentas "diagram-as-code" que a IA já usa bem (Mermaid, PlantUML, Graphviz, `mingrammer/diagrams`) geram
diagramas **planos e funcionais, mas não bonitos**. Não existe hoje um servidor MCP focado em diagramas **isométricos**.

Queremos que uma IA (Claude Code, Claude Desktop, Cursor…) consiga, numa conversa:

1. descrever uma arquitetura em um formato simples;
2. receber de volta uma imagem isométrica bonita, pronta para README, slide, post ou doc;
3. **ver** essa imagem, perceber problemas e corrigir sozinha;
4. opcionalmente gerar uma **animação** (fluxo de requisição, build-up, cenário passo a passo).

## Usuários

| Usuário | O que precisa |
|---|---|
| **IA via MCP** (principal): Claude Code, Claude Desktop e VS Code Copilot | Formato compacto e sem ambiguidade, defaults bonitos, erros acionáveis, feedback visual |
| **Dev/arquiteto** | Pedir "desenha a arquitetura desse repo" e receber algo publicável; versionar o spec no git |
| **Quem faz ajuste fino** | Abrir no app desktop, arrastar um nó, trocar um ícone, exportar |

## Princípios de design

1. **A IA descreve semântica, o motor decide geometria.** A IA nunca calcula pixel. No máximo dá *dicas* (`at: [col, row]`, `row`, `align`).
2. **Bonito por padrão.** O primeiro render sem nenhum ajuste já tem que ser "postável". Tema, sombras, espaçamento e tipografia vêm prontos.
3. **Loop fechado.** Todo `render` devolve a imagem (prévia reduzida) e um relatório de lint estruturado, para a IA se autocorrigir.
4. **Erros que ensinam.** `Ícone 'postgress' não existe. Você quis dizer 'postgres'? Use search_icons(query="postgres").`
5. **Determinístico.** Mesmo spec gera os mesmos bytes, o que permite diff no git, cache e testes de imagem (golden files).
6. **O spec é a fonte da verdade.** Um arquivo `*.diorama.yaml` versionável; imagem é artefato derivado.
7. **Local, rápido e sem dependências.** Um binário, sem browser headless, sem Python, sem rede.

## Anatomia da imagem de referência

A referência ([assets/referencia.webp](assets/referencia.webp)) vem do **Isoflow/FossFLOW** (editor isométrico em React, MIT).
Ela define o vocabulário visual que vamos reproduzir:

| Elemento | Como aparece | Nome no spec |
|---|---|---|
| **Grid de fundo** | Linhas isométricas bem claras em toda a tela | `theme.grid` |
| **Zona** | Paralelogramo no chão, cor translúcida, borda tracejada, label em "pílula" escura **deitado no chão** e rotacionado ("Comms Room", "Teller Area") | `zones[]` |
| **Zona aninhada** | Zona dentro de zona (área dos ATMs dentro do Banking Hall) | `zones[].parent` |
| **Nó** | Ícone isométrico 3D (servidor, switch, firewall, nuvem, NAS, AP, workstation, impressora, telefone, ATM) com sombra suave | `nodes[]` |
| **Label de nó** | Caixa branca arredondada **acima** do nó, ligada por uma linha-guia vertical | `nodes[].label` |
| **Aresta** | Curva suave com seta; cor carrega significado (laranja = tronco/uplink, azul = acesso); tracejada para ligação lógica ("Comms room rack") | `edges[]` |
| **Label de aresta** | Pílula escura com texto branco em negrito no meio da curva ("1Gbps Cat6", "PoE") | `edges[].label` |
| **Portas** | Pequenos círculos na base do switch onde as arestas se conectam | automático |

## Escopo (v1)

- Spec declarativo v1 com zonas (inclusive aninhadas), nós, arestas, labels, estilos e temas.
- Layout automático em grid isométrico, com dicas opcionais.
- Saídas: **SVG, PNG**, depois **GIF, SVG animado, APNG e WebP**; **MP4/WebM** opcionais via ffmpeg.
- Servidor MCP local (stdio) + CLI no mesmo binário.
- Catálogo de ícones com busca por nome, sinônimo e categoria.
- App desktop (Tauri) para preview ao vivo e ajustes. Vem **depois** do MCP; ver [09](09-roadmap.md).

## Fora de escopo (por enquanto)

- Editor colaborativo na nuvem e contas de usuário.
- Geração de ícones por IA generativa (imagem); os ícones são vetoriais e determinísticos.
- 3D real (câmera livre, perspectiva). É **isométrico 2D**, por escolha estética.
- Importar diagramas de qualquer formato; só **Isoflow/FossFLOW JSON** está no roadmap, como ponte para edição manual.

## Critérios de sucesso

- Uma IA, sem exemplos no prompt, gera um diagrama de 8 a 15 nós **sem nenhuma sobreposição** em no máximo 2 iterações.
- O exemplo "agência bancária" ([04](04-spec.md#exemplo-completo)) é reproduzido com qualidade visual comparável à referência.
- PNG @2x em menos de 300 ms; GIF de 3 s em menos de 3 s; GIF de README abaixo de 2 MB.
- Zero dependências de runtime além do binário. ffmpeg só para MP4/WebM.
