# Dioramic — guia para agentes

Dioramic desenha diagramas isométricos de system design a partir de um spec declarativo (`*.diorama.yaml`).
Rust de ponta a ponta: core, renderização, servidor MCP, CLI e app desktop (Tauri 2 + Leptos).
O design está em [docs/](docs/README.md); o processo de trabalho, em [docs/work/](docs/work/README.md).

## Idioma

- Documentação, tarefas e handoffs: **português (pt-BR)**.
- Código, identificadores, nomes e descrições de tools MCP, mensagens de diagnóstico e commits: **inglês**.

## Mapa do workspace

| Caminho | Responsabilidade |
|---|---|
| `crates/dioramic-spec` | Tipos do formato Diorama, parse JSON/YAML, atalhos de aresta, normalização, validação, diagnósticos |
| `crates/dioramic-core` | Layout no plano, rotação/densidade, ícones procedurais, temas, cena, setas/portas, colisão de labels, SVG, animação |
| `crates/dioramic-render` | Fontes embutidas, raster (`resvg`), PNG, GIF (paleta global + frames delta), filmstrip, prévia |
| `crates/dioramic-packs` | Packs de ícones de marca (AWS, GCP, Azure, Kubernetes) e ícones locais: instalação a partir das fontes oficiais, manifest, registro carregado para o core |
| `crates/dioramic-mcp` | Servidor MCP (`rmcp`, stdio) |
| `crates/dioramic-cli` | Binário `dioramic` (render, watch, lint, icons, mcp, init) |
| `crates/test-support` | Fixtures, asserts visuais e helpers de snapshot para testes |
| `xtask` | Mini engine de dev: galeria visual, golden files, modo QA rápido |
| `apps/desktop` | App desktop (Tauri 2 + Leptos) |
| `examples/` | Specs de exemplo; também são fixtures de teste |

## Comandos

```bash
cargo xtask qa <crate>          # modo QA rápido: fmt + clippy + testes só do crate
cargo xtask gallery             # renderiza todos os exemplos × temas × rotações × densidades em target/gallery/
cargo xtask golden --update     # regenera os golden files após mudança visual intencional
cargo run -p dioramic-cli -- render examples/checkout.diorama.yaml -o /tmp/checkout.png
```

Agentes em paralelo devem usar um diretório de build próprio para não travarem uns aos outros:
`CARGO_TARGET_DIR=target/agents/<seu-nome>`.

## Clean code (obrigatório)

- Funções pequenas com um propósito; nomes que dizem o que fazem; nada de abreviações obscuras.
- Tipos expressivos em vez de `bool`s e tuplas soltas (`Rotation`, `Density`, `PlanPoint`, `ScreenPoint`).
- Sem `unwrap()`/`expect()` em código de biblioteca; erros com `thiserror` e diagnósticos com código estável.
- Sem números mágicos espalhados: constantes nomeadas ou campos de tema.
- Determinismo: sem `HashMap` onde a ordem afeta a saída (use `IndexMap`/`BTreeMap`); floats formatados com precisão fixa.
- Comentários explicam o **porquê**, não o quê. Doc comments (`///`) em toda API pública.
- `cargo fmt` e `cargo clippy -- -D warnings` limpos.

## Contratos

Os tipos e assinaturas públicas entre crates são **contratos** ([docs/work/contracts/](docs/work/contracts/)).
Só o agente **refiner** altera contrato. Se precisar de mudança, registre no handoff da tarefa e devolva ao orquestrador.
