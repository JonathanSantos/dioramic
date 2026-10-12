# Exemplos

Specs de exemplo no formato Diorama v1 (`*.diorama.yaml`, ver [docs/04-spec.md](../docs/04-spec.md)).
Eles também são fixtures de teste e entram na galeria (`cargo xtask gallery`).

```bash
cargo run -p dioramic-cli -- render examples/checkout.diorama.yaml -o /tmp/checkout.png
```

| Arquivo | Diagrama | O que demonstra |
|---|---|---|
| [minimal.diorama.yaml](minimal.diorama.yaml) | Checkout mínimo (4 nós) | O exemplo mínimo do doc 04: sem zonas, sem coordenadas, sem cores; `replicas` e atalho de arestas |
| [checkout.diorama.yaml](checkout.diorama.yaml) | Checkout Service | A cena da demo: 4 zonas em linha (INTERNET, EDGE, APP TIER, DATA), estilos do tema (`@primary`, `@secondary`), label com `#` entre aspas e `animation: { preset: flow }` |
| [bank-branch.diorama.yaml](bank-branch.diorama.yaml) | Mayfair Branch Network | O exemplo completo do doc 04, que reproduz a imagem de referência: 17 nós, estilos nomeados (`trunk`, `access`), zona aninhada (`parent`) sem borda, nó fora de zona e aresta `..` sem seta |
| [kubernetes-platform.diorama.yaml](kubernetes-platform.diorama.yaml) | Storefront on Kubernetes | Cluster com namespaces como zonas aninhadas, `replicas`, `badge`, tema `midnight` e `layout: { density: compact }` |
| [event-driven.diorama.yaml](event-driven.diorama.yaml) | Order Events Pipeline | Arestas assíncronas `..>`, estilo de falha (dead letter queue), `color` em nó, tema `paper` e `view: { rotation: auto }` |
| [ml-pipeline.diorama.yaml](ml-pipeline.diorama.yaml) | Recommendation ML Pipeline | Fluxo longo em 5 zonas, laço de realimentação (drift → retreino) com aresta tracejada e cor de aresta como significado |
| [aws-serverless.diorama.yaml](aws-serverless.diorama.yaml) | Serverless Orders API | Ícones de provedor (`aws/lambda`, `aws/s3`, `aws/sqs`…) planos projetados; precisa de `dioramic icons install aws`, e sem o pack renderiza com ícones embutidos e o aviso `W_ICON_PACK_MISSING` |
| [web-platform.diorama.yaml](web-platform.diorama.yaml) | Web Platform | System design de alto nível no tema `schematic`: clientes → borda → serviços → dados, arestas sem estilo coloridas pela classe da conexão (client, external, service, messaging, cache, data), layout `hub`, `rails` e ponta no meio por padrão do tema (sem `wrap` nem rotação no spec), legenda automática |

### Galeria de system designs (`schematic/`)

[schematic/](schematic/README.md) traz dez specs no tema `schematic`, do básico ao complexo (cliente-servidor, três camadas, site
estático com CDN, encurtador de URL, rate limiter, news feed, upload de vídeo e três arquiteturas conceituais: chat, sync de arquivos e
assistente de IA), com miniaturas e o que cada um ensina. Ficam numa subpasta, então os testes e o `cargo xtask gallery`, que leem só
`examples/*.diorama.yaml`, não os varrem.

## Convenções

- Cada arquivo começa com 1–2 linhas de comentário descrevendo o diagrama.
- Ids em `snake_case`; labels com até 28 caracteres.
- O estilo de aresta usa `@` (em YAML, ` #` começa um comentário); labels que contêm `#` vão entre aspas.
