use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PiManifest {
    pub extensions: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub themes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PackageJson {
    pi: Option<PiManifest>,
}

pub(super) fn read_pi_manifest(dir: &Path) -> Option<PiManifest> {
    let content = fs::read_to_string(dir.join("package.json")).ok()?;
    serde_json::from_str::<PackageJson>(&content).ok()?.pi
}
