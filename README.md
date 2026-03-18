# snes-maker

Rust-first tooling for building 2D games that export to SNES-compatible ROMs.

This repository now contains the initial milestone scaffold:

- A Cargo workspace for the editor, CLI, project schema, validator, asset pipeline, platformer preview, and ROM exporter.
- A strict project format built around `project.toml` plus human-readable `content/**/*.ron` files.
- A sample Mega Man-like side-scroller project in `examples/megaman_like_demo`.
- A ROM exporter that stages a shared SNES runtime, generates build reports, and invokes `ca65`/`ld65` when available.

## Workspace

- `crates/snesmaker-cli`: command-line entry point with `new`, `check`, `build-rom`, and `run`.
- `crates/snesmaker-editor`: native desktop editor shell built with `egui` and `eframe`.
- `crates/snesmaker-project`: project manifest, content schema, loading, and template generation.
- `crates/snesmaker-validator`: strict SNES constraint validation and build budgets.
- `crates/snesmaker-assets`: PNG import helpers and simple asset conversion primitives.
- `crates/snesmaker-platformer`: deterministic side-scroller preview physics and scene compilation helpers.
- `crates/snesmaker-events`: dialogue graphs and shared event IR.
- `crates/snesmaker-export`: ROM export orchestration, assembler invocation, and build report generation.
- `runtime/snes`: shared SNES runtime assets assembled into the final ROM.

## Quick Start

```bash
cargo run -p snesmaker-cli -- new my_game
cargo run -p snesmaker-cli -- check examples/megaman_like_demo
cargo run -p snesmaker-cli -- build-rom examples/megaman_like_demo
cargo run -p snesmaker-editor -- examples/megaman_like_demo
```

`build-rom` writes generated assets and a `build-report.json` even when `ca65` or `ld65` are not installed. To produce a real `.sfc`, install the `cc65` toolchain so those binaries are available on `PATH`.

## Current Status

This is a serious milestone-one foundation, not a finished maker yet. The editor shell, project model, validation, and export wiring are in place, while deeper tile editing, richer physics presets, RPG tooling, and a verified gameplay runtime remain ahead.
