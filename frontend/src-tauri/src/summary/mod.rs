//! Meeting summaries: built-in templates, one LLM pass (`llm::generate`) over the
//! meeting's best transcript, Markdown cleaning and auto-titling.

pub mod commands;
pub mod llm;
pub mod processor;
pub mod service;
pub mod template_commands;
pub mod templates;
