use crate::diagnostics::{
    ResourceCollision, ResourceDiagnostic, ResourceDiagnosticKind, ResourceType,
};
use agent::harness::Skill;
use std::path::Path;

use crate::resource_loader::paths::display_path;

pub fn warning_diagnostic(message: impl Into<String>, path: &Path) -> ResourceDiagnostic {
    ResourceDiagnostic {
        kind: ResourceDiagnosticKind::Warning,
        message: message.into(),
        path: Some(display_path(path)),
        collision: None,
    }
}

pub fn collision_diagnostic(existing: &Skill, loser: &Skill) -> ResourceDiagnostic {
    ResourceDiagnostic {
        kind: ResourceDiagnosticKind::Collision,
        message: format!("name \"{}\" collision", loser.name),
        path: Some(loser.file_path.clone()),
        collision: Some(ResourceCollision {
            resource_type: ResourceType::Skill,
            name: loser.name.clone(),
            winner_path: existing.file_path.clone(),
            loser_path: loser.file_path.clone(),
            winner_source: None,
            loser_source: None,
        }),
    }
}
