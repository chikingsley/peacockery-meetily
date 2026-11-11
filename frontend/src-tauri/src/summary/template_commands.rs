use crate::summary::templates;
use serde::{Deserialize, Serialize};

/// Template metadata for the template picker.
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[tauri::command]
pub async fn api_list_templates() -> Result<Vec<TemplateInfo>, String> {
    Ok(templates::list_templates()
        .into_iter()
        .map(|(id, name, description)| TemplateInfo { id, name, description })
        .collect())
}
