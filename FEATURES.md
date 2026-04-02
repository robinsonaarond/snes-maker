# SNES Maker Active Feature Backlog

This file is the implementation-facing todo list for unfinished work identified in `FEATURE-IMPROVEMENTS.md`.

Use this as the Codex roadmap:

1. Work from top to bottom.
2. Finish one feature area before starting the next unless a small dependency unblock is required.
3. Do not reimplement already-solid foundation work unless a task below explicitly calls for it.
4. Keep the sample project, validation flow, playtest sandbox, and ROM export path working after each change.
5. When a task lands, update tests and remove any outdated UI/help text that no longer matches the product.

## Already Solid Foundation

These areas are in good shape and should not be treated as primary backlog items right now:

- Workspace presets, split panes, saved layouts, and layout persistence
- Scene outliner, layer visibility/locking/reordering, and scene object focus tools
- Diagnostics, build budgets, and several quick-fix actions
- In-editor playtest sandbox with overlays, stepping, restart, and physics tuning
- Core tilemap editing basics, collision painting, brushes, snippets, and clipboard stamping
- Validation, CLI workflows, ROM export, and sample-project build flow

## P0: Visual Metasprite And Animation Authoring

Goal: turn the current preview-plus-frame-list workflow into a real visual editor.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-project/src/lib.rs`
new modules under `crates/snesmaker-editor/src/`

- [x] Move animation and metasprite editor code out of `crates/snesmaker-editor/src/main.rs` into focused editor modules.
- [x] Add a metasprite canvas for selecting, placing, deleting, moving, nudging, and flipping sprite pieces.
- [x] Add piece inspector controls for tile index, x/y offset, flip flags, palette slot, and priority.
- [x] Add animation timeline actions for add frame, duplicate frame, delete frame, and reorder frame.
- [x] Add playback controls for play/pause, loop preview, frame scrub, and playback speed.
- [x] Add preview overlays for facing direction, anchor point, and hitbox.
- [x] Persist visual edits back to `.metasprite.ron` and `.animation.ron` resources.
- [x] Add regression coverage for sprite-sheet import -> edit -> save round trips where feasible.

Done when:
users can visually author metasprites, reorder animation frames, preview important overlays, and save those edits without hand-editing files.

## P0: Visual Dialogue And Event Graph Editor

Goal: replace the current file-driven dialogue/script workflow with in-editor graph authoring.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-events/src/lib.rs`
`crates/snesmaker-project/src/lib.rs`
new modules under `crates/snesmaker-editor/src/`

- [x] Move dialogue and event editing code into focused modules instead of keeping it embedded in `main.rs`.
- [x] Add a node-based graph canvas for dialogue nodes and choice links.
- [x] Add node creation, deletion, rename, and reconnect flows.
- [x] Add inspector/editor controls for speaker, text, commands, choices, and next-node routing.
- [x] Add an event script editor for scene scripts and trigger command sequences.
- [x] Let triggers connect to scripts directly from the inspector and/or outliner without file editing.
- [x] Show validator errors inline on graph nodes, scripts, and trigger bindings.
- [x] Add preview panels for rendered dialogue text, command order, and branch outcomes.
- [x] Persist graph edits back to project resources.
- [x] Add regression tests for dialogue/script round-tripping and validator navigation hooks where feasible.

Done when:
users can create and edit dialogue graphs and event scripts entirely inside the editor, with validation feedback visible in the same workflow.

## P1: Prefabs, Snippets, And Browser Placement

Goal: turn the current reusable-selection system into real reusable assets with placement and overrides.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-editor/src/workspace.rs`
`crates/snesmaker-project/src/lib.rs`
sample/template content under `examples/`

- [x] Define a reusable prefab asset format for grouped scene content such as tiles, entities, triggers, checkpoints, and spawns.
- [x] Convert current scene snippets into first-class prefab assets or into a clearly supported upgraded format.
- [x] Add prefab entries to the asset browser with thumbnails, favorites, and usage metadata.
- [x] Support drag-and-drop from the asset browser into the scene canvas for prefabs, snippets, and tile brushes.
- [x] Support drag-and-drop from the asset browser into relevant inspector fields where that improves workflow.
- [x] Place dropped prefabs as instances rather than raw pasted copies when possible.
- [x] Add prefab instance overrides for position, facing, script id, active state, and one-shot/entity flags.
- [x] Show prefab source and overridden fields clearly in the inspector.
- [x] Ship a starter library of common Mega Man-like room chunks and encounter setups in template/sample content.
- [x] Add regression tests for prefab save/load/instance/override behavior.

Done when:
users can save reusable content, place it directly from the browser, and adjust safe per-instance overrides without breaking the source asset.

## P1: Auto-Tiling And Adjacency Rules

Goal: remove repetitive manual terrain cleanup for common tile patterns.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-project/src/lib.rs`
possibly new shared rule types in `crates/snesmaker-project/`

- [x] Define adjacency-rule data structures for tilesets or editor-side rule sets.
- [x] Support rules for terrain edges, ladders, and hazard borders first.
- [x] Apply adjacency rules during paint, erase, fill, and paste operations.
- [x] Add a rebuild-selected-region action for applying rules to existing map sections.
- [x] Add preview/debug UI so users can understand why a tile variant was chosen.
- [x] Add tests for rule evaluation on representative tile neighborhoods.

Done when:
users can paint common terrain types quickly and the editor resolves neighboring tile variants automatically with predictable results.

## P1: Shared Asset Import Pipeline

Goal: move sprite import behavior out of the editor shell and into reusable shared code.

Likely touchpoints:
`crates/snesmaker-assets/src/lib.rs`
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-cli/src/main.rs`

- [x] Move shared PNG and sprite-sheet import logic into `crates/snesmaker-assets`.
- [x] Refactor the editor to use the shared import pipeline instead of keeping parallel logic in the UI layer.
- [x] Expose a reusable non-UI API that the CLI can call later, even if a CLI import command is not added immediately.
- [x] Centralize imported asset id generation, duplicate handling, and validation behavior.
- [x] Remove or update misleading UI/help text that claims functionality not yet implemented.
- [x] Add tests for import validation, asset generation, and duplicate-id handling.

Done when:
sprite import behavior lives in shared code, editor import flows stay working, and import rules are tested outside the UI.

## P2: Editor Modularization And Regression Coverage

Goal: reduce maintenance risk as the editor grows.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
new modules under `crates/snesmaker-editor/src/`
`docs/architecture.md`

- [x] Split `crates/snesmaker-editor/src/main.rs` into modules for workspace, outliner, assets, animation, import, playtest, diagnostics, and scene canvas.
- [x] Keep behavior stable while moving code; avoid feature changes that are unrelated to the extraction.
- [x] Move shared editor persistence helpers and view models into focused files.
- [x] Add regression tests for workspace persistence, snippets/brushes, playtest reset/start modes, and diagnostic quick-fix flows.
- [x] Update `docs/architecture.md` if editor module boundaries or crate responsibilities change materially.

Done when:
the editor is easier to extend without one giant file becoming the bottleneck, and core workflows have better regression protection.

## Stretch Goals

- [ ] Add side-by-side physics preset comparison instead of only sequential swapping.
- [ ] Revisit the custom docking system and decide whether true floating/detachable windows are worth the complexity.
