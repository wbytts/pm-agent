#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceType {
    Extension,
    Skill,
    Prompt,
    Theme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCollision {
    pub resource_type: ResourceType,
    pub name: String,
    pub winner_path: String,
    pub loser_path: String,
    pub winner_source: Option<String>,
    pub loser_source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceDiagnosticKind {
    Warning,
    Error,
    Collision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDiagnostic {
    pub kind: ResourceDiagnosticKind,
    pub message: String,
    pub path: Option<String>,
    pub collision: Option<ResourceCollision>,
}
