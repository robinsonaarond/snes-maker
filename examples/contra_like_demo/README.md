# Contra Like Demo

`contra_like_demo` is a custom run-and-gun sample project for SNES Maker's current side-scroller runtime.

Objective: move right through the jungle outpost, clear the patrol line, touch the red gate switches, and reach the extraction zone at the far end of the map.

Included art and content:

- Custom 16-color palette
- Custom background tileset
- Player, grunt, dog, turret, pickup, gate, sandbag, and crate metasprites
- Preview sheets in `content/sprite_sources/contra_sheet.png` and `content/sprite_sources/stage_preview.png`
- Dialogue and trigger content for the editor workflow

Current runtime note:

The shipped ROM path now uses exported tile collision, shared physics values, ladder tiles, and configurable angled firing. Enemy logic, scripted encounters, and weapon pickups are still much simpler than real Contra, so this demo is strongest as a controller-and-level-flow slice rather than a fully faithful adaptation.
