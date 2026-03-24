# SNES Maker Architecture

This document captures the current high-level design of the workspace based on the code in `crates/`, the shared `runtime/snes` assets, and the sample project format.

## System Architecture

```mermaid
flowchart TB
    user["Designer / Developer"]

    subgraph surfaces["Application Surfaces"]
        cli["snesmaker-cli<br/>new / check / build-rom / run"]
        editor["snesmaker-editor<br/>egui / eframe desktop editor"]
    end

    subgraph core["Shared Core Crates"]
        project["snesmaker-project<br/>schema, load/save, template generation"]
        events["snesmaker-events<br/>dialogue graph + event script IR"]
        validator["snesmaker-validator<br/>diagnostics, SNES limits, build budgets"]
        platformer["snesmaker-platformer<br/>scene compiler + deterministic playtest sim"]
        export["snesmaker-export<br/>runtime staging, build orchestration, build report"]
        assets["snesmaker-assets<br/>PNG to palette / tileset import helpers"]
    end

    subgraph storage["Project Files And Generated Data"]
        manifest["project.toml<br/>meta, build, editor, gameplay settings"]
        content["content/**/*.ron<br/>scenes, dialogues, palettes, tilesets,<br/>metasprites, animations"]
        sprite_sources["content/sprite_sources/*.png<br/>source sprite sheets"]
        workspace[".snesmaker/*.ron<br/>editor layouts, favorites, snippets, brushes"]
        builddir["build/<br/>generated .inc files, build-report.json,<br/>ROM artifacts"]
        runtime["runtime/snes<br/>main.s + linker.cfg"]
    end

    subgraph external["External Tools"]
        assembler["ca65 / ld65<br/>optional cc65 toolchain"]
        emulator["Emulator<br/>(for example ares)"]
        rom["SNES ROM (.sfc)"]
    end

    user --> cli
    user --> editor

    cli -->|"new / check"| project
    cli -->|"check diagnostics"| validator
    cli -->|"build-rom / run"| export

    editor -->|"load/edit/save project"| project
    editor -->|"diagnostics + quick fixes"| validator
    editor -->|"in-editor playtest + trace tools"| platformer
    editor -->|"ROM build + build report + launcher"| export
    editor <-->|"workspace state"| workspace

    project <--> manifest
    project <--> content
    project --> events

    validator --> project
    validator --> events

    platformer --> project

    export --> project
    export --> validator
    export --> platformer
    export --> runtime
    export --> builddir
    export --> assembler
    assembler --> rom
    rom --> emulator

    assets -->|"reads PNG sprite sheets"| sprite_sources
    assets -->|"produces palette / tileset resources"| project
```

## Main Runtime And Build Flow

```mermaid
sequenceDiagram
    participant UI as Editor or CLI
    participant Project as snesmaker-project
    participant Validator as snesmaker-validator
    participant Export as snesmaker-export
    participant Platformer as snesmaker-platformer
    participant Runtime as runtime/snes
    participant Toolchain as ca65/ld65
    participant Output as build/
    participant Emulator as Emulator

    UI->>Project: Load project.toml + content/**/*.ron
    UI->>Validator: Validate ProjectBundle
    Validator-->>UI: Diagnostics + budget summary

    opt In-editor preview
        UI->>Platformer: Create PlaytestSession / simulate_trace
        Platformer-->>UI: Deterministic preview state
    end

    opt Build or run
        UI->>Project: Save current bundle
        UI->>Export: build_rom(project_root)
        Export->>Project: Reload ProjectBundle
        Export->>Validator: Revalidate bundle
        Export->>Platformer: Compile supported scenes
        Export->>Runtime: Copy main.s + linker.cfg
        Export->>Output: Write generated/header.inc
        Export->>Output: Write generated/project_data.inc
        Export->>Output: Write build-report.json

        alt ca65 and ld65 available
            Export->>Toolchain: Assemble and link
            Toolchain-->>Output: Built .sfc ROM
            Output-->>Emulator: Launch ROM when requested
        else Toolchain missing
            Export-->>UI: Return warnings and generated build assets only
        end
    end
```

## Notes

- `snesmaker-project` is the center of the design. Both entry points and most supporting crates work from `ProjectBundle`.
- `snesmaker-validator` is reused by the CLI, the editor, and the exporter so diagnostics stay consistent across workflows.
- `snesmaker-platformer` serves two roles today: deterministic in-editor playtesting and scene compilation for the current side-scroller runtime path.
- `snesmaker-assets` is a workspace utility crate in this snapshot. It reads PNG sprite sheets and produces `PaletteResource` / `TilesetResource`, but it is not yet on the primary CLI/editor dependency path.
