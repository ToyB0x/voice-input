//! Text-free overlay state, driven by monotonic time and a recording identity.
use serde::Serialize;

#[derive(Clone, Default)]
pub(crate) struct Signal {
    pub recording_id: u64,
    pub phase: &'static str,
    pub recording_ms: u64,
    pub level: f32,
    pub recognizing: bool,
    pub loading: bool,
    pub discarding: bool,
    pub correcting: bool,
    pub has_result: bool,
    pub failed: bool,
    pub fallback: bool,
    pub cancelled: bool,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub visible: bool,
    pub phase: &'static str,
    pub message: &'static str,
    pub recording_ms: u64,
    pub stage_ms: u64,
    pub level: f32,
}
#[derive(Default)]
pub(crate) struct Tracker {
    seen: Option<u64>,
    active: Option<u64>,
    stage: &'static str,
    stage_since: u64,
    terminal_since: Option<u64>,
}
impl Tracker {
    pub fn tick(&mut self, enabled: bool, signal: &Signal, delivery: &str, now: u64) -> Status {
        if signal.phase == "recording" && self.seen != Some(signal.recording_id) {
            self.seen = Some(signal.recording_id);
            self.active = enabled.then_some(signal.recording_id);
            self.stage = "";
            self.terminal_since = None;
        }
        if !enabled || signal.phase == "idle" || signal.loading || signal.discarding {
            self.active = None;
        }
        if self.active != Some(signal.recording_id) {
            return Status::default();
        }
        let (phase, message, terminal) = if signal.phase == "recording" {
            ("recording", "録音中", false)
        } else if signal.recognizing {
            ("recognizing", "音声を認識中", false)
        } else if signal.correcting {
            ("correcting", "ローカル補正中・入力は待機", false)
        } else if signal.cancelled {
            ("cancelled", "取り消しました・自動入力しません", true)
        } else if signal.failed {
            ("error", "処理できませんでした・設定画面を確認", true)
        } else if signal.has_result && delivery == "blocked" {
            ("blocked", "入力を見送りました・結果は設定画面に保持", true)
        } else if signal.fallback {
            ("fallback", "補正失敗・Whisper原文を使用", true)
        } else if signal.has_result && delivery == "inserted" {
            ("inserted", "入力しました", true)
        } else {
            ("complete", "完了・結果は設定画面で確認できます", true)
        };
        if self.stage != phase {
            self.stage = phase;
            self.stage_since = now;
        }
        if terminal {
            let start = *self.terminal_since.get_or_insert(now);
            if now.saturating_sub(start) >= 4000 {
                self.active = None;
                return Status::default();
            }
        } else {
            self.terminal_since = None;
        }
        Status {
            visible: true,
            phase,
            message,
            recording_ms: signal.recording_ms,
            stage_ms: now.saturating_sub(self.stage_since),
            level: if phase == "recording" && signal.level.is_finite() {
                signal.level.clamp(0.0, 1.0)
            } else {
                0.0
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recording(id: u64) -> Signal {
        Signal {
            recording_id: id,
            phase: "recording",
            recording_ms: 1234,
            level: 0.1,
            ..Signal::default()
        }
    }
    #[test]
    fn recording_to_correction_and_delivery_expires_without_reappearing() {
        let mut tracker = Tracker::default();
        let mut signal = recording(1);
        assert_eq!(tracker.tick(true, &signal, "armed", 0).phase, "recording");
        signal.phase = "recorded";
        signal.recognizing = true;
        assert_eq!(
            tracker.tick(true, &signal, "armed", 20).phase,
            "recognizing"
        );
        signal.recognizing = false;
        signal.correcting = true;
        assert_eq!(tracker.tick(true, &signal, "armed", 50).stage_ms, 0);
        assert_eq!(tracker.tick(true, &signal, "armed", 1050).stage_ms, 1000);
        signal.correcting = false;
        signal.has_result = true;
        let done = tracker.tick(true, &signal, "inserted", 1100);
        assert_eq!(done.phase, "inserted");
        assert_eq!(done.level, 0.0);
        assert!(tracker.tick(true, &signal, "inserted", 5099).visible);
        assert!(!tracker.tick(true, &signal, "inserted", 5100).visible);
        assert!(!tracker.tick(true, &signal, "inserted", 5200).visible);
        assert!(tracker.tick(true, &recording(2), "armed", 5300).visible);
    }
    #[test]
    fn disabling_clear_and_manual_work_never_open_an_overlay() {
        let mut tracker = Tracker::default();
        let mut signal = recording(1);
        assert!(!tracker.tick(false, &signal, "idle", 0).visible);
        assert!(!tracker.tick(true, &signal, "idle", 10).visible); // Enable applies to the next recording.
        signal = recording(2);
        assert!(tracker.tick(true, &signal, "idle", 20).visible);
        assert!(!tracker.tick(false, &signal, "idle", 30).visible);
        signal.phase = "recorded";
        signal.correcting = true;
        assert!(!tracker.tick(true, &signal, "idle", 40).visible);
        signal = recording(3);
        assert!(tracker.tick(true, &signal, "idle", 50).visible);
        signal.phase = "idle";
        assert!(!tracker.tick(true, &signal, "idle", 60).visible);
        signal.phase = "recorded";
        signal.recognizing = true;
        assert!(!tracker.tick(true, &signal, "idle", 70).visible);
    }
    #[test]
    fn outcome_priority_and_body_free_payload() {
        for (cancelled, failed, fallback, delivery, expected) in [
            (false, false, false, "blocked", "blocked"),
            (false, false, true, "inserted", "fallback"),
            (true, false, true, "blocked", "cancelled"),
            (false, true, false, "blocked", "error"),
        ] {
            let mut tracker = Tracker::default();
            let mut signal = recording(1);
            tracker.tick(true, &signal, "armed", 0);
            signal.phase = "recorded";
            signal.has_result = true;
            signal.cancelled = cancelled;
            signal.failed = failed;
            signal.fallback = fallback;
            let status = tracker.tick(true, &signal, delivery, 1);
            assert_eq!(status.phase, expected);
            let json = serde_json::to_value(status).unwrap();
            let keys: Vec<_> = json
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                vec![
                    "level",
                    "message",
                    "phase",
                    "recordingMs",
                    "stageMs",
                    "visible"
                ]
            );
        }
    }
}
