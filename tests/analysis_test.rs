use std::path::PathBuf;

use gha_see::analysis::{analyze_path, refetch_workflow_remotes_with, revaluate};
use gha_see::eval::{EvalContext, RunState};
use gha_see::fetch::FetchOutcome;
use gha_see::findings::{FindingTarget, Severity};

#[test]
fn analyze_fixtures_dir_reports_cycle_and_happy() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let view = analyze_path(&dir).unwrap();
    assert!(view.workflows.len() >= 2);
    assert!(view.findings.iter().any(|f| f.code == "GHA_CYCLE"));
}

#[test]
fn analyze_path_builds_one_graph_per_workflow() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let view = analyze_path(&dir).unwrap();
    assert_eq!(view.graphs.len(), view.workflows.len());
}

#[test]
fn analyze_path_preserves_raw_workflow_source() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/happy.yml");
    let expected = std::fs::read_to_string(&file).unwrap();

    let view = analyze_path(&file).unwrap();

    assert_eq!(
        view.workflows[0].raw_source.as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn analyze_path_errors_on_missing_root() {
    let missing =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/does-not-exist-dir");
    assert!(analyze_path(&missing).is_err());
}

#[test]
fn analyze_path_reports_missing_need() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bad_needs.yml");
    let view = analyze_path(&file).unwrap();

    assert!(view.findings.iter().any(|f| f.code == "GHA_MISSING_NEED"));
}

#[test]
fn analyze_path_expands_scalar_matrix_into_supported_instances() {
    // `deferred_matrix.yml`'s `strategy.matrix` is a plain scalar axis
    // (`node: [18, 20]`), which Phase A can expand — so it raises no
    // `GHA_DEFERRED`/`GHA_MATRIX_UNSUPPORTED` finding and each combination
    // gets its own `WillRun` instance, keyed by instance id.
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deferred_matrix.yml");
    let view = analyze_path(&file).unwrap();

    assert!(!view
        .findings
        .iter()
        .any(|f| f.code == "GHA_DEFERRED" || f.code == "GHA_MATRIX_UNSUPPORTED"));
    assert_eq!(
        view.job_states[&(file.clone(), "test (node=18)".to_string())],
        RunState::WillRun
    );
    assert_eq!(
        view.job_states[&(file, "test (node=20)".to_string())],
        RunState::WillRun
    );
}

#[test]
fn analyze_path_reports_dynamic_matrix_as_unsupported_and_deferred() {
    let file =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/matrix_unsupported.yml");
    let view = analyze_path(&file).unwrap();

    let finding = view
        .findings
        .iter()
        .find(|f| f.code == "GHA_MATRIX_UNSUPPORTED")
        .expect("matrix_unsupported.yml's dynamic `strategy.matrix` job should raise GHA_MATRIX_UNSUPPORTED");
    assert_eq!(
        finding.severity,
        Severity::Info,
        "GHA_MATRIX_UNSUPPORTED should be Info severity, not an Error/Warning — it's recognized, just not evaluated"
    );
    assert_eq!(
        view.job_states[&(file, "test".to_string())],
        RunState::Deferred
    );
}

#[test]
fn analyze_path_reports_workflow_concurrency_as_whatif_limit() {
    let dir = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_{}",
        std::process::id(),
        "workflow_concurrency"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("concurrency.yml"),
        r#"
name: Concurrency
on: push
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    )
    .unwrap();

    let view = analyze_path(&dir).unwrap();

    let finding = view
        .findings
        .iter()
        .find(|f| f.code == "GHA_WHATIF_LIMIT" && f.message.contains("concurrency"))
        .expect("workflow-level concurrency should raise a what-if limit finding");
    assert_eq!(finding.severity, Severity::Info);
    assert!(matches!(finding.target, FindingTarget::File(_)));
    assert!(!view
        .findings
        .iter()
        .any(|f| { f.code == "GHA_DEFERRED" && f.message.contains("concurrency") }));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_path_reports_service_and_environment_whatif_limits() {
    let dir = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_{}",
        std::process::id(),
        "job_extras_limits"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("extras.yml"),
        r#"
name: Extras
on: push
jobs:
  deploy:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:15
    environment: production
    steps:
      - run: echo hi
"#,
    )
    .unwrap();

    let view = analyze_path(&dir).unwrap();
    let limit_messages: Vec<&str> = view
        .findings
        .iter()
        .filter(|f| f.code == "GHA_WHATIF_LIMIT")
        .map(|f| f.message.as_str())
        .collect();
    assert!(limit_messages
        .iter()
        .any(|message| message.contains("service")));
    assert!(limit_messages
        .iter()
        .any(|message| message.contains("environment")));
    assert!(!view.findings.iter().any(|f| {
        f.code == "GHA_DEFERRED"
            && (f.message.contains("service") || f.message.contains("environment"))
    }));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_path_warns_for_write_all_permissions_on_pull_requests() {
    let dir = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_{}",
        std::process::id(),
        "pr_write_all"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("permissions.yml"),
        r#"
name: Permissions
on:
  pull_request:
permissions: write-all
jobs:
  build:
    runs-on: ubuntu-latest
    permissions: write-all
    steps:
      - run: echo hi
"#,
    )
    .unwrap();

    let view = analyze_path(&dir).unwrap();
    let warnings: Vec<_> = view
        .findings
        .iter()
        .filter(|f| f.code == "GHA_PERMISSIONS_WRITE_ALL")
        .collect();
    assert_eq!(warnings.len(), 2);
    assert!(warnings
        .iter()
        .all(|finding| finding.severity == Severity::Warning));
    assert!(warnings
        .iter()
        .any(|finding| matches!(finding.target, FindingTarget::File(_))));
    assert!(warnings
        .iter()
        .any(|finding| matches!(finding.target, FindingTarget::Job { .. })));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_path_ok_on_existing_dir_with_no_workflow_files() {
    // An existing directory with zero `.yml`/`.yaml` files (e.g. `.` run from
    // a repo root before any workflow has been added yet) is not an error:
    // `discover_workflows` returns an empty list, and `analyze_path` should
    // surface that as an empty-but-`Ok` `AnalysisView` for the TUI's empty
    // state, not a hard failure.
    let dir = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_{}",
        std::process::id(),
        "empty_dir_is_ok"
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let view = analyze_path(&dir).unwrap();

    assert!(view.workflows.is_empty());
    assert!(view.graphs.is_empty());
    assert!(view.bindings.is_empty());
    assert!(view.findings.is_empty());
    assert!(view.job_states.is_empty());
    assert!(view.step_states.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_path_errs_on_invalid_explicit_file_path() {
    // An explicit file path that exists but isn't a `.yml`/`.yaml` file is a
    // hard error, distinct from the "directory with no workflows" empty
    // state above — the user explicitly pointed at something that isn't a
    // workflow, so `analyze_path` should not silently return an empty view.
    let dir = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_{}",
        std::process::id(),
        "invalid_explicit_file"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let not_a_workflow = dir.join("notes.txt");
    std::fs::write(&not_a_workflow, "not yaml, not a workflow").unwrap();

    assert!(analyze_path(&not_a_workflow).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_path_traces_needs_outputs_bindings() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/outputs_trace.yml");
    let view = analyze_path(&file).unwrap();

    assert_eq!(view.bindings.len(), 1);
    let (path, binding) = &view.bindings[0];
    assert_eq!(*path, file);
    assert_eq!(binding.consumer_job, "test");
    assert_eq!(binding.producer_job, "build");
    assert_eq!(binding.output_name, "artifact-id");
}

#[test]
fn analyze_path_stubs_invalid_yaml_and_continues() {
    let dir = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_{}",
        std::process::id(),
        "stubs_invalid_yaml"
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ok.yml"), include_str!("fixtures/happy.yml")).unwrap();
    std::fs::write(dir.join("broken.yml"), "name: [unterminated").unwrap();

    let view = analyze_path(&dir).unwrap();

    assert_eq!(view.workflows.len(), 2);

    let broken = view
        .workflows
        .iter()
        .find(|wf| wf.path.ends_with("broken.yml"))
        .expect("broken.yml should still be represented as a stub");
    assert!(!broken.parse_ok);
    assert!(broken.jobs.is_empty());

    let ok = view
        .workflows
        .iter()
        .find(|wf| wf.path.ends_with("ok.yml"))
        .expect("ok.yml should parse normally");
    assert!(ok.parse_ok);

    assert!(view.findings.iter().any(|f| f.code == "GHA_YAML"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn revaluate_flips_skip_states() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skip_if.yml");
    let view = analyze_path(&file).unwrap();

    assert_eq!(
        view.job_states[&(file.clone(), "pr-only".to_string())],
        RunState::Skipped
    );
    assert_eq!(
        view.job_states[&(file.clone(), "always-push".to_string())],
        RunState::WillRun
    );

    let pr_ctx = EvalContext {
        event_name: "pull_request".to_string(),
        ..EvalContext::default_mock()
    };
    let updated = revaluate(&view, &pr_ctx);

    assert_eq!(
        updated.job_states[&(file.clone(), "pr-only".to_string())],
        RunState::WillRun
    );
    assert_eq!(
        updated.job_states[&(file, "always-push".to_string())],
        RunState::Skipped
    );
}

#[test]
fn revaluate_preserves_structural_findings_without_duplication() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cycle.yml");
    let view = analyze_path(&file).unwrap();
    let cycle_count = view
        .findings
        .iter()
        .filter(|f| f.code == "GHA_CYCLE")
        .count();
    assert_eq!(cycle_count, 1);

    let updated = revaluate(&view, &EvalContext::default_mock());
    let updated_cycle_count = updated
        .findings
        .iter()
        .filter(|f| f.code == "GHA_CYCLE")
        .count();
    assert_eq!(updated_cycle_count, 1);
}

#[test]
fn revaluate_replaces_condition_findings_instead_of_accumulating() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/skip_if.yml");
    let view = analyze_path(&file).unwrap();
    let skip_count = view
        .findings
        .iter()
        .filter(|f| f.code == "GHA_COND_SKIP")
        .count();
    assert_eq!(skip_count, 1);

    let updated = revaluate(&view, &EvalContext::default_mock());
    let updated_skip_count = updated
        .findings
        .iter()
        .filter(|f| f.code == "GHA_COND_SKIP")
        .count();
    assert_eq!(updated_skip_count, 1);
}

#[test]
fn refetch_expands_a_cache_seeded_remote_action_without_live_network() {
    let root = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_remote_cache",
        std::process::id()
    ));
    let cache = root.join("cache");
    let file = root.join(".github/workflows/remote.yml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: acme/actions/setup@v1\n",
    )
    .unwrap();
    let action = cache.join("gh/acme/actions/v1/setup/action.yml");
    std::fs::create_dir_all(action.parent().unwrap()).unwrap();
    std::fs::write(
        &action,
        "name: Cached setup\nruns:\n  using: composite\n  steps:\n    - uses: acme/nested/hello@v2\n",
    )
    .unwrap();
    let nested = cache.join("gh/acme/nested/v2/hello/action.yml");
    std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
    std::fs::write(
        &nested,
        "name: Nested cached\nruns:\n  using: composite\n  steps:\n    - run: echo cached\n",
    )
    .unwrap();

    let offline = analyze_path(&file).unwrap();
    assert!(offline
        .findings
        .iter()
        .any(|finding| finding.code == "GHA_USES_REMOTE"));

    let mut fetched = Vec::new();
    let refreshed =
        refetch_workflow_remotes_with(&offline, Some(0), &cache, |remote, cache_root| {
            fetched.push(remote.raw.clone());
            FetchOutcome::Cached {
                remote: remote.clone(),
                path: remote.cache_path(cache_root),
            }
        })
        .unwrap();
    let step = &refreshed.workflows[0].jobs["build"].steps[0];
    assert_eq!(step.resolved_action_name.as_deref(), Some("Cached setup"));
    assert_eq!(
        step.composite_steps[0].resolved_action_name.as_deref(),
        Some("Nested cached")
    );
    assert_eq!(fetched.len(), 2);
    assert!(!refreshed
        .findings
        .iter()
        .any(|finding| finding.code == "GHA_USES_REMOTE"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn refetch_soft_fails_one_remote_without_dropping_other_jobs() {
    let root = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_remote_soft_fail",
        std::process::id()
    ));
    let cache = root.join("cache");
    let file = root.join("remote.yml");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        &file,
        r#"
jobs:
  healthy:
    runs-on: ubuntu-latest
    steps:
      - run: echo healthy
  broken:
    runs-on: ubuntu-latest
    steps:
      - uses: acme/missing@v1
"#,
    )
    .unwrap();

    let offline = analyze_path(&file).unwrap();
    let refreshed = refetch_workflow_remotes_with(&offline, Some(0), &cache, |remote, _| {
        FetchOutcome::Failed {
            remote: remote.clone(),
            error: "HTTP 404".to_string(),
        }
    })
    .unwrap();

    assert!(refreshed.workflows[0].instances.contains_key("healthy"));
    assert_eq!(
        refreshed.job_states[&(file.clone(), "healthy".to_string())],
        RunState::WillRun
    );
    let fetch_finding = refreshed
        .findings
        .iter()
        .find(|finding| finding.code == "GHA_USES_FETCH")
        .expect("failed remote should produce a fetch finding");
    assert!(fetch_finding.message.contains("HTTP 404"));
    assert!(!fetch_finding.coach.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn remote_uses_sample_declares_marketplace_actions() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples/08_remote_uses.yml");
    let view = analyze_path(&file).unwrap();
    assert!(
        gha_see::analysis::workflow_has_remote_uses(&view.workflows[0]),
        "08_remote_uses.yml should declare marketplace remote uses"
    );
}

#[test]
fn uncached_remote_uses_need_fetch_when_not_in_cache() {
    // Unique owner/repo so the developer cache cannot satisfy analyze_path.
    let root = std::env::temp_dir().join(format!(
        "gha_see_analysis_test_{}_uncached_remote",
        std::process::id()
    ));
    let file = root.join("remote.yml");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        &file,
        "jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: gha-see-test/never-cached-action@v1\n",
    )
    .unwrap();

    let view = analyze_path(&file).unwrap();
    assert!(
        gha_see::analysis::workflow_needs_remote_fetch(&view.workflows[0]),
        "uncached remote uses should still need Fetch"
    );
    assert!(view
        .findings
        .iter()
        .any(|finding| finding.code == "GHA_USES_REMOTE"));

    let _ = std::fs::remove_dir_all(root);
}
