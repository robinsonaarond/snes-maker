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

- [ ] Move animation and metasprite editor code out of `crates/snesmaker-editor/src/main.rs` into focused editor modules.
- [ ] Add a metasprite canvas for selecting, placing, deleting, moving, nudging, and flipping sprite pieces.
- [ ] Add piece inspector controls for tile index, x/y offset, flip flags, palette slot, and priority.
- [ ] Add animation timeline actions for add frame, duplicate frame, delete frame, and reorder frame.
- [ ] Add playback controls for play/pause, loop preview, frame scrub, and playback speed.
- [ ] Add preview overlays for facing direction, anchor point, and hitbox.
- [ ] Persist visual edits back to `.metasprite.ron` and `.animation.ron` resources.
- [ ] Add regression coverage for sprite-sheet import -> edit -> save round trips where feasible.

Done when:
users can visually author metasprites, reorder animation frames, preview important overlays, and save those edits without hand-editing files.

## P0: Visual Dialogue And Event Graph Editor

Goal: replace the current file-driven dialogue/script workflow with in-editor graph authoring.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-events/src/lib.rs`
`crates/snesmaker-project/src/lib.rs`
new modules under `crates/snesmaker-editor/src/`

- [ ] Move dialogue and event editing code into focused modules instead of keeping it embedded in `main.rs`.
- [ ] Add a node-based graph canvas for dialogue nodes and choice links.
- [ ] Add node creation, deletion, rename, and reconnect flows.
- [ ] Add inspector/editor controls for speaker, text, commands, choices, and next-node routing.
- [ ] Add an event script editor for scene scripts and trigger command sequences.
- [ ] Let triggers connect to scripts directly from the inspector and/or outliner without file editing.
- [ ] Show validator errors inline on graph nodes, scripts, and trigger bindings.
- [ ] Add preview panels for rendered dialogue text, command order, and branch outcomes.
- [ ] Persist graph edits back to project resources.
- [ ] Add regression tests for dialogue/script round-tripping and validator navigation hooks where feasible.

Done when:
users can create and edit dialogue graphs and event scripts entirely inside the editor, with validation feedback visible in the same workflow.

## P1: Prefabs, Snippets, And Browser Placement

Goal: turn the current reusable-selection system into real reusable assets with placement and overrides.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-editor/src/workspace.rs`
`crates/snesmaker-project/src/lib.rs`
sample/template content under `examples/`

- [ ] Define a reusable prefab asset format for grouped scene content such as tiles, entities, triggers, checkpoints, and spawns.
- [ ] Convert current scene snippets into first-class prefab assets or into a clearly supported upgraded format.
- [ ] Add prefab entries to the asset browser with thumbnails, favorites, and usage metadata.
- [ ] Support drag-and-drop from the asset browser into the scene canvas for prefabs, snippets, and tile brushes.
- [ ] Support drag-and-drop from the asset browser into relevant inspector fields where that improves workflow.
- [ ] Place dropped prefabs as instances rather than raw pasted copies when possible.
- [ ] Add prefab instance overrides for position, facing, script id, active state, and one-shot/entity flags.
- [ ] Show prefab source and overridden fields clearly in the inspector.
- [ ] Ship a starter library of common Mega Man-like room chunks and encounter setups in template/sample content.
- [ ] Add regression tests for prefab save/load/instance/override behavior.

Done when:
users can save reusable content, place it directly from the browser, and adjust safe per-instance overrides without breaking the source asset.

## P1: Auto-Tiling And Adjacency Rules

Goal: remove repetitive manual terrain cleanup for common tile patterns.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-project/src/lib.rs`
possibly new shared rule types in `crates/snesmaker-project/`

- [ ] Define adjacency-rule data structures for tilesets or editor-side rule sets.
- [ ] Support rules for terrain edges, ladders, and hazard borders first.
- [ ] Apply adjacency rules during paint, erase, fill, and paste operations.
- [ ] Add a rebuild-selected-region action for applying rules to existing map sections.
- [ ] Add preview/debug UI so users can understand why a tile variant was chosen.
- [ ] Add tests for rule evaluation on representative tile neighborhoods.

Done when:
users can paint common terrain types quickly and the editor resolves neighboring tile variants automatically with predictable results.

## P1: Shared Asset Import Pipeline

Goal: move sprite import behavior out of the editor shell and into reusable shared code.

Likely touchpoints:
`crates/snesmaker-assets/src/lib.rs`
`crates/snesmaker-editor/src/main.rs`
`crates/snesmaker-cli/src/main.rs`

- [ ] Move shared PNG and sprite-sheet import logic into `crates/snesmaker-assets`.
- [ ] Refactor the editor to use the shared import pipeline instead of keeping parallel logic in the UI layer.
- [ ] Expose a reusable non-UI API that the CLI can call later, even if a CLI import command is not added immediately.
- [ ] Centralize imported asset id generation, duplicate handling, and validation behavior.
- [ ] Remove or update misleading UI/help text that claims functionality not yet implemented.
- [ ] Add tests for import validation, asset generation, and duplicate-id handling.

Done when:
sprite import behavior lives in shared code, editor import flows stay working, and import rules are tested outside the UI.

## P2: Editor Modularization And Regression Coverage

Goal: reduce maintenance risk as the editor grows.

Likely touchpoints:
`crates/snesmaker-editor/src/main.rs`
new modules under `crates/snesmaker-editor/src/`
`docs/architecture.md`

- [ ] Split `crates/snesmaker-editor/src/main.rs` into modules for workspace, outliner, assets, animation, import, playtest, diagnostics, and scene canvas.
- [ ] Keep behavior stable while moving code; avoid feature changes that are unrelated to the extraction.
- [ ] Move shared editor persistence helpers and view models into focused files.
- [ ] Add regression tests for workspace persistence, snippets/brushes, playtest reset/start modes, and diagnostic quick-fix flows.
- [ ] Update `docs/architecture.md` if editor module boundaries or crate responsibilities change materially.

Done when:
the editor is easier to extend without one giant file becoming the bottleneck, and core workflows have better regression protection.

## Stretch Goals

- [ ] Add side-by-side physics preset comparison instead of only sequential swapping.
- [ ] Revisit the custom docking system and decide whether true floating/detachable windows are worth the complexity.
