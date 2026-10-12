# Animation

Ask `render_diagram` for `format: gif` or `format: svg-animated`. The preview you get back is a
**filmstrip** of 4 key frames side by side (models cannot watch a GIF).

## Presets

| preset | loop | what moves | best for |
|---|---|---|---|
| `flow` (default) | yes | dashes and packets along edges with `flow: true` (or all edges) | "the system is alive" |
| `build` | no | zones appear, nodes drop in by flow rank, edges draw, labels fade in | openings, posts |
| `story` | optional | step by step: highlights a path, dims the rest, captions | explaining a request path, failover |
| `pulse` | yes | pulsing ring on the nodes of steps with `effect: pulse` | incidents, hotspots |

## Spec

```yaml
animation:
  preset: story
  fps: 50              # raster frames per second (GIF default 50)
  duration: 2s         # loop length (seconds or "800ms"), default 2 s
  loop: true
  icons: true          # small looping motion inside the icons (LEDs, waves, rings); false = still
  steps:
    - { path: [users, lb, api, db], caption: "1. Checkout hits the LB and is stored", duration: 2.5s }
    - { edges: [api->cache], caption: "2. Cart read from Redis" }
    - { nodes: [db], effect: pulse, caption: "3. Primary overloaded" }
```

- `path` is sugar: consecutive nodes become the edges between them (they must exist).
- Edge ids are `"from->to"` unless the edge has an explicit `id`.
- Mark the edges that carry traffic with `flow: true` (or `flow: reverse`) in long/map form;
  without any, `flow` animates every edge.
- `render_diagram(animation: { preset, duration, fps })` overrides the spec without editing it;
  `render_diagram(icon_animation: false)` keeps the icons still for one render.
- Icon motion loops seamlessly with the packets. Provider (brand) icons stay still; with `replicas`, only the front copy moves.

## Formats

- `gif`: 1600 px wide, 50 fps, seamless loop, works everywhere (README, Slack, PRs).
- `svg-animated`: a few KB, sharp at any zoom, text as outlines; GitHub READMEs show it, chat apps often do not.
- `apng`, `webp`, `mp4`, `webm` are not supported yet: use `gif`.
