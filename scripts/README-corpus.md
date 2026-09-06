# Workflow corpus (optional)

gha-see does not download workflow YAML from the network. To soak-test parsing and analysis against many real-world files, vendor samples under tests/corpus/ locally (gitignored or committed at your discretion).

## Curating samples

1. Copy .yml / .yaml workflow files into tests/corpus/.
2. Prefer small, representative workflows; avoid secrets and org-specific paths.
3. For inspiration and links to community collections, see the [awesome-actions](https://github.com/sdras/awesome-actions) index (link index only — do not fetch from the binary).

## Running the soak test

The integration test in tests/corpus_smoke.rs is ignored by default so CI and normal cargo test stays fast.

    cargo test --test corpus_smoke -- --ignored

If tests/corpus/ is missing, the ignored test returns immediately without failure.
