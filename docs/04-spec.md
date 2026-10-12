# 04 — Formato Diorama v1 (`*.diorama.yaml`)

É o formato que a IA escreve. Os objetivos, em ordem: **(1) impossível de errar geometria**, **(2) poucos tokens**, **(3) legível por humanos**.

## Exemplo mínimo

```yaml
diorama: 1
title: Checkout Service
nodes:
  - { id: users, icon: users,   label: Users }
  - { id: lb,    icon: loadbalancer, label: Load Balancer }
  - { id: api,   icon: server,  label: API, replicas: 2 }
  - { id: db,    icon: postgres }
edges:
  - users -> lb: HTTPS
  - lb -> api
  - api -> db: SQL
```

Sem zonas, sem coordenadas, sem cores: o motor escolhe tudo. É o que a IA deve tentar primeiro.

## Estrutura

```yaml
diorama: 1                  # versão do spec (obrigatório)
title: string                # opcional; aparece no canto superior
subtitle: string             # opcional
theme: blueprint             # blueprint | midnight | paper | mono | schematic   (ver doc 06)
layout: { ... }              # densidade e dicas globais de layout (opcional)
view: { ... }                # câmera: rotação (opcional)
styles: { nome: EdgeStyle }  # estilos de aresta nomeados (opcional)
zones: [Zone]                # áreas no chão (opcional)
nodes: [Node]                # obrigatório, ≥ 1
edges: [Edge | string]       # opcional
animation: { ... }           # opcional (ver doc 07)
```

### Eixos do plano

O desenho é feito num **plano de chão com grid de tiles**. Os dois eixos aparecem na tela como diagonais:

```
            (0,0)
           ╱     ╲
     y ↙  ╱       ╲  ↘ x
         ╱         ╲
```

- `x` cresce **descendo para a direita**; `y` cresce **descendo para a esquerda**. `z` é altura.
- Coordenadas são **sempre opcionais** e em **tiles inteiros** (ou meio tile). A IA só usa quando quer forçar algo.
- Os eixos acima valem para `view.rotation: 0`. Girar a câmera não muda o spec: `at: [x, y]` continua se referindo ao mesmo lugar do "mundo".

### `layout`

| Campo | Tipo | Default | Descrição |
|---|---|---|---|
| `flow` | `x` \| `y` | `x` | Eixo principal do fluxo (origens → destinos) |
| `density` | `spaced` \| `compact` | `spaced` | **Espaçado:** zonas em linha ao longo do fluxo, com folga generosa (o visual da demo); dobra em linhas só quando a linha ficaria longa demais (ver `wrap`). **Compacto:** zonas dobradas em bloco (serpenteando), com espaçamentos e linhas-guia menores; no checkout ocupa cerca de 46% menos área |
| `algorithm` | `flow` \| `hub` | `flow` (`hub` no tema `schematic`) | Como zonas e nós são posicionados. `flow`: zonas na ordem do fluxo, em linha ou dobradas (`wrap`). `hub`: os nós mais conectados no centro, arestas curtas e alinhadas na malha de meio tile, zonas como retângulos empacotados, origens ainda no começo do fluxo; `wrap` é ignorado e um `at` em qualquer nó/zona faz cair no `flow` ([06](06-renderizacao.md#layout-hub)). O default vem do tema quando o spec não escreve `algorithm`: `schematic` usa `hub`; escrito (ou `--layout`, `layout_algorithm` no MCP, Fluxo/Hub no app), vence. Os temas clássicos ficam em `flow` por decisão (T-423): para hubs, escreva `hub` ou use o `schematic` |
| `wrap` | int \| `auto` | `auto` | Zonas por linha antes de dobrar (as linhas serpenteiam). `auto` no `compact` = ⌈√n⌉. `auto` no `spaced` = todas numa linha enquanto a figura tiver até ~25 tiles de largura; acima disso, a dobra que mostra o texto maior num quadro 16:10 ([06](06-renderizacao.md#densidade)). Um número fixo vale nas duas densidades |
| `routing` | `curved` \| `orthogonal` \| `straight` \| `rails` \| `elbow` | `curved` (`rails` no tema `schematic`) | Estilo padrão das arestas. `curved` é o da referência; `orthogonal` segue o grid, como no Isoflow (os mesmos cotovelos do `rails`, com cantos mais fechados); `rails` (sinônimo `elbow`) são **cotovelos isométricos**: só retas nos eixos do plano, viradas de 90° no plano com canto arredondado (sem diagonais), o mínimo de viradas, folga dos nós; a ponta pode chegar por uma face de trás quando isso economiza viradas e então fica na borda da silhueta do destino ([06](06-renderizacao.md#trilhos-isométricos-rails-sinônimo-elbow)). O default vem do tema quando o spec não escreve `routing`: `schematic` usa `rails`; escrito (ou `--routing`), vence |
| `edge_colors` | `semantic` \| `style` \| `mono` | `semantic` no `schematic`; `style` nos outros | Como as arestas ganham cor. `style`: pelo nome do estilo (`primary`, `secondary`… ou `styles`), o visual de sempre. `semantic`: pela **classe da conexão** (`data`, `cache`, `messaging`, `external`, `client`, `service`, `observability`), inferida das pontas, do operador e do label ([06](06-renderizacao.md#cores-semânticas-de-conexão)); arestas com `@estilo` ou `color` explícitos mantêm o que escreveram. `mono`: uma cor só para tudo que não tem `color` |

![rotação e densidade](assets/rotacao-densidade.png)

### `view`

| Campo | Tipo | Default | Descrição |
|---|---|---|---|
| `rotation` | `0` \| `90` \| `180` \| `270` \| `auto` | `auto` se `layout.density: compact`; senão `0` | Gira a câmera em volta da cena, em passos de 90°. `auto` testa os 4 ângulos e escolhe o de **menos colisões** (labels, pílulas, zonas); empate → menos arestas atravessando/escondidas → 0° → menor área. Custa poucos milissegundos |
| `legend` | `auto` \| `on` \| `off` | `auto` | Legenda das classes de conexão no canto inferior (amostra de traço + nome curto: Data, Cache, Messaging…). Só com `edge_colors: semantic`; `auto` mostra quando há duas classes ou mais em uso. Entra no enquadramento e na colisão |
| `grid` | `{ size, style, opacity, color }` | `{ size: 1, style: lines, opacity: 1 }` | Malha de fundo. `size`: lado da célula em tiles do plano (0,25 a 4; na tela, cada lado mede `size × 56 px` ao longo do eixo isométrico, com o tile padrão de 56 px). `style`: `lines`, `dots` (um ponto em cada cruzamento) ou `none`. `opacity`: 0 a 1. `color`: token ou hex (default: a cor de malha do tema). A malha fica presa à origem do plano do diagrama e gira com ele, nas 4 rotações. Valor fora da faixa → `E_BAD_VALUE`; cor malformada → `E_BAD_COLOR` |

- Girar a câmera muda **só a vista**: layout, ids e coordenadas do spec continuam iguais.
- O default `auto` do compacto é resolvido a partir do spec: trocar a densidade por fora (`--density compact`, alternância do app) num spec `spaced` sem `rotation` mantém 0°.
- Detalhes dos ícones (LEDs, portas do switch, logos) ficam sempre na face visível; os labels de zona escolhem de novo a borda livre.

### `Zone`

| Campo | Tipo | Default | Descrição |
|---|---|---|---|
| `id` | string | — | Obrigatório, `snake_case` |
| `label` | string | — | Texto da pílula no chão |
| `color` | token \| hex | rotação da paleta | `blue`, `orange`, `teal`, `green`, `violet`, `pink`, `red`, `gray` |
| `parent` | id de zona | — | Zona aninhada (ex.: ATMs dentro do Banking Hall) |
| `arrange` | `auto` \| `row` \| `column` \| `grid` | `auto` | Como os nós se distribuem dentro da zona |
| `at` | `[x, y]` | auto | Fixa o canto superior da zona no plano |
| `size` | `[w, d]` | auto | Força o tamanho em tiles |
| `border` | `dashed` \| `solid` \| `none` | `dashed` | |

### `Node`

| Campo | Tipo | Default | Descrição |
|---|---|---|---|
| `id` | string | — | Obrigatório, `snake_case`, único |
| `icon` | string | inferido do `id` ou `block` | Id ou alias do catálogo (30 ícones procedurais: `server`, `postgres`, `storage` para S3/balde, `nas` para NAS/SAN/disco, `k8s/pod`…), um ícone de provedor `<pack>/<id>` (`aws/lambda`, `gcp/bigquery`, `azure/functions`, `k8s/pod`; ver [Ícones de provedores](#ícones-de-provedores-e-do-projeto)) ou do projeto (`local/<arquivo>`). Use `search_icons` |
| `label` | string | — | Até cerca de 28 caracteres; mais que isso quebra em 2 linhas |
| `zone` | id de zona | — | Sem zona e sem `at`, o nó fica ao lado da zona com que mais conversa (fora da reta entre zonas); sem arestas com zonas, ganha célula própria pela ordem do fluxo |
| `color` | token \| hex | do ícone | Tinge primitivas procedurais e o bloco de ícones planos |
| `replicas` | int | 1 | Desenha um "baralho" de até 3 cópias alinhadas ao grid (passos iguais para trás, as de trás levemente escurecidas) e um badge `×N` |
| `badge` | string | — | Pequena etiqueta no canto (ex.: `v2`, `!`) |
| `at` | `[x, y]` | auto | Fixa a posição (tiles) |
| `z` | número | 0 | Elevação (ex.: algo "em cima" do rack) |
| `size` | `s` \| `m` \| `l` | `m` | Escala do ícone |
| `note` | string | — | Tooltip no export HTML e no app desktop; não aparece no PNG |

### `Edge`

Forma curta (recomendada), uma string:

```
<de> <op> <para> [: label] [@estilo]
```

| `op` | Significado |
|---|---|
| `->` | seta no destino |
| `<-` | seta na origem |
| `<->` | seta nos dois lados |
| `--` | linha sem seta |
| `..>` | tracejada com seta (dependência lógica, async) |
| `..` | tracejada sem seta |

Exemplos: `"fw -> core: 1Gbps trunk @trunk"`, `"rack .. core: Comms room rack"`.

> **Cuidado com YAML.** Sem aspas, `- fw -> core: 1Gbps trunk` é lido pelo YAML como um *mapeamento*
> `{"fw -> core": "1Gbps trunk"}`. O parser **aceita as duas formas** (string ou mapeamento de uma chave), então funciona com ou sem aspas.
> Por isso o marcador de estilo é `@` e não `#`: em YAML, ` #` começa um comentário e o estilo seria descartado em silêncio.
> O mapeamento também aceita objeto: `- fw -> core: { label: 1Gbps trunk, style: trunk, flow: true }`.

Forma longa, para quando precisar de mais campos:

| Campo | Tipo | Default | Descrição |
|---|---|---|---|
| `id` | string | `"de->para"` | Usado por `animation.steps` |
| `from`, `to` | id de nó | — | Obrigatórios |
| `label` | string | — | Pílula no meio da aresta |
| `style` | nome | `primary` | De `styles` ou do tema (`primary`, `secondary`, `muted`, `danger`, `success`). Um nome de classe (`@data`, `@cache`…) fixa a classe da aresta |
| `kind` | `data` \| `cache` \| `messaging` \| `external` \| `client` \| `service` \| `observability` | inferido | Classe da conexão, usada pelas cores semânticas e pela legenda |
| `color`, `width`, `dashed` | | do estilo | Sobrescrevem o estilo |
| `arrow` | `end` \| `start` \| `both` \| `none` | `end` | |
| `route` | `curved` \| `orthogonal` \| `straight` \| `rails` \| `elbow` | `layout.routing` | `elbow` = `rails` |
| `flow` | `true` \| `false` \| `reverse` | `false` | Liga a animação de tráfego nesta aresta |

### `styles`

```yaml
styles:
  trunk:  { color: orange, width: 3 }
  access: { color: blue }
  vpn:    { color: violet, dashed: true }
  data:   { color: "#c0262d" }   # nome de classe: muda a cor da classe nas cores semânticas
```

Um estilo com o nome de uma classe (`data`, `cache`, `messaging`, `external`, `client`, `service`, `observability`) sobrescreve a cor,
a largura ou o tracejado **daquela classe** quando `edge_colors: semantic`; também pode ser usado como `@data` numa aresta.

### `animation`

| Campo | Tipo | Default | Descrição |
|---|---|---|---|
| `preset` | `flow` \| `build` \| `story` \| `pulse` | `flow` | Ver [07](07-animacao.md#presets) |
| `fps` | int | 50 | Quadros por segundo do GIF |
| `duration` | número (s) \| `"2.5s"` \| `"800ms"` | 2 s | Duração do loop |
| `loop` | bool | `true` em `flow`/`pulse` | |
| `icons` | bool | `true` | Movimento leve dentro dos ícones (LEDs piscando, ondas, anéis, hélice do k8s), em loop perfeito com o diagrama ([07](07-animacao.md#ícones-animados)). `false` deixa os ícones parados. Não afeta PNG nem SVG estático |
| `steps` | lista | — | Passos do `story` |

## Regras para a IA (vão no `guide` do MCP)

1. Comece **sem** `at`, `size` e cores. Só adicione dicas se o render mostrar um problema.
   Se o diagrama ficou longo demais ou com muito espaço vazio, tente `layout.density: compact`; se algo ficou escondido, `view.rotation: auto`.
2. Ids em `snake_case`, labels curtos (≤ 28 caracteres), no máximo cerca de 40 nós por diagrama (acima disso, divida).
3. Use zonas para **agrupar por fronteira real** (rede, sala, VPC, cluster, tier), não por enfeite.
4. Cor de aresta = **significado** (tronco, acesso, async, falha). Defina em `styles` e reutilize.
5. Antes de inventar um ícone, chame `search_icons`. Se não existir, use `block` com `color` e `label`.

## Diagnósticos

| Código | Severidade | Exemplo de mensagem |
|---|---|---|
| `E_PARSE` | erro | `linha 12, coluna 5: esperado ':'` |
| `E_DUPLICATE_ID` | erro | `nodes[4].id 'core' já existe em nodes[1]` |
| `E_UNKNOWN_REF` | erro | `edges[2]: nó 'core_sw' não existe. Você quis dizer 'core'?` |
| `E_UNKNOWN_ICON` | erro | `nodes[3].icon 'postgress' não existe. Você quis dizer 'postgres'?` (também `aws/lamda` com o pack instalado) |
| `W_ICON_PACK_MISSING` | aviso | `nodes[2].icon 'aws/lambda' precisa do pack 'aws', que não está instalado; desenhado como 'function'` (dica: `dioramic icons install aws`) |
| `E_ZONE_CYCLE` | erro | `zona 'a' é ancestral de si mesma` |
| `W_LABEL_OVERLAP` | aviso | `labels de 'ws1' e 'phones' se sobrepõem; tente arrange: row na zona 'teller'` |
| `W_EDGE_THROUGH_NODE` | aviso | `aresta api2->db passa por cima de 'cache'` |
| `W_EDGE_CROSSING` | aviso | `arestas 'lb->api1' e 'lb->api2' compartilham um nó e se cruzam` (o leque vira um "X"; T-400) |
| `W_LABEL_LONG` | aviso | `label de 'mgr_ws' tem 34 caracteres; foi quebrado em 2 linhas` |
| `W_ISOLATED` | dica | `nó 'printer' não tem arestas` |
| `W_TOO_MANY_NODES` | aviso | `52 nós; considere dividir em 2 diagramas` |

## Ícones de provedores e do projeto

Ícones oficiais de **AWS, Google Cloud, Azure e Kubernetes** entram como `<pack>/<id>` e são desenhados **planos projetados**
sobre uma base isométrica ([06](06-renderizacao.md#ícones-planos-projetados)). O spec não muda: `icon: aws/lambda` é só um id.

- **Packs:** instalados uma vez com `dioramic icons install aws|gcp|azure|k8s`, que baixa o pacote oficial do provedor
  ([ADR-007](02-decisoes.md#adr-007)). Ids normalizados dos nomes de arquivo (`Arch_AWS-Lambda_64.svg` → `aws/lambda`), com apelidos
  curtos (`aws/s3`, `aws/sqs`, `azure/functions`, `k8s/deploy`). Descubra com `dioramic icons search` ou `search_icons`.
- **Pack ausente não quebra o render:** o nó é desenhado com o ícone embutido mais próximo (`aws/lambda` → `function`) e sai o aviso
  `W_ICON_PACK_MISSING` com o comando de instalação. Ícone inexistente **num pack instalado** é `E_UNKNOWN_ICON` com sugestão.
- **Ícones do projeto:** cada `icons/*.svg` na pasta do spec vira `local/<nome-do-arquivo>` (`icons/Stripe Logo.svg` → `local/stripe-logo`).
  Um `icons/manifest.json` opcional (mesmo formato do manifest de pack) define nome, apelidos e `mount: top | stand`.
  Ícones do projeto valem para o CLI, o MCP (pasta do `spec_path`, ou a raiz do workspace para specs inline) e têm precedência sobre os packs.

## Exemplo completo

Recria a [imagem de referência](assets/referencia.webp) (rede de uma agência bancária):

```yaml
diorama: 1
title: Mayfair Branch Network
theme: blueprint

styles:
  trunk:  { color: orange, width: 2.5 }
  access: { color: blue }

zones:
  - { id: wan,    label: WAN,           color: blue }
  - { id: comms,  label: Comms Room,    color: orange }
  - { id: guest,  label: Customer WiFi, color: blue }
  - { id: teller, label: Teller Area,   color: blue }
  - { id: hall,   label: Banking Hall,  color: blue }
  - { id: atm,    label: ATM Lobby,     color: teal, parent: hall, border: none }

nodes:
  - { id: uplink,    icon: cloud,       label: MPLS / Internet Uplink, zone: wan }
  - { id: fw,        icon: firewall,    label: Branch Firewall,        zone: wan }
  - { id: rack,      icon: rack,        label: MAYFAIR-RACK-01 }
  - { id: server,    icon: server,      label: Branch Server,          zone: comms }
  - { id: core,      icon: switch,      label: Branch Core Switch,     zone: comms }
  - { id: nas,       icon: nas,         label: Backup NAS,             zone: comms }
  - { id: guest_ap,  icon: access-point, label: Customer WiFi AP,      zone: guest }
  - { id: teller_sw, icon: switch,      label: Teller Area Switch,     zone: teller }
  - { id: ws1,       icon: workstation, label: Teller Workstation 1,   zone: teller }
  - { id: ws2,       icon: workstation, label: Teller Workstation 2,   zone: teller }
  - { id: phones,    icon: voip-phone,  label: Teller VoIP Phones,     zone: teller }
  - { id: staff_ap,  icon: access-point, label: Staff WiFi AP,         zone: teller }
  - { id: printer,   icon: printer,     label: Branch Printer,         zone: teller }
  - { id: hall_sw,   icon: switch,      label: Banking Hall Switch,    zone: hall }
  - { id: mgr_ws,    icon: workstation, label: Branch Manager Workstation, zone: hall }
  - { id: atm1,      icon: atm,         label: Lobby ATM 1,            zone: atm }
  - { id: atm2,      icon: atm,         label: Lobby ATM 2,            zone: atm }

edges:
  - uplink -> fw: MPLS link to data centre @access
  - fw -> core: 1Gbps trunk @trunk
  - core -> server: 1Gbps Cat6 @trunk
  - core -> nas: 1Gbps Cat6 @trunk
  - rack .. core: Comms room rack
  - core -> guest_ap: PoE+ (guest VLAN only) @trunk
  - core -> teller_sw: 1Gbps PoE+ trunk @trunk
  - core -> hall_sw: 1Gbps trunk @trunk
  - teller_sw -> phones: PoE @access
  - teller_sw -> ws1: Cat6 @access
  - teller_sw -> ws2: Cat6 @access
  - teller_sw -> staff_ap: PoE @access
  - teller_sw -> printer: Cat6 @access
  - hall_sw -> mgr_ws: Cat6 @access
  - hall_sw -> atm1: ATM VLAN @access
  - hall_sw -> atm2: ATM VLAN @access
```

São cerca de 50 linhas (estimativa: menos de 1k tokens) para um diagrama de 17 nós, sem nenhuma coordenada.

## Evolução do formato

- O campo `diorama: 1` é obrigatório. Mudanças incompatíveis viram `diorama: 2`, e o binário traz um migrador (`dioramic migrate`).
- Futuro (v1.x): `layers` (andares empilhados, o clássico "bolo de camadas" isométrico: rede / app / dados), `groups` visuais sem zona,
  `annotations` (notas soltas, setas de destaque) e import/export do JSON do Isoflow/FossFLOW.
