# Styles and colors

## Themes

`theme: blueprint` (default, light blue-grey with grid) · `midnight` (dark navy, neon edges) ·
`paper` (warm off-white, pastel) · `mono` (greyscale, print) · `schematic` (technical drawing for high-level system
design: silver icons, `hub` layout, thick `rails` with the arrow halfway, semantic edge colors and a legend). `render_diagram(theme: ...)`
overrides the spec. For a high-level system design (clients → edge → services → data) start with `theme: schematic` and no
`wrap`, `rotation` or `at`: the defaults keep the spine readable.

## Semantic edge colors

With `layout: { edge_colors: semantic }` (the default in `schematic`) each edge is colored by its **class**, inferred from the icons
at its ends, its operator and its label: `data` (database, storage), `cache`, `messaging` (queues, `..>` async), `external`
(internet, CDN, DNS), `client` (users, laptops, phones), `service` (default), `observability` (monitoring). Leave edges unstyled and
let the classes color them; force one with `kind: data` or `@data`. An explicit `@style` or `color` always wins. Recolor a class with a
style of the same name: `styles: { data: { color: red } }`. `view: { legend: auto | on | off }` controls the legend (auto: 2+ classes).

## Color tokens

`blue`, `orange`, `teal`, `green`, `violet`, `pink`, `red`, `gray`. Hex (`#3e50c8`) also works, but tokens
follow the theme, so prefer them. Zones without `color` rotate through the palette.

## Edge styles

Theme styles, always available: `primary` (default), `secondary`, `muted`, `danger`, `success`.
Define your own once and reference them with `@name`:

```yaml
styles:
  trunk:  { color: orange, width: 3 }
  access: { color: blue }
  async:  { color: violet, dashed: true }
  fail:   { color: red, dashed: true }
edges:
  - fw -> core: 1Gbps trunk @trunk
  - api ..> queue: enqueue @async
  - worker ..> dlq: on failure @fail
```

- Edge color carries **meaning** (trunk, access, async, failure). Do not color edges for decoration.
- Per-edge overrides in long form: `color`, `width` (px), `dashed`.
- `..>` and `..` are dashed regardless of the style.

## Nodes and zones

- `nodes[].color` tints the procedural icon (useful with `icon: block` or to flag a node, e.g. a red DLQ).
- `zones[].color` colors the floor area and its label; `border: dashed | solid | none`.
