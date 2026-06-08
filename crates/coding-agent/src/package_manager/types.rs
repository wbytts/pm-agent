use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceScope {
    User,
    Project,
    Temporary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceOrigin {
    Package,
    TopLevel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMetadata {
    pub source: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    pub base_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResource {
    pub path: String,
    pub enabled: bool,
    pub metadata: PathMetadata,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedPaths {
    pub extensions: Vec<ResolvedResource>,
    pub skills: Vec<ResolvedResource>,
    pub prompts: Vec<ResolvedResource>,
    pub themes: Vec<ResolvedResource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingSourceAction {
    Install,
    Skip,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressAction {
    Install,
    Remove,
    Update,
    Clone,
    Pull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEventKind {
    Start,
    Progress,
    Complete,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressEvent {
    pub kind: ProgressEventKind,
    pub action: ProgressAction,
    pub source: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmCommandConfig {
    pub command: String,
    pub args: Vec<String>,
}

impl Default for NpmCommandConfig {
    fn default() -> Self {
        Self {
            command: "npm".to_string(),
            args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCommandStep {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageOperationPlan {
    pub action: ProgressAction,
    pub source: String,
    pub steps: Vec<PackageCommandStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageUpdate {
    pub source: String,
    pub display_name: String,
    pub kind: PackageKind,
    pub scope: SourceScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Npm,
    Git,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredPackage {
    pub source: String,
    pub scope: SourceScope,
    pub filtered: bool,
    pub installed_path: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PackageFilter {
    pub extensions: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub themes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpmSource {
    pub spec: String,
    pub name: String,
    pub pinned: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSource {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedSource {
    Npm(NpmSource),
    Git(crate::utils::git::GitSource),
    Local(LocalSource),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResourceType {
    Extension,
    Skill,
    Prompt,
    Theme,
}
