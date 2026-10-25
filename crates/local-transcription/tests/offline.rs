use local_transcription::{Backend, Error, Transcriber, prepare_audio, read_wav};
use std::path::Path;

/// Explicit opt-in: the ordinary test suite never downloads a model.
#[test]
#[ignore = "requires VOICE_INPUT_TEST_MODEL; run in the network-denied CI process"]
fn japanese_and_silence_without_network() {
    let path = std::env::var_os("VOICE_INPUT_TEST_MODEL").expect("set VOICE_INPUT_TEST_MODEL");
    let model = Transcriber::load(Path::new(&path), Backend::Cpu).unwrap();
    let audio = read_wav(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ja-oku.wav"
    )))
    .unwrap();
    let text = model.transcribe(&audio).unwrap();
    eprintln!("Japanese transcript: {text}");
    for word in ["月日", "旅"] {
        assert!(text.contains(word), "missing {word}: {text}");
    }
    assert_eq!(model.transcribe(&vec![0.0; 32_000]).unwrap(), "");
    let hint = "旅のメモ: 月日、百代、過客、旅人。";
    let text = model.transcribe_with_hint(&audio, Some(hint)).unwrap();
    for word in ["月日", "旅"] {
        assert!(text.contains(word), "hinted recognition lost {word}");
    }
    assert_eq!(
        model
            .transcribe_with_hint(&vec![0.0; 32_000], Some(hint))
            .unwrap(),
        ""
    );
    // Every call creates a fresh decoder state: unrelated hints cannot persist.
    let unhinted = model.transcribe(&audio).unwrap();
    assert!(!unhinted.contains("ゼノヴァリンク"));
    // Exercise the device-rate path too: 16 kHz WAV-only checks bypass resampling.
    let device_audio: Vec<f32> = audio.iter().flat_map(|sample| [*sample; 3]).collect();
    let resampled = prepare_audio(&device_audio, 48_000).unwrap();
    assert_eq!(resampled.len(), audio.len());
    let resampled_text = model.transcribe(&resampled).unwrap();
    for word in ["月日", "旅"] {
        assert!(
            resampled_text.contains(word),
            "resampling lost {word}: {resampled_text}"
        );
    }
    assert!(matches!(
        model.transcribe(&[f32::NAN; 16_000]),
        Err(Error::InvalidAudio)
    ));
    assert!(matches!(model.transcribe(&[]), Err(Error::InvalidAudio)));
}
