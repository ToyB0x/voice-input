//! Explicit setup recipes: verified bundled model IDs, then saved preferences.
use crate::settings::Preferences;
use local_models::Status as Preparation;
use serde::Deserialize;
use std::path::Path;
use tauri::Manager;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preset {
    id: String,
    name: String,
    description: String,
    evidence: String,
    speech: String,
    correction: Option<String>,
    text_hints: bool,
}
impl Preset {
    fn ids(&self) -> Vec<&str> {
        std::iter::once(self.speech.as_str())
            .chain(self.correction.as_deref())
            .collect()
    }
}
fn recipe(id: &str) -> Result<Preset, String> {
    let presets: Vec<Preset> = serde_json::from_str(include_str!("../../src/usage-presets.json"))
        .map_err(|_| "プリセットの定義を読み込めません。")?;
    presets
        .into_iter()
        .find(|p| {
            p.id == id && !p.name.is_empty() && !p.description.is_empty() && !p.evidence.is_empty()
        })
        .ok_or("プリセットを選んでください。".into())
}
fn configured(
    mut prefs: Preferences,
    id: &str,
    generation: u64,
    prepared: &Preparation,
) -> Result<Preferences, String> {
    let preset = recipe(id)?;
    if prepared.phase != "ready"
        || prepared.generation != generation
        || prepared
            .prepared
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>()
            != preset.ids()
    {
        return Err("選択したプリセットのモデルを準備・検証してください。選択変更・中止後は再検証が必要です。".into());
    }
    for model in &prepared.prepared {
        if !Path::new(&model.path).is_absolute()
            || !std::fs::symlink_metadata(&model.path).is_ok_and(|metadata| metadata.is_file())
        {
            return Err("検証済みモデルが見つかりません。もう一度準備してください。".into());
        }
    }
    prefs.model_path = prepared.prepared[0].path.clone();
    prefs.backend = "metal".into();
    prefs.auto_correction = false;
    prefs.recognition_context = preset.text_hints;
    prefs.ocr_context = preset.text_hints;
    prefs.whisper_context = true;
    prefs.correction_context = false;
    prefs.correction_instruction.clear();
    prefs.reuse_correction_model = false;
    if preset.correction.is_some() {
        prefs.correction_model_path = prepared.prepared[1].path.clone();
        prefs.correction_engine = "qwen3".into();
        prefs.correction_backend = "metal".into();
    }
    prefs.usage_preset = Some(preset.id);
    prefs.validate()?;
    Ok(prefs)
}
#[tauri::command]
pub fn apply_usage_preset(
    app: tauri::AppHandle,
    id: String,
    generation: u64,
    preferences: Preferences,
) -> Result<crate::desktop::Status, String> {
    let activity = app.state::<crate::activity::Activity>();
    let _gate = activity.lock()?;
    let recording = app.state::<crate::recorder::Recorder>().status()?;
    if matches!(recording.phase, "recording" | "preparing")
        || recording.recognition.working
        || app.state::<crate::comparison::Comparisons>().working()
        || app.state::<crate::correction::Corrector>().status().working
        || app.state::<crate::updater::Updates>().installing()
    {
        return Err("録音・比較・補正・更新の終了をお待ちください。".into());
    }
    if recording.sample_count > 0
        || recording.recognition.transcript.is_some()
        || app.state::<crate::comparison::Comparisons>().has_results()
        || app
            .state::<crate::correction::Corrector>()
            .status()
            .text
            .is_some()
    {
        return Err("モデル切替の前に、録音・比較・補正結果をコピーして破棄してください。自動では破棄しません。".into());
    }
    let state = app
        .state::<local_models::Manager>()
        .ready_status(generation)
        .map_err(str::to_owned)?;
    let prefs = configured(preferences, &id, generation, &state)?;
    crate::desktop::apply_preferences(&app, &app.state::<crate::desktop::Desktop>(), prefs)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipes_require_the_current_verified_generation_and_preserve_unrelated_preferences() {
        let root = std::env::temp_dir().join(format!("preset-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let speech = root.join("speech.bin");
        let correction = root.join("correction.gguf");
        std::fs::write(&speech, b"fixture").unwrap();
        std::fs::write(&correction, b"fixture").unwrap();
        for id in ["asr-first", "text-hints", "qwen-preview"] {
            let recipe = recipe(id).unwrap();
            assert!(
                recipe
                    .ids()
                    .iter()
                    .all(|id| local_models::catalog().iter().any(|model| model.id == *id))
            );
            assert!(
                !recipe.name.is_empty()
                    && !recipe.description.is_empty()
                    && !recipe.evidence.is_empty()
            );
            let prepared = Preparation {
                generation: 7,
                phase: "ready".into(),
                prepared: recipe
                    .ids()
                    .iter()
                    .enumerate()
                    .map(|(i, id)| local_models::Prepared {
                        id: id.to_string(),
                        path: if i == 0 { &speech } else { &correction }
                            .to_str()
                            .unwrap()
                            .into(),
                    })
                    .collect(),
                ..Preparation::default()
            };
            let before = Preferences {
                shortcut: "Control+Alt+KeyV".into(),
                show_overlay: true,
                auto_insert: true,
                auto_correction: true,
                ..Preferences::default()
            };
            let after = configured(before.clone(), id, 7, &prepared).unwrap();
            assert_eq!(after.shortcut, before.shortcut);
            assert_eq!(after.auto_insert, before.auto_insert);
            assert!(after.show_overlay);
            assert!(!after.auto_correction);
            assert_eq!(after.model_path, speech.to_str().unwrap());
            assert_eq!(after.usage_preset.as_deref(), Some(id));
            assert_eq!(after.ocr_context, id == "text-hints");
            assert!(!after.correction_context);
            assert!(configured(before.clone(), id, 6, &prepared).is_err());
            let mut cancelled = prepared.clone();
            cancelled.phase = "cancelled".into();
            assert!(configured(before.clone(), id, 7, &cancelled).is_err());
            let mut wrong = prepared.clone();
            wrong.prepared[0].id = "base".into();
            assert!(configured(before, id, 7, &wrong).is_err());
            if id == "qwen-preview" {
                assert_eq!(after.correction_model_path, correction.to_str().unwrap());
                assert_eq!(after.correction_engine, "qwen3");
            }
        }
        std::fs::remove_file(&speech).unwrap();
        let missing = Preparation {
            generation: 7,
            phase: "ready".into(),
            prepared: vec![local_models::Prepared {
                id: "large-v3-turbo".into(),
                path: speech.to_str().unwrap().into(),
            }],
            ..Preparation::default()
        };
        assert!(configured(Preferences::default(), "asr-first", 7, &missing).is_err());
        assert!(recipe("unknown").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
