//! Text written by `init`: the Dioramic skill (`SKILL.md`, Agent Skills standard) and the optional
//! VS Code instructions file. Source: `docs/work/research/clients.md` §4.3 and §4.5.

/// Version written in the skill marker.
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Placeholder replaced by [`GENERATOR_VERSION`].
const VERSION_PLACEHOLDER: &str = "{{VERSION}}";

/// `name` of the skill; must equal its folder name (Agent Skills standard).
pub const SKILL_NAME: &str = "dioramic";

const SKILL_TEMPLATE: &str = r#"---
name: dioramic
description: Draw isometric architecture, network and system-design diagrams (PNG, SVG, GIF) from a small YAML spec with the dioramic MCP tools or CLI. Use when the user asks for an architecture diagram, infrastructure or network map, system design drawing, request-flow animation, or edits a *.diorama.yaml file.
argument-hint: "[what to draw]"
metadata:
  generator: dioramic
  version: "{{VERSION}}"
---

# Dioramic diagrams

Dioramic renders a declarative spec (DioramaSpec v1) into an isometric diagram. The engine does layout, colors and routing; you describe *what* exists and how it connects.

## Workflow

1. Write a **minimal** spec: nodes, edges, optional zones. No coordinates (`at`, `size`) and no colors at first.
   Prefer a file `diagrams/<slug>.diorama.yaml` when you can edit files.
2. Call `render_diagram` with `spec_path` (or inline `spec`). First time in a session, `get_guide("spec")` is available for details.
   Unsure an icon exists? Call `search_icons` first; otherwise use `icon: block` with a `label`.
3. Look at the returned **preview image** and the **diagnostics** in the JSON summary.
   - `E_*` = error, nothing rendered: fix and retry (the message suggests the right id/icon).
   - `W_*` = warning: fix if visible (overlap, edge through node, long label).
4. Iterate **at most 3 rounds**. Typical fixes, in order:
   too long or sparse → `layout.density: compact`; hidden labels/arrows → `view.rotation: auto`;
   crowded zone → zone `arrange: row|column`; tangled edges → `layout.algorithm: hub`; still wrong → add `at: [x, y]` to one or two nodes.
   High-level system design: `theme: schematic` (hub layout, rails, edge colors by kind, legend) reads best.
5. Report the output path(s) to the user. Keep the `.diorama.yaml` next to the image.

## Format cheatsheet

```yaml
diorama: 1                      # required
title: Checkout Service
theme: blueprint                # blueprint | midnight | paper | mono | schematic
layout: { flow: x, density: spaced }   # density: spaced | compact; algorithm: flow | hub; routing: curved | orthogonal | straight | rails
view: { rotation: auto }        # 0 | 90 | 180 | 270 | auto (fewest collisions)
styles:
  trunk: { color: orange, width: 3 }
  async: { color: violet, dashed: true }
zones:                          # real boundaries only (VPC, network, room, cluster, tier)
  - { id: dmz,  label: DMZ, color: blue }
  - { id: data, label: Data tier }
nodes:                          # required, >= 1; ids snake_case, labels <= 28 chars
  - { id: users, icon: users, label: Users }
  - { id: lb,    icon: loadbalancer, label: Load Balancer, zone: dmz }
  - { id: api,   icon: server, label: API, replicas: 2 }
  - { id: db,    icon: postgres, zone: data }
  - { id: queue, icon: block, label: Jobs, color: teal }
edges:                          # "from OP to: label @style"
  - users -> lb: HTTPS
  - lb -> api @trunk
  - api -> db: SQL
  - api ..> queue: enqueue @async
animation: { preset: flow }     # flow | build | story | pulse
```

- Edge ops: `->` arrow at target, `<-` at source, `<->` both, `--` plain line, `..>` dashed arrow (async/logical), `..` dashed line.
- Style marker is `@name` (never `#`, YAML treats it as a comment). Quotes are optional.
- Long form when needed: `- { from: api, to: db, label: SQL, style: trunk, flow: true, route: orthogonal }`.
- Node extras: `replicas: N`, `badge: v2`, `size: s|m|l`, `z: 1`. Zone extras: `parent: <zone>`, `arrange: auto|row|column|grid`, `border: dashed|solid|none`.
- Color tokens: blue, orange, teal, green, violet, pink, red, gray. Edge color = meaning (trunk, access, async, failure), defined once in `styles`.
- About 40 nodes max per diagram; split bigger systems.
- Animated output: `render_diagram` with `format: gif | svg-animated` and `animation.preset`
  (`flow` = moving traffic on edges with `flow: true`, or all; `story` = step-by-step `steps` with captions). You get a 4-frame filmstrip as preview.

## Without the MCP server

If the `dioramic` tools are not available, use the CLI (same engine):

```bash
dioramic lint   diagrams/checkout.diorama.yaml        # diagnostics only
dioramic render diagrams/checkout.diorama.yaml -o diagrams/checkout.png
dioramic render diagrams/checkout.diorama.yaml -o diagrams/checkout.gif
dioramic icons search postgres
```

Open the PNG to inspect it, fix the spec, re-run. If `dioramic` is not installed, tell the user and stop.
"#;

/// `.github/instructions/dioramic.instructions.md` (only with `--instructions`).
pub const INSTRUCTIONS: &str = r#"---
name: Dioramic specs
description: Rules for editing Dioramic isometric diagram specs.
applyTo: "**/*.diorama.{yaml,json}"
---
These files are DioramaSpec v1 (`diorama: 1`). Keep them minimal: no `at`, `size` or colors unless a render showed a problem.
After editing, re-render with the `render_diagram` MCP tool (`spec_path`) or `dioramic render <file> -o <file>.png`, and fix every `E_*` diagnostic.
Use the `dioramic` skill for the full format cheatsheet.
"#;

/// The skill file for this version.
pub fn skill_markdown() -> String {
    SKILL_TEMPLATE.replace(VERSION_PLACEHOLDER, GENERATOR_VERSION)
}

/// `metadata` marker of a generated skill: who wrote it and which version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorMarker {
    /// Version string (`0.1.0`).
    pub version: String,
}

/// Reads the `metadata.generator: dioramic` / `metadata.version` marker from the YAML frontmatter.
/// A file without it was written or edited by the user and must not be replaced silently.
pub fn generator_marker(markdown: &str) -> Option<GeneratorMarker> {
    let frontmatter = markdown.strip_prefix("---\n").or_else(|| markdown.strip_prefix("---\r\n"))?;
    let end = frontmatter.find("\n---")?;
    let mut generated = false;
    let mut version = None;
    for line in frontmatter[..end].lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("generator:") {
            generated = unquote(value) == "dioramic";
        } else if let Some(value) = line.strip_prefix("version:") {
            version = Some(unquote(value).to_owned());
        }
    }
    generated.then(|| GeneratorMarker { version: version.unwrap_or_default() })
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches(|quote| quote == '"' || quote == '\'')
}

/// Numeric `major.minor.patch` comparison; missing or non-numeric parts count as 0.
pub fn version_key(version: &str) -> [u64; 3] {
    let mut key = [0; 3];
    let numbers = version.split(['.', '-', '+']).take(3).map(|part| part.parse().unwrap_or(0));
    for (slot, number) in key.iter_mut().zip(numbers) {
        *slot = number;
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_follows_the_agent_skills_standard() {
        let skill = skill_markdown();
        assert!(skill.starts_with("---\nname: dioramic\n"));
        let description = skill.lines().find_map(|line| line.strip_prefix("description: ")).unwrap_or_default();
        assert!((1..=1024).contains(&description.len()));
        assert!(skill.lines().count() < 500);
        assert!(!skill.contains(VERSION_PLACEHOLDER));
        assert_eq!(generator_marker(&skill), Some(GeneratorMarker { version: GENERATOR_VERSION.to_owned() }));
    }

    #[test]
    fn user_files_have_no_marker() {
        assert_eq!(generator_marker("# my notes\n"), None);
        assert_eq!(generator_marker("---\nname: dioramic\ndescription: mine\n---\nbody"), None);
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_key("0.10.0") > version_key("0.9.3"));
        assert_eq!(version_key("1.2"), [1, 2, 0]);
        assert_eq!(version_key("1.2.3-beta"), [1, 2, 3]);
    }
}
