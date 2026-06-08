use agent::harness::Skill;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSkillBlock {
    pub name: String,
    pub location: String,
    pub content: String,
    pub user_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillCommandExpansion {
    Expanded(String),
    NotSkillCommand(String),
    UnknownSkill(String),
}

pub fn expand_skill_command(text: &str, skills: &[Skill]) -> Result<SkillCommandExpansion, String> {
    let Some(invocation) = parse_skill_invocation(text) else {
        return Ok(SkillCommandExpansion::NotSkillCommand(text.to_string()));
    };

    let Some(skill) = skills.iter().find(|skill| skill.name == invocation.name) else {
        return Ok(SkillCommandExpansion::UnknownSkill(text.to_string()));
    };

    let content = fs::read_to_string(&skill.file_path)
        .map_err(|error| format!("读取 skill 文件失败：{}: {error}", skill.file_path))?;
    let body = strip_frontmatter(&content).trim().to_string();
    let base_dir = Path::new(&skill.file_path)
        .parent()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let skill_block = format!(
        "<skill name=\"{}\" location=\"{}\">\nReferences are relative to {}.\n\n{}\n</skill>",
        skill.name, skill.file_path, base_dir, body
    );

    if invocation.args.is_empty() {
        Ok(SkillCommandExpansion::Expanded(skill_block))
    } else {
        Ok(SkillCommandExpansion::Expanded(format!(
            "{skill_block}\n\n{}",
            invocation.args
        )))
    }
}

pub fn parse_skill_block(text: &str) -> Option<ParsedSkillBlock> {
    let close_tag = "\n</skill>";
    let text = text.strip_prefix("<skill name=\"")?;
    let name_end = text.find("\" location=\"")?;
    let name = &text[..name_end];
    let after_name = &text[name_end + "\" location=\"".len()..];
    let location_end = after_name.find("\">\n")?;
    let location = &after_name[..location_end];
    let after_header = &after_name[location_end + "\">\n".len()..];
    let close_index = after_header.find(close_tag)?;
    let content = &after_header[..close_index];
    let trailing = &after_header[close_index + close_tag.len()..];
    let user_message = trailing
        .strip_prefix("\n\n")
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(ToString::to_string);
    if trailing.is_empty() || trailing.starts_with("\n\n") {
        return Some(ParsedSkillBlock {
            name: name.to_string(),
            location: location.to_string(),
            content: content.to_string(),
            user_message,
        });
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SkillInvocation<'a> {
    name: &'a str,
    args: &'a str,
}

fn parse_skill_invocation(text: &str) -> Option<SkillInvocation<'_>> {
    let rest = text.strip_prefix("/skill:")?;
    if rest.is_empty() {
        return None;
    }
    let space_index = rest.find(' ');
    let name = space_index.map_or(rest, |index| &rest[..index]);
    if name.is_empty() {
        return None;
    }
    let args = space_index.map_or("", |index| rest[index + 1..].trim());
    Some(SkillInvocation { name, args })
}

fn strip_frontmatter(content: &str) -> &str {
    let normalized = content.strip_prefix('\u{feff}').unwrap_or(content);
    if !normalized.starts_with("---\n") && !normalized.starts_with("---\r\n") {
        return normalized;
    }

    let body_start = normalized
        .find("\n---\n")
        .map(|index| index + "\n---\n".len())
        .or_else(|| {
            normalized
                .find("\r\n---\r\n")
                .map(|index| index + "\r\n---\r\n".len())
        });
    body_start.map_or(normalized, |index| &normalized[index..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn expands_skill_command_to_skill_block() {
        let dir = temp_dir();
        let skill_path = dir.join("SKILL.md");
        fs::write(
            &skill_path,
            "---\nname: demo\ndescription: Demo\n---\nUse this skill.",
        )
        .expect("skill should be written");
        let skills = vec![skill("demo", &skill_path)];

        let expanded = expand_skill_command("/skill:demo extra context", &skills)
            .expect("skill should expand");

        let SkillCommandExpansion::Expanded(text) = expanded else {
            panic!("skill should expand");
        };
        assert!(text.contains("<skill name=\"demo\""));
        assert!(text.contains("References are relative to"));
        assert!(text.contains("Use this skill."));
        assert!(text.ends_with("extra context"));
    }

    #[test]
    fn returns_original_for_non_skill_or_unknown_skill() {
        assert_eq!(
            expand_skill_command("hello", &[]).expect("non skill should pass through"),
            SkillCommandExpansion::NotSkillCommand("hello".to_string())
        );
        assert_eq!(
            expand_skill_command("/skill:missing", &[]).expect("unknown skill should pass through"),
            SkillCommandExpansion::UnknownSkill("/skill:missing".to_string())
        );
    }

    #[test]
    fn parses_expanded_skill_block() {
        let text = concat!(
            "<skill name=\"demo\" location=\"/tmp/demo/SKILL.md\">\n",
            "References are relative to /tmp/demo.\n\n",
            "Use this skill.\n",
            "</skill>\n\n",
            "user request",
        );

        let parsed = parse_skill_block(text).expect("skill block should parse");
        assert_eq!(parsed.name, "demo");
        assert_eq!(parsed.location, "/tmp/demo/SKILL.md");
        assert!(parsed.content.contains("Use this skill."));
        assert_eq!(parsed.user_message.as_deref(), Some("user request"));
    }

    #[test]
    fn parses_skill_invocation_like_pi() {
        assert_eq!(
            parse_skill_invocation("/skill:demo one two"),
            Some(SkillInvocation {
                name: "demo",
                args: "one two",
            })
        );
        assert_eq!(
            parse_skill_invocation("/skill:demo"),
            Some(SkillInvocation {
                name: "demo",
                args: "",
            })
        );
        assert_eq!(parse_skill_invocation("/skill:"), None);
    }

    fn skill(name: &str, path: &Path) -> Skill {
        Skill {
            name: name.to_string(),
            description: "Demo".to_string(),
            content: String::new(),
            file_path: path.to_string_lossy().to_string(),
            source_info: None,
            disable_model_invocation: false,
        }
    }

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm-agent-skill-command-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }
}
