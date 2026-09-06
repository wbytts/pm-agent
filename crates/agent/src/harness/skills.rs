use crate::harness::types::Skill;

pub fn format_skills_for_system_prompt(skills: &[Skill]) -> String {
    let visible_skills = skills
        .iter()
        .filter(|skill| !skill.disable_model_invocation)
        .collect::<Vec<_>>();
    if visible_skills.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "The following skills provide specialized instructions for specific tasks.".to_string(),
        "Use the read tool to load a skill's file when the task matches its description.".to_string(),
        "When a skill file references a relative path, resolve it against the skill directory (parent of SKILL.md / dirname of the path) and use that absolute path in tool commands.".to_string(),
        String::new(),
        "<available_skills>".to_string(),
    ];

    for skill in visible_skills {
        lines.push("  <skill>".to_string());
        lines.push(format!("    <name>{}</name>", escape_xml(&skill.name)));
        lines.push(format!(
            "    <description>{}</description>",
            escape_xml(&skill.description)
        ));
        lines.push(format!(
            "    <location>{}</location>",
            escape_xml(&skill.file_path)
        ));
        lines.push("  </skill>".to_string());
    }

    lines.push("</available_skills>".to_string());
    lines.join("\n")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_visible_skills_as_xml_block() {
        let prompt = format_skills_for_system_prompt(&[
            Skill {
                name: "build".to_string(),
                description: "Use <build> & \"test\" 'ship'".to_string(),
                content: String::new(),
                file_path: "/tmp/skills/build & deploy/SKILL.md".to_string(),
                source_info: None,
                disable_model_invocation: false,
            },
            Skill {
                name: "hidden".to_string(),
                description: "hidden".to_string(),
                content: String::new(),
                file_path: "/tmp/hidden.md".to_string(),
                source_info: None,
                disable_model_invocation: true,
            },
        ]);

        assert!(prompt.contains(
            "Use the read tool to load a skill's file when the task matches its description."
        ));
        assert!(prompt.contains("<available_skills>"));
        assert!(prompt.contains("Use &lt;build&gt; &amp; &quot;test&quot; &apos;ship&apos;"));
        assert!(prompt.contains("/tmp/skills/build &amp; deploy/SKILL.md"));
        assert!(!prompt.contains("hidden"));
    }
}
