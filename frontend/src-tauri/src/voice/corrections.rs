//! User-approved literal recognition corrections. Provider artifacts stay original.
use regex::{NoExpand, RegexBuilder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptCorrection {
    pub heard: String,
    pub replacement: String,
}

pub fn correct_text(text: &str, corrections: &[TranscriptCorrection]) -> String {
    let mut result = text.to_string();
    for correction in corrections {
        if correction.heard.trim().is_empty() || correction.replacement.trim().is_empty() { continue; }
        // Unicode word boundaries prevent a short name from altering another word.
        let pattern = format!(r"\b{}\b", regex::escape(correction.heard.trim()));
        if let Ok(regex) = RegexBuilder::new(&pattern).case_insensitive(true).build() {
            result = regex.replace_all(&result, NoExpand(correction.replacement.trim())).into_owned();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrects_whole_phrases_preserving_other_words_and_literal_replacements() {
        let rules = vec![TranscriptCorrection { heard: "Sika Huntech".into(), replacement: "Cica-Huntek".into() }];
        assert_eq!(correct_text("SIKA HUNTECH's work; Sika and Sika Huntechs remain.", &rules), "Cica-Huntek's work; Sika and Sika Huntechs remain.");
        assert_eq!(correct_text("Original", &[]), "Original");
    }
}
