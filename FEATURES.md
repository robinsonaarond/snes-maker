# Recommended Features for SNES Maker

This codebase already has a strong milestone-one foundation: a native editor shell, scene painting, collision painting, object placement, sprite-sheet import, tile pixel editing, validation, and ROM export. The biggest opportunity now is turning that foundation into a much more fluid maker-style editor.

From the current UI, the main gaps are discoverability, asset workflow, and editor ergonomics. The app already knows about scenes, layers, palettes, tilesets, metasprites, animations, dialogues, triggers, scripts, and physics presets, but the UI only exposes part of that model. The recommendations below are ordered by impact and deliberately borrow the best ideas from Unity, Unreal, Godot, and MegaMan Maker.

## Implementation Status

- `[x]` Implemented
- `[ ]` Not implemented yet
- Some larger feature areas are now partially complete, so their headings remain open until the whole workflow is there.

## Top 10 Recommended Feature Improvements

### 1. Dockable workspace with saved editor layouts

Right now the editor is effectively a fixed left panel, center canvas, and long right-side inspector. A dockable workspace would make the tool feel dramatically more professional and let different tasks have different layouts.

- [x] Add workspace presets for `Level Design`, `Animation`, `Eventing`, and `Debug`.
- [x] Add toggleable/floating windows for Scene Outliner, Asset Browser, and Diagnostics.
- [ ] Add true dockable tabs for Scene, Inspector, Assets, Animation, Diagnostics, Build Report, and Playtest.
- [ ] Let users save named layouts such as `Level Design`, `Animation`, and `Eventing`.
- [ ] Support split views and layout persistence so designers can keep the scene, asset browser, and preview open at the same time.

This is the most direct quality-of-life win and takes strong inspiration from Unity, Unreal, and Godot.

### 2. Scene outliner and layer manager

The data model already supports multiple tile layers plus scene objects, but the current editing flow is mostly canvas-first and effectively focused on the first layer. A proper outliner would make scenes much easier to understand and scale.

- [x] Add a scene tree that lists layers, spawns, checkpoints, entities, triggers, and scripts in one place.
- [x] Add visibility, lock, reorder, and rename controls for layers.
- [x] Make selection in the outliner sync with the canvas and inspector.
- [ ] Add solo controls for layers and object groups.
- [ ] Add quick actions like duplicate, focus, and isolate.

This should feel like a blend of the Unity Hierarchy, Godot Scene dock, and the fast room-organization mindset from MegaMan Maker.

### 3. Content browser and asset library

The editor currently surfaces tiles and animations, but most asset types still feel hidden behind files or one-off inspectors. A visual content browser would make the whole app more learnable and much faster to use.

- [x] Add filterable browsing for scenes, palettes, tilesets, metasprites, animations, dialogues, scripts, and imported sprite sheets.
- [ ] Add thumbnail browsing for palettes, tilesets, metasprites, animations, dialogues, scripts, and imported sprite sheets.
- [ ] Add favorites and usage metadata so users can answer questions like “where is this metasprite used?”
- [ ] Support drag-and-drop from the browser into the canvas and inspector.

This is one of the clearest Unreal-style upgrades the app could make, with familiar parallels to the Godot filesystem dock and Unity project browser.

### 4. Reusable prefabs, archetypes, and room chunks

SNES-style games repeat enemy setups, hazard patterns, checkpoint clusters, and room fragments constantly. The app should let users save and reuse those patterns instead of rebuilding them by hand.

- Let users save a tile/object selection as a reusable prefab or chunk.
- Support instance overrides for things like position, facing, trigger script, or active state.
- Ship a starter library of common Mega Man-like building blocks such as doorways, enemy pods, moving-platform setups, and checkpoint rooms.

This borrows the best ideas from Unity prefabs, Godot reusable scenes, and MegaMan Maker’s fast copy-and-remix workflow.

### 5. Advanced tilemap authoring tools

The current tile workflow is functional, but it is still much closer to a prototype editor than a polished level-building tool. Tile authoring is where a maker rises or falls.

- [x] Add an eyedropper-style tile sampler on the active layer.
- [x] Add bulk selection actions for fill/clear and solid, ladder, and hazard edits.
- [ ] Add box fill, line, flood fill, mirror painting, and stamp brushes.
- [ ] Support multi-tile brush presets and reusable terrain stamps.
- [ ] Add auto-tiling or adjacency rules for terrain edges, ladders, hazard borders, and other repeated patterns.

This should lean heavily on Godot’s TileMap ergonomics and MegaMan Maker’s speed-first editing feel.

### 6. Visual metasprite and animation editor

Animations can already be sequenced, but metasprites themselves are not yet authored visually in the editor. That leaves one of the most important content pipelines feeling incomplete.

- Add a metasprite canvas for placing, flipping, nudging, and aligning sprite pieces visually.
- Add an animation timeline with duplicate frame, onion-skin, playback speed, loop preview, and frame reordering tools.
- Add preview modes for facing direction, palette slot changes, hitboxes, and anchor points.

This would make the app feel much closer to Unity’s animation tooling, Godot’s 2D animation workflow, and the hands-on immediacy creators expect from maker tools.

### 7. Visual dialogue and event graph editor

The project format already supports dialogue graphs and event scripts, but the editor still tells users to author them in files. That is one of the biggest current UX gaps.

- Add a node-based graph editor for dialogue, branching choices, trigger flow, and event commands.
- Let users connect triggers directly to scripts and see validation errors inline on the graph.
- Add preview panels for speaker text, command sequences, and branch outcomes.

This is the feature that would most clearly channel Unreal Blueprints and make narrative or scripted content far more approachable.

### 8. One-click play mode with debug overlays

The editor can validate and build ROMs, but the iteration loop is still heavier than it needs to be. A strong maker needs an instant “try it now” button.

- Add an in-editor playtest mode that starts from the current scene, selected spawn, or selected checkpoint.
- Add overlays for collision, camera bounds, trigger activation, spawn points, checkpoints, and entity state.
- Add frame-step, slow motion, and quick restart so users can tune feel and placement without rebuilding constantly.

This should take cues from Unity Play Mode, Unreal PIE, and Godot’s lightweight scene testing workflow.

### 9. Physics and feel tuning sandbox

Physics presets already exist in the project model, but the editor mostly exposes player HUD settings today. For a platformer-focused tool, movement tuning should be a first-class workflow.

- Add a dedicated physics preset editor with duplication, compare, and reset-to-template actions.
- Visualize jump arcs, acceleration curves, coyote time, and ladder speed instead of hiding them in raw numbers.
- Let users hot-swap a preset into play mode instantly to compare “Mega Man-like” versus “Mario-like” feel.

This is a very Godot/Unity-style improvement that would make SNES Maker feel much more serious for gameplay tuning.

### 10. Diagnostics, budgets, and quick-fix center

Validation already exists, and that is a major strength of the codebase, but the current diagnostics UI is still just a flat list. A dedicated budget-and-errors center would turn strict SNES constraints into a usable design tool.

- [x] Add a dedicated diagnostics workspace window.
- [ ] Add filters, grouping, searchable codes, and clickable navigation back to the offending asset or scene.
- [ ] Visualize tile, palette, metasprite, and ROM-bank budgets with progress bars and warning thresholds.
- [ ] Add quick-fix actions for common issues like duplicate ids, missing references, oversized palettes, or invalid entry scenes.

This would borrow from the Unity Console, Unreal Message Log, and Godot’s warning UX while staying grounded in SNES-specific budgets.

## Best First Wave

If the goal is to make the app feel dramatically better without boiling the ocean, I would build these first:

1. Dockable workspace with saved layouts: started
2. Scene outliner and layer manager: started
3. Content browser and asset library: started
4. Advanced tilemap authoring tools: started
5. One-click play mode with debug overlays

That sequence would improve the everyday editing loop immediately, then create the right foundation for deeper authoring features like the metasprite editor, event graph, and prefab library.
