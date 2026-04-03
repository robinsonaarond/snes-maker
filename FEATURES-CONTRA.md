# Contra Adaptation Roadmap

This file is the implementation-facing roadmap for turning SNES Maker into something that can build a much more faithful Contra-style action game.

Use this as the Codex roadmap:

1. Work from top to bottom.
2. Keep the editor, validator, sample projects, and ROM export path working after each slice.
3. Prefer small vertical slices that improve exported gameplay immediately.
4. Do not fake faithfulness in sample content when the runtime does not support it yet.
5. When a slice lands, update sample content, tests, and outdated UI/help text so the product story stays honest.

## Current Baseline

The current exported ROM path can already support:

- One player character with left/right movement, jump, and horizontal shooting
- Scrolling side-view stages
- Basic pickups, patrol enemies, switches, and solid blocker entities
- Simple HUD output and sample-project ROM export

The current exported ROM path is still missing the key things that make Contra feel like Contra:

- Real tile-driven collision and hazard handling parity with authored scene collision
- A richer player state machine with aim directions, crouch/prone, and weapon behaviors
- Enemy bullets, spawn waves, scripted encounters, destructibles, and bosses
- Runtime trigger execution, camera locks, checkpoints, and stage flow scripting
- Better animation, rendering layers, and audio support
- Co-op support

## Landed Foundation

These groundwork steps are already implemented and should be treated as shipped progress rather than future ideas:

- [x] Export collision flags derived from scene collision into generated ROM data.
- [x] Add runtime tile queries for solid and hazard checks instead of relying only on a single hardcoded ground line.
- [x] Resolve player movement against exported tile flags for basic horizontal and vertical blocking.
- [x] Apply hazard damage from exported collision data while respecting invulnerability frames.
- [x] Export shared `PhysicsProfile` values into generated ROM data and use them in the runtime controller.
- [x] Support ladders in the exported ROM runtime.
- [x] Support configurable aim modes plus per-project projectile count, speed, and fire cadence settings.
- [x] Keep sample ROM builds and automated tests passing after the collision/runtime slice.

Relevant touchpoints:
`crates/snesmaker-export/src/lib.rs`
`runtime/snes/main.s`

Done when:
the exported ROM can use authored scene collision as the foundation for movement and hazard response instead of behaving like a flat-ground prototype.

## P0: Shared World And Physics Foundation

Goal: make the exported ROM follow authored world data closely enough that level design matters.

Likely touchpoints:
`crates/snesmaker-export/src/lib.rs`
`runtime/snes/main.s`
`crates/snesmaker-platformer/src/lib.rs`
`crates/snesmaker-project/src/lib.rs`

- [x] Export collision flags for solid, ladder, and hazard data into generated project data.
- [x] Add runtime tile lookup helpers and basic solid/hazard collision checks.
- [ ] Remove remaining hardcoded movement assumptions that bypass authored collision.
- [x] Align exported runtime movement rules with `PhysicsProfile` data instead of hardcoded jump/gravity constants.
- [x] Support ladders in the exported ROM runtime.
- [ ] Support slopes only if the editor/runtime data model can stay simple; otherwise defer explicitly.
- [ ] Add pit, ceiling, and wall edge-case tests so collision changes do not regress existing demos.
- [ ] Export and use true world dimensions and camera constraints rather than heavily resampling layout intent.

Done when:
scene collision, hazards, and movement tuning authored in project data produce meaningfully similar behavior in playtest and exported ROMs.

## P0: Contra Player Controller And Weapons

Goal: add the player verbs that define the feel of Contra.

Likely touchpoints:
`runtime/snes/main.s`
`crates/snesmaker-export/src/lib.rs`
`crates/snesmaker-project/src/lib.rs`
`crates/snesmaker-editor/src/playtest_panel.rs`
`crates/snesmaker-platformer/src/lib.rs`

- [ ] Add crouch and prone states to the exported player controller.
- [x] Add aiming directions for straight, up, down-diagonal, and up-diagonal firing.
- [x] Allow jump-shooting with aim-direction support.
- [x] Replace the fixed three-bullet limit with configurable projectile budgets as the first step toward weapon-driven caps.
- [ ] Add weapon definitions for default, spread, laser, flame, and machine-gun style fire.
- [ ] Add pickup-driven weapon swaps and optional power-loss behavior on death.
- [ ] Support per-weapon fire cadence, projectile speed, range, and hit behavior.
- [ ] Add player death, respawn, and checkpoint restore rules that feel appropriate for a run-and-gun.
- [ ] Add exported tests or deterministic validation fixtures for controller state transitions and weapon limits.

Done when:
the player can move, aim, fire, die, and recover in a way that feels recognizably Contra-like instead of “generic platform shooter.”

## P1: Enemy Behaviors, Projectiles, And Destructibles

Goal: move from static patrol props to authored encounters.

Likely touchpoints:
`runtime/snes/main.s`
`crates/snesmaker-project/src/lib.rs`
`crates/snesmaker-export/src/lib.rs`
sample content under `examples/`

- [ ] Expand entity behavior data beyond `None` and horizontal `Patrol`.
- [ ] Add enemy projectile support in the exported runtime.
- [ ] Add enemy states for pop-up turrets, jumpers, flyers, rushers, and destructible emplacements.
- [ ] Support spawn waves that activate from camera position or trigger volumes.
- [ ] Add off-screen activation/despawn rules so encounters feel paced rather than overcrowded.
- [ ] Add destructible props that can gate progress or hide pickups.
- [ ] Add one or two boss-friendly state machines or scripted hooks instead of trying to encode every boss as generic patrol logic.
- [ ] Update sample content to include at least one full encounter pattern and one mini-boss or boss gate.

Done when:
encounters can be authored as waves and behaviors instead of as scattered patrol enemies with no projectile pressure.

## P1: Runtime Triggers, Camera Locks, And Stage Flow

Goal: make authored scripts and trigger volumes matter in the exported ROM.

Likely touchpoints:
`crates/snesmaker-events/src/lib.rs`
`crates/snesmaker-project/src/lib.rs`
`crates/snesmaker-export/src/lib.rs`
`runtime/snes/main.s`

- [ ] Export trigger volumes and compiled event data into ROM-friendly tables.
- [ ] Execute a useful runtime subset of event commands in exported builds.
- [ ] Support scripted enemy spawns, gate locks, unlocks, and simple dialogue/callout beats.
- [ ] Add camera lock regions for encounter arenas and boss intros.
- [ ] Add checkpoints and scene/stage progression hooks that work in exported ROMs.
- [ ] Make switch/gate content use the same runtime trigger model as other stage scripting instead of special-case behavior where possible.
- [ ] Add regression coverage for trigger entry, one-shot behavior, checkpoint restore, and camera lock release.

Done when:
the exported game can stage ambushes, lock the player into fights, and progress through a level using authored triggers rather than only static geometry.

## P1: Animation, Rendering, And Audio

Goal: close the presentation gap between a debug runtime and a convincing action game.

Likely touchpoints:
`runtime/snes/main.s`
`crates/snesmaker-export/src/lib.rs`
`crates/snesmaker-project/src/lib.rs`
asset content under `examples/`

- [ ] Support richer animation selection per player and enemy state.
- [ ] Add muzzle flashes, hit effects, explosions, and death animations.
- [ ] Improve sprite/palette assignment so multiple enemy and effect families can coexist cleanly.
- [ ] Support more than a single useful background layer from exported scene data.
- [ ] Add parallax or foreground/occlusion support where the engine budget allows.
- [ ] Add a basic music and SFX path for shots, hits, pickups, explosions, and stage clear moments.
- [ ] Update sample content so visual polish reflects newly available runtime features.

Done when:
the exported build reads as a deliberate action game presentation instead of a functional engine proof-of-concept.

## P2: Editor Tooling For Contra-Style Content

Goal: let users author the new gameplay systems without falling back to hand-editing data files.

Likely touchpoints:
`crates/snesmaker-editor/src/`
`crates/snesmaker-project/src/lib.rs`
`crates/snesmaker-events/src/lib.rs`

- [ ] Add editor inspectors for weapon definitions, projectile settings, and enemy behavior presets.
- [ ] Add authoring support for encounter waves, spawn triggers, and boss gates.
- [ ] Improve scene visualization for collision, hazards, trigger volumes, and camera lock regions.
- [ ] Add validation messaging that explains when content depends on runtime features not yet exported.
- [ ] Ship a polished Contra-like example that exercises the supported systems honestly.

Done when:
users can author a compelling Contra-like stage inside the editor without needing to reverse-engineer runtime tables by hand.

## P2: Co-op

Goal: add optional two-player support after the single-player loop feels right.

Likely touchpoints:
`runtime/snes/main.s`
`crates/snesmaker-export/src/lib.rs`
`crates/snesmaker-project/src/lib.rs`
`crates/snesmaker-editor/src/playtest_panel.rs`

- [ ] Add a second player runtime state, input handling, spawn logic, and camera rules.
- [ ] Decide whether exported co-op uses shared lives, separate lives, or configurable rules.
- [ ] Update encounters, projectile budgets, and performance constraints for two players.
- [ ] Add sample content and tests that cover join/rejoin, respawn, and camera edge cases.

Done when:
co-op is a supported mode rather than a fragile afterthought layered on top of a single-player runtime.

## Next Recommended Slice

After the physics/ladder/aiming groundwork, the next highest-impact slice is:

- [ ] add crouch and prone states with meaningful gameplay differences
- [ ] add pickup-driven weapon definitions instead of project-wide projectile settings
- [ ] begin enemy projectile and encounter-spawn support so level pressure feels like Contra

That combination should move the runtime from “better-feeling controller” into “recognizable Contra encounter loop.”
