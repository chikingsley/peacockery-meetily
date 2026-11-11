//! Meeting summary templates. Every template ships inside the binary
//! (`frontend/src-tauri/templates/*.json`); there are no user or resource folders.

mod defaults;
mod loader;
mod types;

pub use defaults::DEFAULT_TEMPLATE_ID;
pub use loader::{get_template, list_template_ids, list_templates, validate_and_parse_template};
pub use types::{Template, TemplateSection};
