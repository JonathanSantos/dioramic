# DioramaSpec v1

```yaml
diorama: 1                     # required, always 1
title: string                  # optional, top-left corner
subtitle: string               # optional
theme: blueprint               # blueprint | midnight | paper | mono | schematic
layout: { flow: x, density: spaced, wrap: auto, algorithm: flow, routing: curved, edge_colors: style }   # algorithm flow | hub; edge_colors semantic | style | mono
view: { rotation: 0, grid: { size: 1, style: lines }, legend: auto }   # rotation 0 | 90 | 180 | 270 | auto; grid style lines | dots | none; legend auto | on | off
styles: { name: { color, width, dashed } }
zones: [Zone]                  # optional
nodes: [Node]                  # required, at least 1
edges: [Edge]                  # optional
animation: { ... }             # optional, see get_guide("animation")
```

## Node

`{ id, icon, label, zone, color, replicas, badge, at, z, size, note }`

- `id`: unique snake_case (letters, digits, `_`, `-`, `/`; starts with a letter).
- `icon`: catalog id or alias (`server`, `postgres`, `k8s/pod`), or a provider/project icon (`aws/lambda`, `gcp/bigquery`,
  `local/logo`). Omitted → inferred from the id, else `block`. Unsure? Call `search_icons`. Unknown icon → `E_UNKNOWN_ICON`
  with a suggestion; provider pack not installed → `W_ICON_PACK_MISSING` (renders with a stand-in).
- `label`: keep it ≤ 28 chars (longer wraps to 2 lines, `W_LABEL_LONG`).
- `replicas: N` draws stacked copies and a `×N` badge; `badge: v2` adds a small tag; `size: s | m | l`; `z` = elevation.
- `at: [x, y]` pins the node in plan tiles (half tiles allowed). Avoid unless a render showed a problem.

## Zone

`{ id, label, color, parent, arrange, at, size, border }`

- Use zones only for **real boundaries**: network, VPC, room, cluster, tier.
- `parent: <zone id>` nests a zone. `arrange: auto | row | column | grid`. `border: dashed | solid | none`.

## Edge

Shorthand (preferred): `"from OP to: label @style"`. Quotes are optional in YAML.

| op | meaning |
|---|---|
| `->` | arrow at target |
| `<-` | arrow at source |
| `<->` | arrows at both ends |
| `--` | plain line |
| `..>` | dashed arrow (async, logical dependency) |
| `..` | dashed line |

- The style marker is `@name`, never `#` (YAML treats ` #` as a comment).
- Long form: `{ from: api, to: db, label: SQL, style: trunk, arrow: end, route: orthogonal, flow: true }`.
- Map form: `- api -> db: { label: SQL, style: trunk, flow: true }`.
- Edge ids default to `"from->to"` (used by animation steps).
- `kind: data | cache | messaging | external | client | service | observability` sets the connection class used by semantic edge
  colors (default: inferred; `@data` works too). See get_guide("styles").

## Layout and view

- `layout.density`: `spaced` (default, zones in one line) or `compact` (zones folded into a block, ~58% less area).
- `layout.algorithm`: `flow` (default; zones in flow order, `wrap` folds them) | `hub` (default in `schematic`: the busiest nodes in
  the middle, short aligned edges, zones packed as rectangles, sources still first; `wrap` is ignored; a node or zone with `at`
  falls back to `flow`). Try `hub` when edges cross or run long.
- `layout.flow`: `x` (default) or `y`. `layout.routing`: `curved` (default; `rails` in the `schematic` theme) | `orthogonal` (the same elbows as `rails`, tighter corners) | `straight` | `rails` | `elbow` (`elbow` = `rails`: isometric elbows — legs along the floor axes only, 90° turns with rounded corners, no diagonals, as few turns as possible; every line leaves its own slot of a node side and parallel lines keep a lane apart; an arrow may arrive at a back face and then ends where the line meets the node's outline). `view.grid`: `size` (tiles, 0.25–4), `style` (`lines` | `dots` | `none`), `opacity` (0–1), `color`.
- `layout.edge_colors`: `style` (default; `semantic` in `schematic`) colors edges by style name, `semantic` by connection class,
  `mono` with one color. `view.legend`: `auto` (default: shown with semantic colors and 2+ classes) | `on` | `off`.
- `rails` looks best with `density: spaced`; nodes sit about 1 tile apart in `compact`, so there is little floor for parallel lanes.
- `render_diagram(layout_algorithm, routing, grid, icon_animation)` overrides these for one render without editing the spec; the summary's
  `view` echoes what was used.
- `view.rotation: auto` tries the 4 camera angles and keeps the one with the fewest collisions.
  Rotation changes only the view, never ids or coordinates.
- Plan axes at rotation 0: `x` grows down-right, `y` grows down-left on screen.

## Rules

1. Start without `at`, `size` and colors. Add hints only after a render shows a problem.
2. snake_case ids, short labels, about 40 nodes max per diagram (split bigger systems).
3. Edge color = meaning (trunk, access, async, failure): define it once in `styles`.
4. Unknown icon? `search_icons`, else `icon: block` with a `color` and `label`.

## Diagnostics

`E_PARSE`, `E_VERSION`, `E_NO_NODES`, `E_EDGE_SYNTAX`, `E_BAD_ID`, `E_DUPLICATE_ID`, `E_UNKNOWN_REF`, `E_UNKNOWN_ICON`,
`E_ZONE_CYCLE`, `E_BAD_COLOR`, `E_BAD_VALUE`, `E_LIMIT` are errors (nothing rendered).
`W_LABEL_OVERLAP`, `W_EDGE_THROUGH_NODE`, `W_LABEL_LONG`, `W_TOO_MANY_NODES`, `W_SELF_LOOP`, `W_ICON_PACK_MISSING`, `W_EDGE_CROSSING` are warnings; `W_ISOLATED` is a hint.
Each diagnostic has `path` (e.g. `edges[2]`), `message` and often a `hint` with the fix.
