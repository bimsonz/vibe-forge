# Vibe - Project Context

## Build

```sh
cargo build              # debug build
cargo test               # run all tests
cargo build --release    # optimized build
cargo install --path .   # install to ~/.cargo/bin
```

## Structure

- `src/cli.rs` - CLI command definitions (clap derive)
- `src/config.rs` - Global (~/.config/vibe/) and workspace (.vibe/) configuration
- `src/main.rs` - Entry point, workspace root resolution, command dispatch
- `src/error.rs` - Error types (VibeError via thiserror)
- `src/commands/` - Command implementations (init, new, spawn, kill, status, doctor, etc.)
- `src/domain/` - Core entities: Session, Agent, Workspace, Template, Plan
- `src/infra/` - Infrastructure: TmuxController, git ops, Claude CLI, StateManager, file watcher
- `src/tui/` - Terminal UI: ratatui dashboard, widgets, event loop, app state

## Patterns

- Async runtime: tokio with full features
- tmux interaction: shells out to `tmux` CLI via `tokio::process::Command`
- State: serialized as JSON to `.vibe/workspace.json`, loaded/saved via `StateManager`
- Templates: markdown with `+++` TOML frontmatter delimiters
- Backwards compat: new fields use `#[serde(default)]` so old state files deserialize correctly
- Multi-repo: `WorkspaceKind::SingleRepo` (default) or `MultiRepo`, detected by `discover_repos()`

## Naming

The CLI binary is `vibe`. The project directory may still be called `forge` (historical name). Internal code references use "vibe" (e.g., `VibeError`, `VibeWatcher`). The TUI banner reads "VIBE TREE".
