# Japanese evaluation fixtures

Seven full human-read utterances from **Google FLEURS**, `ja_jp`, validation split.
Attribution: **Alexis Conneau, Min Ma, Simran Khanuja, Yu Zhang, Vera Axelrod,
Siddharth Dalmia, Jason Riesa, Clara Rivera, Ankur Bapna** (2022),
*FLEURS: Few-shot Learning Evaluation of Universal Representations of Speech*.
https://arxiv.org/abs/2205.12446

- Dataset/source card: https://huggingface.co/datasets/google/fleurs
- Audio and text license: **CC BY 4.0**, https://creativecommons.org/licenses/by/4.0/
  (license text included in `CC-BY-4.0.txt`). No endorsement by the creators is implied.
- Pinned converted dataset revision: `168de341b3db6859a9bac1c50a2ef5e3b47647e0`
- Original parquet: https://huggingface.co/datasets/google/fleurs/resolve/168de341b3db6859a9bac1c50a2ef5e3b47647e0/ja_jp/validation/0000.parquet
- Selected rows: **2, 59, 104, 234, 192, 27, 243**, chosen by domain/length before running model inference.
- Changes: decoded to mono, 16 kHz, signed 16-bit PCM WAV. Full utterances retained, no trimming.
  Reference strings use the original `raw_transcription`, without correction based on model output.
- `corpus.json` records source sentence IDs, original paths, gender labels, original byte hashes,
  converted file hashes, reference text and categories for each clip.
- `silence.wav`: generated two seconds of digital zero PCM, no external source.

This is a small **read-speech** subset with daily expressions, requests,
work/business-related text, numbers and proper nouns. It is not spontaneous
conversation or a representative benchmark of business dictation; category labels
indicate content, not recording circumstances. Multiple gender labels are present;
the source rows do not provide individual speaker IDs. Do not infer a speaker count.
The data may have appeared in model training; these results do not establish unseen-data accuracy.

To reproduce, download the pinned parquet, install `pyarrow` and `ffmpeg`, then run
`python3 scripts/prepare-evaluation-corpus.py PATH_TO_PARQUET` from the repository.
The script checks the parquet hash before selecting rows and records per-source checksums.
CI uses committed WAVs and does not need these dataset-preparation dependencies.
