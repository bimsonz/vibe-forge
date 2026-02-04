use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// A Plan is a shared document between agents.
/// Lives at .forge/plans/{id}.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub title: String,
    pub session_id: Uuid,
    pub file_path: PathBuf,
    pub status: PlanStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
    Superseded,
}

impl std::fmt::Display for PlanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "Draft"),
            Self::Active => write!(f, "Active"),
            Self::Completed => write!(f, "Completed"),
            Self::Superseded => write!(f, "Superseded"),
        }
    }
}

impl Plan {
    pub fn new(title: String, session_id: Uuid, plans_dir: &PathBuf) -> Self {
        let id = Uuid::new_v4();
        let file_path = plans_dir.join(format!("{id}.md"));
        Self {
            id,
            title,
            session_id,
            file_path,
            status: PlanStatus::Draft,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
