pub mod widgets;

use std::path::PathBuf;

/// Placeholder for Phase 3 TUI implementation.
pub async fn run(_workspace_root: PathBuf) -> anyhow::Result<()> {
    println!("TUI dashboard is not yet implemented.");
    println!("Use CLI commands instead: forge new, forge status, forge spawn, forge kill");
    Ok(())
}
