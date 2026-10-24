# Initial macOS baseline

Unmodified `model-evaluation-summary` artifact from [Actions run 40042105375](https://github.com/ToyB0x/voice-input/actions/runs/40042105375), PR #7 head `35c137757b77f4b0fbe4b07056775cbd7e8f9675`.
The PR workflow checked out merge revision `28dfbe23992404fc6152dd2767793957df60900b`; all four reports have that revision and a clean working tree.

- [summary.md](summary.md): generated comparison table.
- [comparison.json](comparison.json): full references, hypotheses, measurements, host metadata and content hashes.
- [recommendations.json](recommendations.json): generated provisional CPU ranking on this subset only.

These historical results are not regenerated when application code changes. Re-run the workflow to measure new code and compare matching protocols/corpora.
CPU then Metal ran on the same runner for each model; base and small used separate runners with the same reported virtual hardware (Apple M1, 3 logical CPUs, 7 GiB RAM, macOS 14).
Model files were prepared before the network-denied benchmark. Load times include filesystem/OS cache effects; CPU ran first, so CPU/Metal load times are not an unbiased backend comparison.

Reference text reproduced from FLEURS is CC BY 4.0; attribution, source and transformations are in [fixtures/README.md](../fixtures/README.md), with the [license](../fixtures/CC-BY-4.0.txt).
See [evaluation protocol and limits](../../docs/model-evaluation.md). No audio or model weights are duplicated here.
