# WGC Stability And Recovery Gate

Use this gate only for a controlled Windows Graphics Capture (WGC) field run. It does not infer a pass from application logs, a GPU capability probe, a DXGI fallback, or a fan-out benchmark.

```powershell
pnpm benchmark:screen-share:wgc-stability-gate -- --manifest artifacts/screen-share-benchmarks/wgc-stability-host-a/wgc-gate-manifest.json --output artifacts/screen-share-benchmarks/wgc-stability-host-a/wgc-gate.json --markdown artifacts/screen-share-benchmarks/wgc-stability-host-a/wgc-gate.md
```

The evidence report must use `scope: "wgc-stability-recovery-evidence"`. A passing report records all of the following with attributed measurements: a completed capture of at least 30 minutes, resource growth and no unacceptable leak, successful lock-screen and display-reconfiguration recovery, coverage of at least two monitors, black-frame counts, recovery event accounting, frame continuity after recovery, and one or more external artifact references with SHA-256 hashes.

The gate output always uses `scope: "wgc_stability_recovery"`, `spec_completion: "not_evaluated"`, and a recommended exit code: `0` passed, `1` failed threshold or explicit fault, `2` incomplete field or evidence, and `3` invalid manifest or report. `--collect-only` returns process exit code 0 without changing the JSON recommendation.

Reference the resulting `wgc-gate.json` from the `wgc_stability_recovery` entry in the full-spec evidence manifest. It is one required gate and cannot alone declare the complete screen-share specification passed.

