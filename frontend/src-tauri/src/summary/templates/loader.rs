use super::defaults::{get_builtin_template, BUILTIN_TEMPLATES};
use super::types::Template;

/// Load and validate a built-in template.
pub fn get_template(template_id: &str) -> Result<Template, String> {
    let json = get_builtin_template(template_id).ok_or_else(|| {
        format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        )
    })?;
    validate_and_parse_template(json)
}

/// Parse template JSON and validate its structure.
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;
    template.validate()?;
    Ok(template)
}

/// Ids of every built-in template, sorted.
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = BUILTIN_TEMPLATES.iter().map(|(id, _)| id.to_string()).collect();
    ids.sort();
    ids
}

/// `(id, name, description)` of every built-in template.
pub fn list_templates() -> Vec<(String, String, String)> {
    list_template_ids()
        .into_iter()
        .filter_map(|id| match get_template(&id) {
            Ok(template) => Some((id, template.name, template.description)),
            Err(e) => {
                log::warn!("Skipping invalid template '{}': {}", id, e);
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::defaults::DEFAULT_TEMPLATE_ID;
    use super::*;

    #[test]
    fn every_builtin_template_parses() {
        let ids = list_template_ids();
        assert_eq!(ids.len(), BUILTIN_TEMPLATES.len());
        for id in ids {
            let template = get_template(&id).unwrap_or_else(|e| panic!("{}: {}", id, e));
            assert!(!template.name.is_empty());
            assert!(!template.description.is_empty());
            assert!(!template.sections.is_empty());
        }
        assert_eq!(list_templates().len(), BUILTIN_TEMPLATES.len());
    }

    #[test]
    fn default_template_exists() {
        assert!(get_template(DEFAULT_TEMPLATE_ID).is_ok());
    }

    #[test]
    fn unknown_and_invalid_templates_fail() {
        assert!(get_template("psychatric_session").is_err());
        assert!(validate_and_parse_template("invalid json").is_err());
    }
}
