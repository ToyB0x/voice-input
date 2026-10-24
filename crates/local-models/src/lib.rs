//! Explicit, pinned model preparation. This crate never receives user content.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

type Result<T> = std::result::Result<T, &'static str>;
const CANCELLED: &str = "モデルの準備を取り消しました。";
const INVALID: &str = "モデルの容量またはSHA-256が一致しません。再ダウンロードしてください。";

#[derive(Clone, Deserialize)]
struct Identity {
    file: String,
    bytes: u64,
    sha256: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    revision: String,
}
#[derive(Deserialize)]
struct Group {
    #[serde(default)]
    repo: String,
    #[serde(default)]
    revision: String,
    models: BTreeMap<String, Identity>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Label {
    name: String,
    memory_hint: String,
}
#[derive(Deserialize)]
struct Catalog {
    transcription: Group,
    correction: Group,
    labels: BTreeMap<String, Label>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub bytes: u64,
    pub license: String,
    pub memory_hint: String,
    pub cached: bool,
    #[serde(skip)]
    identity: Identity,
}
impl Model {
    fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.identity.repo, self.identity.revision, self.identity.file
        )
    }
    fn path(&self, root: &Path) -> PathBuf {
        root.join(format!(
            "{}-{}-{}",
            self.id,
            &self.identity.sha256[..12],
            self.identity.file
        ))
    }
    fn exists(&self, root: &Path) -> bool {
        fs::symlink_metadata(self.path(root)).is_ok_and(|m| m.is_file() && m.len() == self.bytes)
    }
}
pub fn catalog() -> Vec<Model> {
    let source: Catalog =
        serde_json::from_str(include_str!("../catalog.json")).expect("bundled model catalog");
    let mut result = Vec::new();
    for (kind, group, license) in [
        ("transcription", source.transcription, "MIT"),
        ("correction", source.correction, "Apache-2.0"),
    ] {
        for (id, mut identity) in group.models {
            if identity.repo.is_empty() {
                identity.repo = group.repo.clone();
                identity.revision = group.revision.clone();
            }
            let label = &source.labels[&id];
            result.push(Model {
                id,
                name: label.name.clone(),
                kind: kind.into(),
                bytes: identity.bytes,
                license: license.into(),
                memory_hint: label.memory_hint.clone(),
                cached: false,
                identity,
            });
        }
    }
    result
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub id: String,
    pub path: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub generation: u64,
    pub phase: String,
    pub model_id: String,
    pub received: u64,
    pub total: u64,
    pub index: usize,
    pub count: usize,
    pub message: String,
    pub prepared: Vec<Prepared>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: "idle".into(),
            model_id: String::new(),
            received: 0,
            total: 0,
            index: 0,
            count: 0,
            message: "モデルの取得は明示操作時だけ行います。".into(),
            prepared: Vec::new(),
        }
    }
}
impl Status {
    fn busy(&self) -> bool {
        ["starting", "verifying", "downloading"].contains(&self.phase.as_str())
    }
}
struct Inner {
    status: Status,
    worker: Option<JoinHandle<()>>,
}
#[derive(Clone)]
pub struct Manager {
    root: PathBuf,
    inner: Arc<Mutex<Inner>>,
    cancelled: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
}
impl Manager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            inner: Arc::new(Mutex::new(Inner {
                status: Status::default(),
                worker: None,
            })),
            cancelled: Arc::new(AtomicBool::new(false)),
            closing: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn status(&self) -> Status {
        self.inner
            .lock()
            .expect("model manager mutex")
            .status
            .clone()
    }
    pub fn models(&self) -> Vec<Model> {
        catalog()
            .into_iter()
            .map(|mut m| {
                m.cached = m.exists(&self.root);
                m
            })
            .collect()
    }
    pub fn start(&self, ids: Vec<String>, download: bool) -> Result<Status> {
        let all = catalog();
        if ids.is_empty() || ids.len() > 2 {
            return Err("音声モデルと必要な補正モデルを選んでください。");
        }
        let models = ids
            .iter()
            .map(|id| {
                all.iter()
                    .find(|m| &m.id == id)
                    .cloned()
                    .ok_or("未対応のモデルです。")
            })
            .collect::<Result<Vec<_>>>()?;
        if models[0].kind != "transcription"
            || (models.len() == 2 && models[1].kind != "correction")
        {
            return Err("モデルの種類が不正です。");
        }
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "モデルの状態を取得できません。")?;
        if self.closing.load(Ordering::Acquire) || inner.status.busy() {
            return Err("モデルの準備が終わるまでお待ちください。");
        }
        self.cancelled.store(false, Ordering::Release);
        inner.status = Status {
            generation: inner.status.generation + 1,
            phase: "starting".into(),
            count: models.len(),
            ..Status::default()
        };
        let worker = self.clone();
        inner.worker = Some(thread::spawn(move || {
            let result = worker.prepare(&models, download);
            let mut state = worker.inner.lock().expect("model manager mutex");
            match result {
                Ok(prepared) if worker.check_cancel().is_ok() => {
                    state.status.phase = "ready".into();
                    state.status.prepared = prepared;
                    state.status.message = "検証完了。設定欄へ反映できます。".into();
                }
                Ok(_) | Err(CANCELLED) => {
                    state.status.phase = "cancelled".into();
                    state.status.prepared.clear();
                    state.status.message = CANCELLED.into();
                }
                Err(error) => {
                    state.status.phase = "error".into();
                    state.status.prepared.clear();
                    state.status.message = error.into();
                }
            }
        }));
        Ok(inner.status.clone())
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn shutdown(&self) {
        self.closing.store(true, Ordering::Release);
        self.cancel();
        let handle = self
            .inner
            .lock()
            .ok()
            .and_then(|mut inner| inner.worker.take());
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
    fn check_cancel(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Acquire) || self.closing.load(Ordering::Acquire) {
            Err(CANCELLED)
        } else {
            Ok(())
        }
    }
    fn progress(&self, phase: &str, received: u64) {
        let mut state = self.inner.lock().expect("model manager mutex");
        state.status.phase = phase.into();
        state.status.received = received;
        state.status.message = if phase == "downloading" {
            "公式配布元からモデルを取得しています。"
        } else {
            "モデルのSHA-256を検証しています。"
        }
        .into();
    }
    fn verify(&self, path: &Path, model: &Model) -> Result<()> {
        self.progress("verifying", 0);
        if !fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == model.bytes) {
            return Err(INVALID);
        }
        let mut input = File::open(path).map_err(|_| "保存したモデルを読み取れません。")?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 262144];
        let mut bytes = 0u64;
        loop {
            self.check_cancel()?;
            let n = input
                .read(&mut buffer)
                .map_err(|_| "モデルの検証中に読み取りが失敗しました。")?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > model.bytes {
                return Err(INVALID);
            }
            digest.update(&buffer[..n]);
            self.progress("verifying", bytes);
        }
        if bytes != model.bytes || format!("{:x}", digest.finalize()) != model.identity.sha256 {
            return Err(INVALID);
        }
        Ok(())
    }
    fn prepare(&self, models: &[Model], download: bool) -> Result<Vec<Prepared>> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&self.root)
            .map_err(|_| "モデルの保存先を作成できません。")?;
        let mut prepared = Vec::new();
        for (index, model) in models.iter().enumerate() {
            self.check_cancel()?;
            {
                let mut state = self.inner.lock().expect("model manager mutex");
                state.status.model_id = model.id.clone();
                state.status.total = model.bytes;
                state.status.index = index + 1;
            }
            let target = model.path(&self.root);
            if self.verify(&target, model).is_err() {
                self.check_cancel()?;
                if !download {
                    return Err(INVALID);
                }
                self.download(model, &target)?;
            }
            self.check_cancel()?;
            prepared.push(Prepared {
                id: model.id.clone(),
                path: target.to_string_lossy().into_owned(),
            });
        }
        Ok(prepared)
    }
    fn download(&self, model: &Model, target: &Path) -> Result<()> {
        // macOS ships curl. Disable user configuration; only bundled public
        // URLs are accepted. No user content, cookies, tokens or paths are sent.
        let mut command = Command::new("/usr/bin/curl");
        command.args([
            "--disable",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--globoff",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "15",
            "--speed-time",
            "30",
            "--speed-limit",
            "1024",
            "--max-time",
            "7200",
            "--max-filesize",
            &model.bytes.to_string(),
            "--url",
            &model.url(),
        ]);
        self.download_with(command, model, target)
    }
    fn download_with(&self, mut command: Command, model: &Model, target: &Path) -> Result<()> {
        let generation = self.status().generation;
        let partial = self
            .root
            .join(format!("download-{}-{generation}.part", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&partial)
            .map_err(|_| "取得用ファイルを作成できません。空き容量を確認してください。")?;
        let cleanup = Partial(partial.clone());
        self.progress("downloading", 0);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::null());
        let child = command
            .spawn()
            .map_err(|_| "モデル取得を開始できません。")?;
        self.wait_download(child, &partial, model.bytes)?;
        self.verify(&partial, model)?;
        self.check_cancel()?;
        File::open(&partial)
            .and_then(|f| f.sync_all())
            .map_err(|_| "モデルを保存できません。空き容量を確認してください。")?;
        fs::rename(&partial, target).map_err(|_| "検証済みモデルを保存できません。")?;
        drop(cleanup);
        Ok(())
    }
    fn wait_download(&self, child: Child, partial: &Path, limit: u64) -> Result<()> {
        let mut child = Reap(child);
        loop {
            self.check_cancel()?;
            let received = fs::metadata(partial)
                .map_err(|_| "取得用ファイルを確認できません。")?
                .len();
            if received > limit {
                return Err(INVALID);
            }
            self.progress("downloading", received);
            if let Some(status) = child
                .0
                .try_wait()
                .map_err(|_| "取得処理の状態を確認できません。")?
            {
                return if status.success() {
                    Ok(())
                } else {
                    Err("取得に失敗しました。ネット接続と空き容量を確認し、再試行してください。")
                };
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
struct Partial(PathBuf);
impl Drop for Partial {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    fn fixture(label: &str) -> (Manager, Model) {
        let root =
            std::env::temp_dir().join(format!("voice-models-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let manager = Manager::new(root);
        let mut model = catalog().remove(0);
        model.bytes = 3;
        model.identity.bytes = 3;
        model.identity.sha256 = format!("{:x}", Sha256::digest(b"abc"));
        (manager, model)
    }
    #[test]
    fn pinned_catalog_has_safe_unique_identities() {
        let models = catalog();
        assert_eq!(models.len(), 8);
        for m in &models {
            assert!(
                m.id.chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
            );
            assert_eq!(m.identity.sha256.len(), 64);
            assert_eq!(m.identity.revision.len(), 40);
            assert!(!m.identity.file.contains('/'));
            assert!(m.url().starts_with("https://huggingface.co/"));
            assert!(m.bytes > 100_000_000 && m.bytes < 6_000_000_000);
        }
        assert_eq!(
            models
                .iter()
                .map(|m| &m.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            8
        );
    }
    #[test]
    fn cached_files_are_verified_and_corruption_is_not_applied() {
        let (manager, model) = fixture("integrity");
        let file = model.path(&manager.root);
        fs::write(&file, b"abc").unwrap();
        assert!(manager.verify(&file, &model).is_ok());
        fs::write(&file, b"bad").unwrap();
        assert_eq!(manager.verify(&file, &model), Err(INVALID));
        assert!(manager.prepare(&[model], false).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"bad");
        manager.shutdown();
        fs::remove_dir_all(manager.root).unwrap();
    }
    #[test]
    fn publish_only_verified_transfers_and_preserve_prior_model_on_failure() {
        let (manager, model) = fixture("publish");
        let target = model.path(&manager.root);
        let mut valid = Command::new("/usr/bin/printf");
        valid.arg("abc");
        manager.download_with(valid, &model, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"abc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut corrupt = Command::new("/usr/bin/printf");
        corrupt.arg("bad");
        assert_eq!(
            manager.download_with(corrupt, &model, &target),
            Err(INVALID)
        );
        assert_eq!(fs::read(&target).unwrap(), b"abc");
        assert!(
            fs::read_dir(&manager.root).unwrap().all(|e| !e
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "part"))
        );
        manager.shutdown();
        fs::remove_dir_all(manager.root).unwrap();
    }
    #[test]
    fn cancellation_reaps_transport_and_keeps_existing_model() {
        let (manager, model) = fixture("cancel");
        let target = model.path(&manager.root);
        fs::write(&target, b"abc").unwrap();
        let partial = manager.root.join("test.part");
        fs::write(&partial, b"").unwrap();
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        manager.cancel();
        let started = Instant::now();
        assert_eq!(manager.wait_download(child, &partial, 3), Err(CANCELLED));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        {
            let _cleanup = Partial(partial.clone());
        }
        assert!(!partial.exists());
        assert_eq!(fs::read(target).unwrap(), b"abc");
        manager.shutdown();
        assert!(manager.start(vec!["base".into()], true).is_err());
        fs::remove_dir_all(manager.root).unwrap();
    }
    #[test]
    fn oversized_failed_transfer_is_never_published() {
        let (manager, model) = fixture("oversize");
        let partial = manager.root.join("test.part");
        fs::write(&partial, b"toolong").unwrap();
        let child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        assert_eq!(manager.wait_download(child, &partial, 3), Err(INVALID));
        assert!(!model.path(&manager.root).exists());
        assert!(manager.start(vec!["../private".into()], true).is_err());
        assert!(manager.start(vec!["qwen3-4b".into()], true).is_err());
        manager.shutdown();
        fs::remove_dir_all(manager.root).unwrap();
    }
}
