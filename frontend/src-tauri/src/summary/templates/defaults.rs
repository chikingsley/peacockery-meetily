//! Every shipped template, compiled into the binary.

/// Template used when a summary request names none.
pub const DEFAULT_TEMPLATE_ID: &str = "standard_meeting";

/// `(id, json)` for every shipped template.
pub const BUILTIN_TEMPLATES: &[(&str, &str)] = &[
    ("daily_standup", include_str!("../../../templates/daily_standup.json")),
    ("project_sync", include_str!("../../../templates/project_sync.json")),
    ("retrospective", include_str!("../../../templates/retrospective.json")),
    (
        "sales_marketing_client_call",
        include_str!("../../../templates/sales_marketing_client_call.json"),
    ),
    ("standard_meeting", include_str!("../../../templates/standard_meeting.json")),
];

pub fn get_builtin_template(id: &str) -> Option<&'static str> {
    BUILTIN_TEMPLATES
        .iter()
        .find(|(template_id, _)| *template_id == id)
        .map(|(_, json)| *json)
}
