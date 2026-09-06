# gha-see sample workflows

Run the whole curated pack from the repository root:

```bash
cargo run -- samples/
```

Or open one focused example:

```bash
cargo run -- samples/03_dataflow_outputs.yml
```

The local reusable pair is nested so its relative path stays realistic; open
it with `cargo run -- samples/09_local_reusable/`.

The files are intentionally small and safe: gha-see analyzes their structure
and expressions but never executes their steps.

- `01_happy_path.yml` — one job with named `uses:` and `run:` steps.
- `02_matrix.yml` — a two-axis matrix with both `include` and `exclude`.
- `03_dataflow_outputs.yml` — job outputs consumed through
  `needs.build.outputs.artifact_name`; magenta “passes output” edges on the
  graph.
- `04_conditional_skip.yml` — job and step conditions driven by
  `github.event_name`; edit What-if context to try another event.
- `05_fan_in_out_dag.yml` — parallel build branches and a fan-in deployment
  chain large enough to exercise diagram navigation and panning.
- `06_concurrency_services.yml` — workflow/job concurrency and a PostgreSQL
  service declaration.
- `07_dispatch_inputs.yml` — typed `workflow_dispatch` inputs shown in
  What-if context.
- `08_remote_uses.yml` — marketplace actions for the explicit **Fetch remotes**
  / per-workflow **Fetch** demonstration (network access only when you click
  Fetch).
- `09_local_reusable/caller.yml` and `reusable.yml` — a local reusable
  workflow call resolved within the sample directory.
- `10_cicd_rust.yml` — maximal Rust CI/CD showcase (Test/SAST/Build →
  GitHub Release → CloudFormation staging/canary/promote/rollback/notify).
  Open this alone for a full-tour demo without stepping through `01`–`09`.
- `11_cicd_typescript.yml` — same job topology for TypeScript/Node
  (npm/eslint/tsc, CycloneDX); same depends-on / passes-output graph shape.
- `12_env_staging_prod.yml` — contiguous staging then production deploy
  chains for environment band experiments.
- `13_env_scattered.yml` — same env name on distant jobs (no single hull)
  plus a contiguous `release` pair.

In the web UI: the center pane shows one Needs + dataflow graph for the active
workflow; use **Fetch remotes** (or per-row **Fetch**) for pending remotes, and
**Apply** to recolor job/step run states.