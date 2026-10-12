# Dioramic quickstart

Dioramic turns a small declarative spec (DioramaSpec v1) into an isometric architecture diagram.
The engine does layout, colors and arrow routing; you describe what exists and how it connects.

## Loop (max 3 rounds)

1. Write a **minimal** spec: nodes, edges, optional zones. No `at`, `size` or colors at first.
2. Call `render_diagram` with `spec` (inline object) or `spec_path` (a `*.diorama.yaml` file you can edit).
3. Look at the preview image and the `diagnostics` in the JSON summary:
   - `E_*` = error, nothing was rendered: fix the spec and call again (`isError: true`).
   - `W_*` = warning: fix it if you can see the problem in the preview.
4. Typical fixes, in this order: too long or sparse → `layout: { density: compact }`;
   hidden labels or arrows → `view: { rotation: auto }`; crowded zone → zone `arrange: row | column`;
   still wrong → pin one or two nodes with `at: [x, y]`.
5. Tell the user the `output_path` (and `spec_path`). Inline specs are saved as `<slug>.diorama.yaml` next to the output.

## Minimal spec

```yaml
diorama: 1
title: Checkout Service
nodes:
  - { id: users, icon: users, label: Users }
  - { id: lb,    icon: loadbalancer, label: Load Balancer }
  - { id: api,   icon: server, label: API, replicas: 2 }
  - { id: db,    icon: postgres }
edges:
  - users -> lb: HTTPS
  - lb -> api
  - api -> db: SQL
```

Other guides: `spec` (full format), `styles` (colors and edge styles), `animation` (GIF / animated SVG),
`examples` (complete specs), `icons` (catalog).
