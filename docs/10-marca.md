# 10 — Marca e identidade visual

<p align="center">
  <img src="../brand/logo/dioramic-logo.png" alt="Dioramic" width="520">
</p>

## Nome

**Dioramic** vem de *diorama*: uma cena em miniatura montada sobre uma base, como as de museu ou as maquetes de ferromodelismo.
É o que a ferramenta entrega: **a sua arquitetura como uma pequena cena isométrica**, com servidores, bancos e cabos com pacotes andando.

- **Grafia:** "Dioramic" em texto corrido; `dioramic` (minúsculo) no wordmark, no CLI, no crate e em URLs.
- **Pronúncia:** *di-o-RÁ-mic*.
- **Artefatos:** binário `dioramic`, crates `dioramic-*`, arquivos `*.diorama.yaml`, URIs `dioramic://` ([ADR-009](02-decisoes.md#adr-009)).

## Frases

| Uso | Frase |
|---|---|
| **Tagline principal** | Sua infraestrutura, em diorama. |
| Descritor (README, bio) | Diagramas isométricos de system design que a sua IA desenha via MCP. |
| Curta (posts, release notes) | Do prompt ao diorama. |
| Descrição do repositório (GitHub, até 350 caracteres) | Diagramas isométricos bonitos de system design, desenhados pela sua IA via MCP. PNG, SVG e GIF animado. Rust, local e open source. |

## Voz

- **Preciso como engenheiro, caprichoso como maquetista.** Mostra o resultado antes de explicar.
- Frases curtas, verbos concretos ("desenha", "anima", "exporta"). Números quando houver ("1,1 MB a 50 fps").
- Sem hype ("revolucionário", "mágico"). O diagrama bonito fala por si.
- Humor leve e de miniatura é bem-vindo ("seu cluster em escala 1:87"), mas nunca na documentação técnica.

## Logo

| Peça | Arquivo | Quando usar |
|---|---|---|
| Lockup (marca + nome) | [logo/dioramic-logo.svg](../brand/logo/dioramic-logo.svg) · [PNG](../brand/logo/dioramic-logo.png) | Fundos claros: topo do README, site, slides |
| Lockup escuro | [logo/dioramic-logo-dark.svg](../brand/logo/dioramic-logo-dark.svg) · [PNG](../brand/logo/dioramic-logo-dark.png) | Fundos escuros |
| Marca | [logo/dioramic-mark.svg](../brand/logo/dioramic-mark.svg) | Avatar, cantos, quando o nome já aparece ao lado |
| Marca simplificada | [logo/dioramic-mark-simple.svg](../brand/logo/dioramic-mark-simple.svg) | Abaixo de 48 px (favicon, tray) |
| Wordmark | [logo/dioramic-wordmark.svg](../brand/logo/dioramic-wordmark.svg) · [escuro](../brand/logo/dioramic-wordmark-dark.svg) | Só o nome |
| Ícone de app | [icon/dioramic-icon-1024.png](../brand/icon/dioramic-icon-1024.png) · [SVG](../brand/icon/dioramic-icon.svg) | App desktop, avatar da org no GitHub |
| Favicon | [icon/dioramic-favicon.svg](../brand/icon/dioramic-favicon.svg) · [32 px](../brand/icon/dioramic-favicon-32.png) | Site de docs |
| Banner / social preview | [banner/](../brand/banner/) | Topo do README (claro/escuro) e *Settings → Social preview* do GitHub (1280×640) |

### Anatomia

- **A marca é um diorama:** a **base índigo** é o palco (o grid isométrico), a **torre** é computação, o **cilindro** é dados,
  e o **cabo âmbar com um pacote** é o fluxo entre eles. São os três elementos de qualquer system design.
- **O wordmark** usa Space Grotesk Bold em minúsculas, com os pingos dos dois "i" trocados por **cubos isométricos**:
  o índigo (componente) e o âmbar (fluxo). É a assinatura da marca em tamanho pequeno.

### Regras

- **Respiro:** em volta do lockup, deixar no mínimo a altura do cubo do "i" multiplicada por 2.
- **Tamanho mínimo:** lockup com 120 px de largura; marca completa com 48 px; abaixo disso, a marca simplificada.
- **Não fazer:** rotacionar, distorcer, trocar as cores dos cubos, aplicar sombra ou degradê no wordmark, recriar o wordmark com outra fonte,
  ou colocar o lockup claro sobre fundo escuro (use a versão `-dark`).

## Cores

| Token | Hex | Uso |
|---|---|---|
| **Índigo** | `#3E50C8` | Cor principal: base da marca, cubo do primeiro "i", links, arestas `secondary` |
| Índigo profundo | `#2C3A9E` | Face sombreada do índigo |
| Índigo claro | `#6F86F5` | Índigo sobre fundo escuro |
| **Âmbar** | `#E8833A` | Acento: fluxo, cubo do segundo "i", arestas `primary`, destaques |
| Âmbar claro / profundo | `#F5A25A` / `#C2621F` | Faces do cubo âmbar |
| **Tinta** | `#1C2130` | Texto, contornos e wordmark no tema claro |
| **Papel** | `#F2F4F8` | Fundo do tema claro (`blueprint`) |
| **Noite** | `#0C111D` | Fundo do tema escuro (`midnight`) |
| Ardósia | `#56607A` | Servidores, elementos neutros |
| Verde-água | `#2C9A8C` | Dados (cilindro da marca, zona `teal`) |
| Menta LED | `#7EF0A8` | Detalhes "ligados" (LEDs) |
| Névoa | `#DDE2EC` | Grid, bordas suaves |

O índigo e o âmbar vêm da referência original (arestas azuis de acesso e laranja de tronco) e viraram as cores da marca.

## Tipografia

| Fonte | Uso | Licença |
|---|---|---|
| **Space Grotesk** (Bold 700, Medium 500) | Wordmark e títulos de marketing | SIL OFL |
| **Inter** (400, 500, 600, 700) | Interface, docs, **labels dos diagramas** (embutida no binário, [ADR-008](02-decisoes.md#adr-008)) | SIL OFL |

## Estilo dos diagramas

A identidade visual do produto **é** o render padrão. Toda arte de marketing deve sair do próprio Dioramic (ou do [gerador do spike](spike/brand.mjs)) e seguir:

- Isometria verdadeira de 30°, com grid de fundo sutil.
- Contorno escuro de cerca de 1 px em todos os sólidos; faces com sombreamento 1,0 / 0,8 / 0,62 (luz do alto à esquerda).
- Sombras suaves e desfocadas sob os nós; nada de sombras duras.
- Labels de nó em caixa branca arredondada com linha-guia vertical; labels de aresta em pílula escura.
- Zonas translúcidas com borda tracejada e nome deitado no chão.
- Dois temas oficiais: `blueprint` (claro) e `midnight` (escuro). **Toda imagem pública sai nas duas versões.**

## Regenerar os arquivos

Os assets de [brand/](../brand/) são gerados por código: [spike/brand.mjs](spike/brand.mjs) (marca, banner, ícones) e [spike/demo.mjs](spike/demo.mjs) (GIFs do README).
Os diagramas e GIFs do README já saem do próprio `dioramic` (`docs/assets/`, T-303); só a marca e o banner continuam no script. Comer a própria comida é parte da marca.
