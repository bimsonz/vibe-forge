use crate::domain::plan::{Plan, PlanStatus};
use crate::error::VibeError;
use crate::infra::state::StateManager;
use std::path::Path;

pub async fn create(
    workspace_root: &Path,
    title: String,
    session: Option<String>,
) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    if !state_manager.is_initialized() {
        return Err(VibeError::NotInitialized);
    }

    let plans_dir = state_manager.plans_dir();
    let plan = Plan::new(title.clone(), session, &plans_dir);

    let body = format!("# {title}\n\n## Goals\n\n- \n\n## Steps\n\n1. \n");
    plan.save(&body)?;

    println!("Plan created: {}", plan.title);
    println!("  File: {}", plan.file_path.display());
    println!("  Status: {}", plan.status);
    println!("\nEdit the file to flesh out your plan, then share the path with agents.");
    Ok(())
}

pub async fn list(workspace_root: &Path) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    if !state_manager.is_initialized() {
        return Err(VibeError::NotInitialized);
    }

    let plans_dir = state_manager.plans_dir();
    let plans = Plan::load_all(&plans_dir);

    if plans.is_empty() {
        println!("No plans found. Create one with: vibe plan new \"My plan title\"");
        return Ok(());
    }

    println!("Plans:");
    for plan in &plans {
        let icon = match plan.status {
            PlanStatus::Draft => "◐",
            PlanStatus::Active => "●",
            PlanStatus::Completed => "✓",
            PlanStatus::Superseded => "▪",
        };
        let session_info = plan
            .session_name
            .as_deref()
            .map(|s| format!(" (session: {s})"))
            .unwrap_or_default();
        println!(
            "  {icon} {} [{}]{session_info}",
            plan.title, plan.status,
        );
        println!("    {}", plan.file_path.display());
    }
    Ok(())
}

pub async fn view(workspace_root: &Path, query: String) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    if !state_manager.is_initialized() {
        return Err(VibeError::NotInitialized);
    }

    let plans_dir = state_manager.plans_dir();
    let plans = Plan::load_all(&plans_dir);

    // Find by title substring or UUID prefix
    let query_lower = query.to_lowercase();
    let found = plans.iter().find(|p| {
        p.title.to_lowercase().contains(&query_lower)
            || p.id.to_string().starts_with(&query_lower)
            || p.file_path
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().contains(&query_lower))
    });

    match found {
        Some(plan) => {
            let (_, body) = Plan::load(&plan.file_path)?;
            println!("Title:   {}", plan.title);
            println!("Status:  {}", plan.status);
            if let Some(ref s) = plan.session_name {
                println!("Session: {s}");
            }
            println!("File:    {}", plan.file_path.display());
            println!("Created: {}", plan.created_at.format("%Y-%m-%d %H:%M"));
            println!("Updated: {}", plan.updated_at.format("%Y-%m-%d %H:%M"));
            println!("\n---\n");
            println!("{body}");
        }
        None => {
            return Err(VibeError::User(format!("No plan matching '{query}'")));
        }
    }
    Ok(())
}

pub async fn copy(workspace_root: &Path, query: String) -> Result<(), VibeError> {
    let state_manager = StateManager::new(workspace_root);
    if !state_manager.is_initialized() {
        return Err(VibeError::NotInitialized);
    }

    let plans_dir = state_manager.plans_dir();
    let plans = Plan::load_all(&plans_dir);

    let query_lower = query.to_lowercase();
    let found = plans.iter().find(|p| {
        p.title.to_lowercase().contains(&query_lower)
            || p.id.to_string().starts_with(&query_lower)
            || p.file_path
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().contains(&query_lower))
    });

    match found {
        Some(plan) => {
            let (_, body) = Plan::load(&plan.file_path)?;
            crate::infra::clipboard::copy_text(&body)?;
            println!("Plan '{}' copied to clipboard.", plan.title);
        }
        None => {
            return Err(VibeError::User(format!("No plan matching '{query}'")));
        }
    }
    Ok(())
}
