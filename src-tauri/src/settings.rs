//! Only preferences are serializable; audio and transcripts never enter this file.
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub version: u8,
    #[serde(default)]
    pub usage_preset: Option<String>,
    pub model_path: String,
    pub backend: String,
    pub shortcut: String,
    #[serde(default)]
    pub auto_insert: bool,
    #[serde(default)]
    pub auto_correction: bool,
    #[serde(default)]
    pub reuse_correction_model: bool,
    #[serde(default)]
    pub show_overlay: bool,
    #[serde(default)]
    pub recognition_context: bool,
    #[serde(default)]
    pub ocr_context: bool,
    #[serde(default = "default_whisper_context")]
    pub whisper_context: bool,
    #[serde(default)]
    pub correction_context: bool,
    #[serde(default)]
    pub correction_model_path: String,
    #[serde(default)]
    pub correction_instruction: String,
    #[serde(default = "default_correction_engine")]
    pub correction_engine: String,
    #[serde(default = "default_correction_backend")]
    pub correction_backend: String,
}
fn default_whisper_context() -> bool {
    true
}
fn default_correction_engine() -> String {
    "qwen3".into()
}
fn default_correction_backend() -> String {
    "metal".into()
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            usage_preset: None,
            model_path: String::new(),
            backend: "metal".into(),
            shortcut: "Command+Shift+Space".into(),
            auto_insert: false,
            auto_correction: false,
            reuse_correction_model: false,
            show_overlay: false,
            recognition_context: false,
            ocr_context: false,
            whisper_context: true,
            correction_context: false,
            correction_model_path: String::new(),
            correction_instruction: String::new(),
            correction_engine: default_correction_engine(),
            correction_backend: "metal".into(),
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .usage_preset
            .as_deref()
            .is_some_and(|id| !["asr-first", "text-hints", "qwen-preview"].contains(&id))
        {
            return Err("保存されたプリセット名が不正です。".into());
        }
        if self.version != 1
            || !["cpu", "metal"].contains(&self.backend.as_str())
            || !["cpu", "metal"].contains(&self.correction_backend.as_str())
            || !["qwen3", "apple-foundation"].contains(&self.correction_engine.as_str())
        {
            return Err("設定のバージョンまたは実行方式が不正です。".into());
        }
        if self.correction_instruction.len() > 1536 || self.correction_instruction.contains('\0') {
            return Err("補正の追加指示はUTF-8で1536バイト以内・NULなしにしてください。".into());
        }
        if self.correction_model_path.len() > 4096
            || (!self.correction_model_path.is_empty()
                && !Path::new(&self.correction_model_path).is_absolute())
        {
            return Err("補正モデルには絶対パスを指定してください。".into());
        }
        if self.model_path.len() > 4096
            || (!self.model_path.is_empty() && !Path::new(&self.model_path).is_absolute())
            || self.shortcut.len() > 128
        {
            return Err(
                "モデルには絶対パスを指定してください。設定が長すぎないかも確認してください。"
                    .into(),
            );
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<Preferences, String> {
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::default()),
        Err(e) => return Err(format!("設定を確認できません: {e}")),
        Ok(meta) if meta.len() > 65_536 => return Err("設定ファイルが大きすぎます。".into()),
        Ok(_) => {}
    }
    let settings: Preferences = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("設定を読み込めません。初期値で起動しました: {e}"))?;
    settings.validate()?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &Preferences) -> Result<(), String> {
    settings.validate()?;
    let parent = path.parent().ok_or("設定の保存先が不正です。")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!("settings-{}.tmp", std::process::id()));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(settings)?)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("設定を保存できません: {e}"))
}

#[cfg(target_os = "macos")]
impl Preferences {
    pub fn context_config(&self) -> crate::context::Config {
        crate::context::Config {
            ax: self.recognition_context,
            ocr: self.ocr_context,
            whisper: self.whisper_context,
            correction: self.correction_context,
            comparison: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_round_trip_and_invalid_file_is_preserved() {
        let root =
            std::env::temp_dir().join(format!("voice-input-settings-{}", std::process::id()));
        let path = root.join("settings.json");
        let _ = fs::remove_dir_all(&root);
        assert_eq!(load(&path).unwrap(), Preferences::default());
        let legacy: Preferences =
            serde_json::from_str(r#"{"version":1,"modelPath":"","backend":"cpu","shortcut":""}"#)
                .unwrap();
        assert!(!legacy.auto_insert);
        assert!(!legacy.auto_correction);
        assert!(!legacy.reuse_correction_model);
        assert!(!legacy.show_overlay);
        assert!(!legacy.recognition_context);
        assert!(!legacy.ocr_context && !legacy.correction_context);
        assert!(legacy.whisper_context);
        assert_eq!(legacy.correction_engine, "qwen3");
        let settings = Preferences {
            correction_engine: "apple-foundation".into(),
            correction_instruction: "自然な漢字表記にしてください。".into(),
            auto_correction: true,
            reuse_correction_model: true,
            show_overlay: true,
            model_path: "/tmp/日本語モデル.bin".into(),
            shortcut: String::new(),
            ..Preferences::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, b"invalid").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reject_unknown_fields_and_unsupported_settings() {
        assert!(serde_json::from_str::<Preferences>(r#"{"version":1,"modelPath":"","backend":"cpu","shortcut":"","transcript":"private"}"#).is_err());
        let mut settings = Preferences {
            model_path: "relative.bin".into(),
            ..Preferences::default()
        };
        assert!(settings.validate().is_err());
        settings.model_path.clear();
        settings.correction_engine = "cloud".into();
        assert!(settings.validate().is_err());
        settings.correction_engine = "apple-foundation".into();
        assert!(settings.validate().is_ok());
        settings.version = 2;
        assert!(settings.validate().is_err());
    }
}
