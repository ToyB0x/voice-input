# Japanese ASR comparison

Fixed 7-utterance FLEURS read-speech subset + digital silence; two measured repetitions with a retained model.
CER: NFKC/casefold, punctuation and whitespace removed. Lower is better; spelling variants remain errors.

| Model | Backend | CER | RTF | Median inference (s) | Peak process RSS (MiB) | Load (s) | Cold first inference (s) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| base | cpu | 34.98% | 0.149 | 1.280 | 443.7 | 16.885 | 1.455 |
| base | metal | 34.98% | 0.470 | 3.904 | 549.1 | 0.172 | 3.590 |
| small | cpu | 15.27% | 0.453 | 3.962 | 878.0 | 15.322 | 4.576 |
| small | metal | 15.27% | 1.134 | 10.481 | 1036.8 | 0.635 | 9.199 |

Provisional CPU ranking on this fixed subset: speed = **base**, accuracy = **small**.

These are small-sample results on GitHub macOS runners, not a general model recommendation or a physical-Mac GPU comparison.
Peak RSS is per-process ru_maxrss and does not measure all GPU/driver allocations. Cold first inference uses the first corpus case, outside measured repetitions.
Per-case references, hypotheses, raw/normalized errors, timings and host metadata are included in the JSON/CSV artifacts.
