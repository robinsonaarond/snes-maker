# Feature Implementation Review

Date: 2026-03-31

## Overall Determination

This is a strong maker foundation, but it is not a fully mature maker yet.

My short read is:

- Editor shell and workflow foundation: strong
- Validation, build, export, and playtest loop: strong
- Advanced visual authoring workflows: still uneven
- Overall against `FEATURES.md`: about **7.5/10**

The project already does real work end to end. `cargo test` passed, the sample project validated cleanly, and `cargo run -p snesmaker-cli -- build-rom examples/megaman_like_demo` produced a ROM plus `build-report.json`. The best-implemented areas are the workspace/layout system, scene outliner, diagnostics, playtest sandbox, physics tuning, and validation/export pipeline.

The weakest areas are the ones that need deeper visual authoring: metasprites, animation editing, dialogue/event graph editing, prefab overrides, auto-tiling, and browser drag-and-drop.

## What I Verified

- Reviewed `FEATURES.md`
- Read the core workspace crates and editor implementation
- Ran `cargo test`
- Ran `cargo run -p snesmaker-cli -- check examples/megaman_like_demo`
- Ran `cargo run -p snesmaker-cli -- build-rom examples/megaman_like_demo`

## Feature-By-Feature Assessment

| Feature Area | `FEATURES.md` Status | My Assessment | Notes |
| --- | --- | --- | --- |
| Dockable workspace with saved layouts | Complete | Strong | Workspace presets, dock tabs, saved layouts, split panes, and persistence are real. The one caveat is that this is a custom fixed-slot docking system, not truly freeform floating-window docking. |
| Scene outliner and layer manager | Complete | Strong | Scenes, layers, spawns, checkpoints, entities, triggers, and scripts are surfaced in the outliner. Visibility, locking, soloing, focus, duplication, rename, and layer reorder are present. |
| Content browser and asset library | Mostly complete | Solid | Filtering, thumbnails, favorites, and usage summaries are implemented well. The missing drag-and-drop is real, and several assets still route to status messages, previews, or clipboard loading instead of direct in-canvas placement. |
| Reusable prefabs, archetypes, and room chunks | Partial | Partial | Scene snippets and tile brushes are useful and genuinely implemented, but there is no prefab instance system, no instance overrides, and no built-in starter library. |
| Advanced tilemap authoring tools | Mostly complete | Solid | Eyedropper, bulk fill/clear, collision painting, line drawing, mirror tools, flood fill, clipboard stamping, and saved brushes are all present. Auto-tiling and adjacency rules are still absent. |
| Visual metasprite and animation editor | Open | Partial | Animation preview exists, sprite-sheet import is real, and frame lists can be edited, but there is no visual metasprite canvas, no timeline-style editor, no onion skin, no anchor/hitbox overlay workflow, and no real frame reordering UI. |
| Visual dialogue and event graph editor | Open | Not implemented yet | The data model exists and dialogues/scripts are browseable, but the editor itself explicitly says these are still authored in files. |
| One-click play mode with debug overlays | Complete | Strong | The in-editor sandbox supports start modes, play/pause, frame stepping, restart, slow motion, overlays, and ROM build/launch actions. This is one of the best-finished areas. |
| Physics and feel tuning sandbox | Complete | Strong | Preset selection, live editing, duplication, reset-to-template, movement traces, and fast playtest restart are all there. The only limitation is that compare is sequential rather than side-by-side. |
| Diagnostics, budgets, and quick-fix center | Complete | Strong | Dedicated diagnostics UI, grouping/filter/search, budget bars, navigation hooks, and multiple quick fixes are implemented and feel genuinely useful. |

## Where The Feature List Is Slightly Optimistic

- The docking/layout work is good, but I would describe it as a polished multi-pane layout system rather than fully dockable/floating pro-app windows.
- The animation tooling is better described as preview plus list-based frame editing than as a true visual animation editor.
- I found a help string claiming imported animations can be reordered in the inspector, but I did not find actual frame reordering logic.

## Codebase-Wide Strengths

- The crate split is sensible: project schema, validator, export, platformer preview, events, CLI, and editor are separated in a clean way.
- The validator/export path is one of the healthiest parts of the repository. It is strict, understandable, and actually exercised by the sample project.
- The playtest and physics work is more than placeholder UI. There is real simulation logic behind it.
- Workspace persistence is thoughtfully implemented, including saved layouts, favorites, snippets, and brushes.

## Codebase-Wide Risks

- `crates/snesmaker-editor/src/main.rs` is very large at roughly 9.6k lines. A lot of the product is working, but maintainability is becoming a real risk.
- Automated tests pass, but test coverage is still fairly light for a UI-heavy editor. Most interaction-heavy workflows are not deeply protected by tests.
- `crates/snesmaker-assets` appears disconnected from the main CLI/editor path right now. Asset import logic is effectively living in the editor instead of being shared cleanly.
- The schema is a bit ahead of the implemented runtime/tooling in places. For example, the project model includes broader genre/event concepts, while the codebase still centers on the side-scroller path.

## Highest-Value Next Improvements

1. Build a true visual metasprite/animation authoring workflow.
2. Add a real node-based dialogue/event editor.
3. Turn snippets/brushes into real prefab instances with overrides and browser-to-canvas placement.
4. Add auto-tiling or adjacency rules for common terrain patterns.
5. Refactor the editor into smaller modules and move shared asset import behavior into `snesmaker-assets`.

## Bottom Line

The implementation quality is better than “prototype only,” especially in layouting, outliner flow, diagnostics, playtest, and export. The repo already feels like a serious editor foundation.

The gap between the current product and a truly polished “maker” is now mostly in authoring depth, not in core plumbing. In other words: the shell is strong, the pipelines work, and the next wins should focus on visual content creation tools rather than more scaffolding.
