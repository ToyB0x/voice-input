//! Deterministic comparison plans and reference-based metrics. No I/O or models.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use unicode_categories::UnicodeCategories;
use unicode_normalization::UnicodeNormalization;

pub const MAX_CASES: usize = 2048;
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub speech: Vec<String>,
    pub speech_backends: Vec<String>,
    pub corrections: Vec<String>,
    pub correction_backends: Vec<String>,
    pub sources: Vec<String>,
    pub routes: Vec<String>,
    pub reference: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Case {
    pub speech: String,
    pub speech_backend: String,
    pub correction: String,
    pub correction_backend: String,
    pub source: String,
    pub route: String,
}
impl Case {
    pub fn whisper_context(&self) -> bool {
        matches!(self.route.as_str(), "asr-only" | "both")
    }
    pub fn correction_context(&self) -> bool {
        matches!(self.route.as_str(), "correction-only" | "both")
    }
}
fn allowed(values: &[String], choices: &[&str], maximum: usize) -> bool {
    !values.is_empty()
        && values.len() <= maximum
        && values.iter().all(|s| choices.contains(&s.as_str()))
}
pub fn plan(selection: &Selection) -> Result<Vec<Case>, &'static str> {
    if !allowed(
        &selection.speech,
        &[
            "base",
            "small",
            "medium",
            "large-v3-turbo",
            "large-v3",
            "current-speech",
        ],
        6,
    ) || !allowed(&selection.speech_backends, &["cpu", "metal"], 2)
        || !allowed(
            &selection.corrections,
            &[
                "none",
                "apple-foundation",
                "qwen3-1.7b",
                "qwen3-4b",
                "qwen3-8b",
                "current-correction",
            ],
            6,
        )
        || selection.correction_backends.len() > 2
        || selection
            .correction_backends
            .iter()
            .any(|b| !["cpu", "metal"].contains(&b.as_str()))
        || (selection
            .corrections
            .iter()
            .any(|c| !["none", "apple-foundation"].contains(&c.as_str()))
            && selection.correction_backends.is_empty())
        || !allowed(&selection.sources, &["none", "ax", "ocr", "both"], 4)
        || !allowed(
            &selection.routes,
            &["none", "asr-only", "correction-only", "both"],
            4,
        )
        || selection
            .reference
            .as_ref()
            .is_some_and(|s| s.chars().count() > 2048 || s.contains('\0'))
    {
        return Err("比較条件または基準文が不正です。モデル・方式・文脈条件を選択してください。");
    }
    let mut cases = BTreeSet::new();
    for speech in &selection.speech {
        for speech_backend in &selection.speech_backends {
            for correction in &selection.corrections {
                let backends = match correction.as_str() {
                    "none" => vec!["none".to_owned()],
                    "apple-foundation" => vec!["system".to_owned()],
                    _ => selection.correction_backends.clone(),
                };
                for correction_backend in backends {
                    for source in &selection.sources {
                        for route in &selection.routes {
                            // Collapse conditions with identical actual inputs.
                            let route = if correction == "none" {
                                match route.as_str() {
                                    "correction-only" => "none",
                                    "both" => "asr-only",
                                    value => value,
                                }
                            } else {
                                route.as_str()
                            };
                            let (source, route) = if source == "none" || route == "none" {
                                ("none", "none")
                            } else {
                                (source.as_str(), route)
                            };
                            cases.insert(Case {
                                speech: speech.clone(),
                                speech_backend: speech_backend.clone(),
                                correction: correction.clone(),
                                correction_backend: correction_backend.clone(),
                                source: source.into(),
                                route: route.into(),
                            });
                            if cases.len() > MAX_CASES {
                                return Err(
                                    "比較件数が上限を超えます。選択条件を減らしてください。",
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(cases.into_iter().collect())
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub edits: usize,
    pub reference_chars: usize,
    pub cer: Option<f64>,
    pub silence_false_positive: Option<bool>,
}
fn normalized(text: &str) -> Vec<char> {
    text.nfkc()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace() && !c.is_punctuation())
        .collect()
}
pub fn score(reference: Option<&str>, candidate: &str) -> Option<Score> {
    let reference = normalized(reference?);
    let candidate = normalized(candidate);
    let mut previous: Vec<usize> = (0..=candidate.len()).collect();
    let mut current = vec![0; candidate.len() + 1];
    for (i, left) in reference.iter().enumerate() {
        current[0] = i + 1;
        for (j, right) in candidate.iter().enumerate() {
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + usize::from(left != right));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let edits = previous[candidate.len()];
    Some(Score {
        edits,
        reference_chars: reference.len(),
        cer: (!reference.is_empty()).then(|| edits as f64 / reference.len() as f64),
        silence_false_positive: reference.is_empty().then_some(!candidate.is_empty()),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> Selection {
        Selection {
            speech: vec!["base".into()],
            speech_backends: vec!["cpu".into()],
            corrections: vec!["none".into(), "apple-foundation".into()],
            correction_backends: vec!["cpu".into(), "metal".into()],
            sources: vec!["none".into(), "ax".into(), "ocr".into(), "both".into()],
            routes: vec![
                "none".into(),
                "asr-only".into(),
                "correction-only".into(),
                "both".into(),
            ],
            reference: None,
        }
    }
    #[test]
    fn distinct_actual_inputs_and_all_context_routes() {
        let cases = plan(&selection()).unwrap();
        assert_eq!(cases.len(), 14); // 4 uncorrected + 10 corrected conditions
        assert!(
            cases
                .iter()
                .any(|c| c.source == "ocr" && c.whisper_context() && c.correction_context())
        );
        assert!(
            cases
                .iter()
                .filter(|c| c.correction == "none")
                .all(|c| !c.correction_context())
        );
        assert_eq!(cases, plan(&selection()).unwrap());
    }
    #[test]
    fn maximum_catalog_matrix_and_reject_untrusted_axes() {
        let mut s = selection();
        s.speech = vec![
            "base",
            "small",
            "medium",
            "large-v3-turbo",
            "large-v3",
            "current-speech",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        s.speech_backends.push("metal".into());
        s.corrections = vec![
            "none",
            "apple-foundation",
            "qwen3-1.7b",
            "qwen3-4b",
            "qwen3-8b",
            "current-correction",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(plan(&s).unwrap().len(), 1128);
        s.speech.push("remote-api".into());
        assert!(plan(&s).is_err());
    }
    #[test]
    fn cer_requires_reference_and_does_not_invent_accuracy() {
        assert!(score(None, "推測").is_none());
        assert_eq!(
            score(Some("ＡＢＣ、１２３。"), "abc 123").unwrap().cer,
            Some(0.0)
        );
        assert_eq!(score(Some("東京"), "京都").unwrap().cer, Some(1.0));
        assert!(score(Some("あ"), "あいうえお").unwrap().cer.unwrap() > 1.0);
        let silence = score(Some(""), "誤認識").unwrap();
        assert!(silence.cer.is_none());
        assert_eq!(silence.silence_false_positive, Some(true));
    }
}
