//! Peacockery Voice integration: settings, the API key, the managed client and their commands.

pub mod commands;
pub mod credential;
pub mod service;
pub mod settings;

pub use service::{describe_error, VoiceService};
pub use settings::VoiceSettings;
