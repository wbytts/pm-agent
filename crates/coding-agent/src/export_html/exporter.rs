use std::fs;
use std::path::{Path, PathBuf};

use agent::harness::{JsonlSessionStorage, SessionMetadata, SessionStorage, SessionTreeEntry};
use ai::ToolDefinition;
use serde::Serialize;

use crate::export_html::template::render_template;
use crate::export_html::theme::ExportTheme;
use crate::session_manager::SessionManager;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedToolHtml {
    pub call_html: Option<String>,
    pub result_html_collapsed: Option<String>,
    pub result_html_expanded: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    pub output_path: Option<PathBuf>,
    pub theme_name: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub rendered_tools: std::collections::BTreeMap<String, RenderedToolHtml>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExport {
    pub header: SessionMetadata,
    pub entries: Vec<SessionTreeEntry>,
    pub leaf_id: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub rendered_tools: std::collections::BTreeMap<String, RenderedToolHtml>,
    pub theme: String,
}

pub fn generate_session_html<S: SessionStorage>(
    manager: &SessionManager<S>,
    options: &ExportOptions,
) -> Result<String, String> {
    let theme = ExportTheme::resolve(options.theme_name.as_deref());
    let export = session_export(manager, options, &theme)?;
    let json = serde_json::to_string(&export).map_err(|error| error.to_string())?;
    Ok(render_template(&json, &theme))
}

pub fn export_session_to_html<S: SessionStorage>(
    manager: &SessionManager<S>,
    options: ExportOptions,
) -> Result<PathBuf, String> {
    let session_file = manager
        .session_file()
        .ok_or_else(|| "Cannot export in-memory session to HTML".to_string())?;
    if !session_file.exists() {
        return Err("Nothing to export yet - start a conversation first".to_string());
    }

    let html = generate_session_html(manager, &options)?;
    let output_path = output_path(
        options.output_path.as_deref(),
        session_file,
        "pm-agent-session",
    );
    fs::write(&output_path, html).map_err(|error| error.to_string())?;
    Ok(output_path)
}

pub fn export_from_file(
    input_path: impl AsRef<Path>,
    options: ExportOptions,
) -> Result<PathBuf, String> {
    let input_path = input_path.as_ref();
    if !input_path.exists() {
        return Err(format!("File not found: {}", input_path.display()));
    }

    let manager = SessionManager::<JsonlSessionStorage>::open(input_path, None)?;
    let html = generate_session_html(&manager, &options)?;
    let output_path = output_path(
        options.output_path.as_deref(),
        input_path,
        "pm-agent-session",
    );
    fs::write(&output_path, html).map_err(|error| error.to_string())?;
    Ok(output_path)
}

fn session_export<S: SessionStorage>(
    manager: &SessionManager<S>,
    options: &ExportOptions,
    theme: &ExportTheme,
) -> Result<SessionExport, String> {
    Ok(SessionExport {
        header: manager.storage_metadata().clone(),
        entries: manager.entries(),
        leaf_id: manager.leaf_id()?,
        system_prompt: options.system_prompt.clone(),
        tools: options.tools.clone(),
        rendered_tools: options.rendered_tools.clone(),
        theme: theme.name.clone(),
    })
}

fn output_path(explicit: Option<&Path>, source_path: &Path, prefix: &str) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }

    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("session");
    PathBuf::from(format!("{prefix}-{stem}.html"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::AgentMessage;
    use ai::MessageRole;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn generates_html_from_memory_session() {
        let mut manager = SessionManager::in_memory("/tmp/project");
        manager
            .append_message(AgentMessage::new(
                MessageRole::User,
                "hello <world>".to_string(),
            ))
            .expect("message should append");

        let html = generate_session_html(
            &manager,
            &ExportOptions {
                theme_name: Some("light".to_string()),
                ..ExportOptions::default()
            },
        )
        .expect("html should generate");

        assert!(html.contains("PM Agent Session Export"));
        assert!(!html.contains("hello <world>"));
        assert!(html.contains("light"));
        assert!(html.contains("eyJoZWFkZXIi"));
    }

    #[test]
    fn exports_persisted_session_to_file() {
        let dir = temp_dir();
        let out = dir.join("export.html");
        let mut manager =
            SessionManager::create("/tmp/project", Some(dir.clone())).expect("session");
        manager
            .append_message(AgentMessage::new(
                MessageRole::Assistant,
                "saved".to_string(),
            ))
            .expect("message");

        let path = export_session_to_html(
            &manager,
            ExportOptions {
                output_path: Some(out.clone()),
                ..ExportOptions::default()
            },
        )
        .expect("export should succeed");

        assert_eq!(path, out);
        let html = fs::read_to_string(path).expect("html should exist");
        assert!(!html.contains("saved"));
        assert!(html.contains("eyJoZWFkZXIi"));
    }

    #[test]
    fn session_export_includes_available_tools_like_pi() {
        let manager = SessionManager::in_memory("/tmp/project");

        let export = session_export(
            &manager,
            &ExportOptions {
                tools: vec![ai::ToolDefinition {
                    name: "read".to_string(),
                    description: "读取文件".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "文件路径"
                            }
                        },
                        "required": ["path"]
                    }),
                }],
                ..ExportOptions::default()
            },
            &ExportTheme::resolve(None),
        )
        .expect("session export should build");

        assert_eq!(export.tools.len(), 1);
        assert_eq!(export.tools[0].name, "read");
        assert_eq!(
            export.tools[0].parameters["properties"]["path"]["type"],
            "string"
        );
    }

    #[test]
    fn session_export_includes_rendered_tools_like_pi() {
        let manager = SessionManager::in_memory("/tmp/project");
        let mut rendered_tools = std::collections::BTreeMap::new();
        rendered_tools.insert(
            "call-1".to_string(),
            RenderedToolHtml {
                call_html: Some("<span>custom call</span>".to_string()),
                result_html_collapsed: Some("<span>short</span>".to_string()),
                result_html_expanded: Some("<div>full</div>".to_string()),
            },
        );

        let export = session_export(
            &manager,
            &ExportOptions {
                rendered_tools,
                ..ExportOptions::default()
            },
            &ExportTheme::resolve(None),
        )
        .expect("session export should build");

        let rendered = export
            .rendered_tools
            .get("call-1")
            .expect("rendered tool should be exported");
        assert_eq!(
            rendered.call_html.as_deref(),
            Some("<span>custom call</span>")
        );
        assert_eq!(
            rendered.result_html_collapsed.as_deref(),
            Some("<span>short</span>")
        );
        assert_eq!(
            rendered.result_html_expanded.as_deref(),
            Some("<div>full</div>")
        );
    }

    fn temp_dir() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-export-html-test-{id}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
