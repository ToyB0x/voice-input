//! A single quiet helper, owned and reaped by the app. Text travels over pipes only.
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime},
};
const LIMIT: u64 = 24_577;
const IDLE: Duration = Duration::from_secs(300);
const FAILED: &str = "補正ヘルパーを実行できません。元の認識結果は保持しています。";
#[derive(PartialEq)]
struct Key {
    runtime: std::path::PathBuf,
    model: String,
    backend: String,
    size: u64,
    modified: Option<SystemTime>,
}
struct Resident {
    key: Key,
    generation: u64,
    last_used: Instant,
    child: Child,
    stdin: Option<ChildStdin>,
    response: mpsc::Receiver<Vec<u8>>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Resident {
    fn drop(&mut self) {
        // Kill before joining: a stalled native call or full pipe cannot block exit.
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stdin.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
#[derive(Default)]
pub(crate) struct Pool {
    resident: Mutex<Option<Resident>>,
    generation: AtomicU64,
}
impl Pool {
    pub fn release(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.expire(Duration::ZERO);
    }
    pub fn tick(&self) {
        self.expire(IDLE);
    }
    fn expire(&self, timeout: Duration) {
        if let Ok(mut slot) = self.resident.try_lock()
            && slot.as_ref().is_some_and(|resident| {
                resident.last_used.elapsed() >= timeout
                    || resident.generation != self.generation.load(Ordering::Acquire)
            })
        {
            slot.take();
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &self,
        runtime: &Path,
        model: &str,
        backend: &str,
        mut input: Vec<u8>,
        epoch: &AtomicU64,
        current: u64,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        let generation = self.generation.load(Ordering::Acquire);
        let metadata = std::fs::metadata(model).map_err(|_| FAILED.to_owned())?;
        let key = Key {
            runtime: runtime.into(),
            model: model.into(),
            backend: backend.into(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
        };
        let mut slot = self.resident.lock().map_err(|_| FAILED.to_owned())?;
        if epoch.load(Ordering::Acquire) != current {
            return Err(FAILED.into());
        }
        if slot.as_ref().is_some_and(|r| {
            r.key != key || r.generation != generation || r.last_used.elapsed() >= IDLE
        }) {
            slot.take();
        }
        if slot.is_none() {
            let mut child = Command::new(runtime)
                .args([model, backend, "--serve"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| FAILED.to_owned())?;
            // Both pipes are guaranteed by Stdio::piped on a successful spawn.
            let stdin = child.stdin.take();
            let stdout = child.stdout.take().expect("piped helper stdout");
            let (tx, response) = mpsc::sync_channel(1);
            let reader = std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut frame = Vec::new();
                    let read = reader.by_ref().take(LIMIT).read_until(b'\n', &mut frame);
                    if read.is_err()
                        || frame.is_empty()
                        || frame.len() as u64 >= LIMIT
                        || frame.last() != Some(&b'\n')
                    {
                        break;
                    }
                    // Never block shutdown on an unsolicited extra response.
                    if tx.try_send(frame).is_err() {
                        break;
                    }
                }
            });
            *slot = Some(Resident {
                key,
                generation,
                last_used: Instant::now(),
                child,
                stdin,
                response,
                reader: Some(reader),
            });
        }
        let resident = slot.as_mut().expect("initialized resident");
        let mut stdin = resident.stdin.take().ok_or_else(|| FAILED.to_owned())?;
        input.push(b'\n');
        let writer = std::thread::spawn(move || {
            let written = stdin.write_all(&input).and_then(|_| stdin.flush());
            (stdin, written)
        });
        let started = Instant::now();
        let mut received = None;
        let result = loop {
            if epoch.load(Ordering::Acquire) != current
                || self.generation.load(Ordering::Acquire) != generation
                || started.elapsed() > timeout
            {
                break Err(FAILED.to_owned());
            }
            if received.is_some() && writer.is_finished() {
                break Ok(received.take().unwrap());
            }
            match resident.response.recv_timeout(Duration::from_millis(20)) {
                Ok(frame) if received.is_none() => received = Some(frame),
                Ok(_) => break Err(FAILED.to_owned()),
                Err(mpsc::RecvTimeoutError::Disconnected) => break Err(FAILED.to_owned()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if received.is_some() && writer.is_finished() {
                break Ok(received.take().unwrap());
            }
            if !matches!(resident.child.try_wait(), Ok(None)) {
                break Err(FAILED.to_owned());
            }
        };
        if result.is_err() {
            let _ = resident.child.kill();
            let _ = resident.child.wait();
        }
        let written = writer.join();
        let result = match written {
            Ok((stdin, Ok(()))) => {
                resident.stdin = Some(stdin);
                result
            }
            _ => Err(FAILED.to_owned()),
        };
        // A failure/rejection ends the session; do not retry or replay the input.
        let keep = result.as_ref().is_ok_and(|bytes| {
            serde_json::from_slice::<serde_json::Value>(bytes)
                .is_ok_and(|v| v.get("text").is_some())
        }) && epoch.load(Ordering::Acquire) == current
            && self.generation.load(Ordering::Acquire) == generation;
        if keep {
            resident.last_used = Instant::now();
        } else {
            slot.take();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: std::path::PathBuf,
        runtime: std::path::PathBuf,
        model: String,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "voice-reuse-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            // Invoke a stable interpreter. Executing freshly-written shebang files
            // in parallel tests races Linux fork-inherited write FDs (ETXTBSY).
            let interpreter = Command::new("python3")
                .args(["-c", "import sys; print(sys.executable)"])
                .output()
                .unwrap();
            assert!(interpreter.status.success());
            let runtime =
                std::path::PathBuf::from(String::from_utf8(interpreter.stdout).unwrap().trim());
            let model = root.join("helper.py");
            std::fs::write(
                &model,
                r#"#!/usr/bin/env python3
import json, os, pathlib, sys, time
pathlib.Path(sys.argv[0] + '.pid').write_text(str(os.getpid()))
for count, line in enumerate(sys.stdin, 1):
    request = json.loads(line)
    if request['transcript'] == 'wait': time.sleep(30)
    if request['transcript'] == 'oversize':
        print('x' * 30000, flush=True)
        continue
    print(json.dumps(dict(text=request['transcript'], pid=os.getpid(), count=count)), flush=True)
"#,
            )
            .unwrap();
            Self {
                root,
                runtime,
                model: model.to_string_lossy().into_owned(),
            }
        }
        fn request(
            &self,
            pool: &Pool,
            text: &str,
            epoch: &AtomicU64,
            current: u64,
            timeout: Duration,
        ) -> Result<serde_json::Value, String> {
            pool.request(
                &self.runtime,
                &self.model,
                "cpu",
                serde_json::to_vec(&serde_json::json!({"transcript": text, "context":""})).unwrap(),
                epoch,
                current,
                timeout,
            )
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| FAILED.into()))
        }
        fn pid(&self) -> String {
            let file = self.root.join("helper.py.pid");
            let start = Instant::now();
            while start.elapsed() < Duration::from_secs(5) {
                if let Ok(pid) = std::fs::read_to_string(&file)
                    && pid.parse::<u32>().is_ok()
                {
                    return pid;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("synthetic helper did not publish a PID");
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn assert_reaped(pid: &str) {
        assert!(pid.parse::<u32>().is_ok());
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
    #[test]
    fn reuses_one_process_without_replaying_the_previous_request_and_releases_on_settings_change() {
        let f = Fixture::new();
        let pool = Pool::default();
        let epoch = AtomicU64::new(1);
        let a = f
            .request(&pool, "first synthetic", &epoch, 1, Duration::from_secs(5))
            .unwrap();
        let b = f
            .request(&pool, "second synthetic", &epoch, 1, Duration::from_secs(5))
            .unwrap();
        assert_eq!(a["pid"], b["pid"]);
        assert_eq!(b["count"], 2);
        assert_eq!(b["text"], "second synthetic");
        pool.release();
        assert_reaped(&a["pid"].to_string());
        let c = f
            .request(&pool, "fresh", &epoch, 1, Duration::from_secs(5))
            .unwrap();
        assert_ne!(c["pid"], a["pid"]);
        assert_eq!(c["count"], 1);
        pool.release();
        assert_reaped(&c["pid"].to_string());
    }
    #[test]
    fn idle_expiry_and_model_replacement_reap_the_resident() {
        let f = Fixture::new();
        let pool = Pool::default();
        let epoch = AtomicU64::new(1);
        let a = f
            .request(&pool, "first", &epoch, 1, Duration::from_secs(5))
            .unwrap();
        pool.resident.lock().unwrap().as_mut().unwrap().last_used = Instant::now() - IDLE;
        pool.tick();
        assert_reaped(&a["pid"].to_string());
        let b = f
            .request(&pool, "second", &epoch, 1, Duration::from_secs(5))
            .unwrap();
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&f.model)
            .unwrap()
            .write_all(b"\n# replacement with different size\n")
            .unwrap();
        let c = f
            .request(&pool, "third", &epoch, 1, Duration::from_secs(5))
            .unwrap();
        assert_ne!(b["pid"], c["pid"]);
        assert_reaped(&b["pid"].to_string());
        pool.release();
    }
    #[test]
    fn cancellation_kills_and_reaps_before_a_new_request_can_start() {
        let f = Arc::new(Fixture::new());
        let pool = Arc::new(Pool::default());
        let epoch = Arc::new(AtomicU64::new(1));
        let worker = {
            let f = f.clone();
            let pool = pool.clone();
            let epoch = epoch.clone();
            std::thread::spawn(move || f.request(&pool, "wait", &epoch, 1, Duration::from_secs(5)))
        };
        let pid = f.pid();
        epoch.store(2, Ordering::Release);
        assert!(worker.join().unwrap().is_err());
        assert_reaped(&pid);
        let next = f
            .request(&pool, "new result", &epoch, 2, Duration::from_secs(5))
            .unwrap();
        assert_eq!(next["text"], "new result");
        assert_eq!(next["count"], 1);
        pool.release();
    }
    #[test]
    fn timeout_and_oversized_response_do_not_leave_a_child_or_replay_input() {
        let f = Fixture::new();
        let pool = Pool::default();
        let epoch = AtomicU64::new(1);
        assert!(
            f.request(&pool, "wait", &epoch, 1, Duration::from_millis(300))
                .is_err()
        );
        assert_reaped(&f.pid());
        assert!(
            f.request(&pool, "oversize", &epoch, 1, Duration::from_secs(5))
                .is_err()
        );
        assert_reaped(&f.pid());
        assert!(pool.resident.lock().unwrap().is_none());
    }
}
