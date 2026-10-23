# Japanese ASR fixture

`ja-oku.wav` is the first 12 seconds of:

- Title: **Oku no Hosomichi - introductory sentences - 2025 7 17**
- Creator credit on source: **Nesnad & AI**
- Source: https://commons.wikimedia.org/wiki/File:Oku_no_Hosomichi_-_introductory_sentences_-_2025_7_17.ogg
- Original: https://upload.wikimedia.org/wikipedia/commons/8/8e/Oku_no_Hosomichi_-_introductory_sentences_-_2025_7_17.ogg
- License: **CC0 1.0**, https://creativecommons.org/publicdomain/zero/1.0/
- Original SHA-256: `ed3b7c246c480a1b35cc3970a37db4dc6c7a30715772ed57e3fbadc5ab2de2bb`
- Converted SHA-256: `4c236b692fbea9b009191ff9f18c498143094d38eed1da1843ff825b14b6390f`
- Changes: trimmed to 12 seconds, downmixed/resampled to mono 16 kHz signed 16-bit PCM WAV.

Conversion:

```sh
ffmpeg -i original.ogg -t 12 -ar 16000 -ac 1 -c:a pcm_s16le ja-oku.wav
```

The passage starts with 「月日は百代の過客にして、行きかふ年もまた旅人なり」
(Matsuo Bashō, public-domain text). CI checks a few key words rather than exact
punctuation/orthography. This short literary/synthetic sample is a wiring and
Japanese-language smoke test, **not** a benchmark of dictation accuracy or natural
speech. No user recordings are stored here or in CI artifacts.
