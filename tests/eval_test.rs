use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use gha_see::eval::{evaluate, EvalContext, NeedStatus, RunState};
use gha_see::ir::normalize;
use gha_see::parse::parse_workflow_str;

#[test]
fn skips_pr_job_on_push_context() {
    let raw = parse_workflow_str(include_str!("fixtures/skip_if.yml")).unwrap();
    let wf = normalize(PathBuf::from("skip_if.yml"), raw);
    let ctx = EvalContext {
        event_name: "push".into(),
        ..EvalContext::default_mock()
    };

    let (states, findings) = evaluate(&wf, &ctx);

    assert_eq!(states["pr-only"], RunState::Skipped);
    assert_eq!(states["always-push"], RunState::WillRun);
    assert!(findings.iter().any(|f| f.code == "GHA_COND_SKIP"));
}

#[test]
fn runs_pr_job_on_pull_request_context() {
    let raw = parse_workflow_str(include_str!("fixtures/skip_if.yml")).unwrap();
    let wf = normalize(PathBuf::from("skip_if.yml"), raw);
    let ctx = EvalContext {
        event_name: "pull_request".into(),
        ..EvalContext::default_mock()
    };

    let (states, _findings) = evaluate(&wf, &ctx);

    assert_eq!(states["pr-only"], RunState::WillRun);
    assert_eq!(states["always-push"], RunState::Skipped);
}

#[test]
fn unknown_for_unsupported_fn() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    if: hashFiles('**/x') == ''
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let ctx = EvalContext::default_mock();

    let (states, findings) = evaluate(&wf, &ctx);

    assert_eq!(states["build"], RunState::Unknown);
    assert!(findings.iter().any(|f| f.code == "GHA_COND_UNKNOWN"));
}

#[test]
fn default_mock_is_push_to_main_with_no_env() {
    let ctx = EvalContext::default_mock();
    assert_eq!(ctx.event_name, "push");
    assert_eq!(ctx.ref_name, "refs/heads/main");
    assert!(ctx.env.is_empty());
}

#[test]
fn no_condition_always_runs() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build"], RunState::WillRun);
    assert!(findings.is_empty());
}

#[test]
fn true_and_false_literals() {
    let yaml = r#"
name: N
on: push
jobs:
  always-on:
    if: 'true'
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
  never-on:
    if: 'false'
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["always-on"], RunState::WillRun);
    assert_eq!(states["never-on"], RunState::Skipped);
}

#[test]
fn success_and_always_are_true_failure_and_cancelled_are_false() {
    let yaml = r#"
name: N
on: push
jobs:
  a:
    if: success()
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
  b:
    if: always()
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
  c:
    if: failure()
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
  d:
    if: cancelled()
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["a"], RunState::WillRun);
    assert_eq!(states["b"], RunState::WillRun);
    assert_eq!(states["c"], RunState::Skipped);
    assert_eq!(states["d"], RunState::Skipped);
}

#[test]
fn and_or_not_and_parentheses_combine() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    if: "!(github.event_name == 'pull_request') && (github.ref == 'refs/heads/main' || false)"
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build"], RunState::WillRun);
}

#[test]
fn env_condition_resolves_when_env_var_set() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    if: env.DEPLOY == 'yes'
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);

    let mut env = BTreeMap::new();
    env.insert("DEPLOY".to_string(), "yes".to_string());
    let ctx = EvalContext {
        env,
        ..EvalContext::default_mock()
    };

    let (states, _findings) = evaluate(&wf, &ctx);
    assert_eq!(states["build"], RunState::WillRun);
}

#[test]
fn env_condition_is_unknown_when_env_var_unset() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    if: env.DEPLOY == 'yes'
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build"], RunState::Unknown);
    assert!(findings.iter().any(|f| f.code == "GHA_COND_UNKNOWN"));
}

#[test]
fn deferred_job_state_matches_support_tier() {
    // Unlike a plain scalar `strategy.matrix` (see
    // `matrix_instances_get_independent_run_states` below), a
    // dynamic/expression-valued matrix isn't expandable in v1, so the job
    // stays a single `Deferred` instance.
    let yaml = r#"
name: N
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix: ${{ fromJson(needs.setup.outputs.matrix) }}
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["test"], RunState::Deferred);
}

#[test]
fn matrix_instances_get_independent_run_states() {
    let raw = parse_workflow_str(include_str!("fixtures/deferred_matrix.yml")).unwrap();
    let wf = normalize(PathBuf::from("deferred_matrix.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["test (node=18)"], RunState::WillRun);
    assert_eq!(states["test (node=20)"], RunState::WillRun);
}

#[test]
fn skipped_upstream_skips_dependent_job_regardless_of_its_own_condition() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    if: "false"
    runs-on: ubuntu-latest
    steps: [{run: echo build}]
  deploy:
    needs: [build]
    runs-on: ubuntu-latest
    steps: [{run: echo deploy}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build"], RunState::Skipped);
    assert_eq!(states["deploy"], RunState::Skipped);
}

#[test]
fn unknown_upstream_makes_dependent_unknown() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    if: hashFiles('**/x') == ''
    runs-on: ubuntu-latest
    steps: [{run: echo build}]
  deploy:
    needs: [build]
    runs-on: ubuntu-latest
    steps: [{run: echo deploy}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build"], RunState::Unknown);
    assert_eq!(states["deploy"], RunState::Unknown);
}

#[test]
fn step_level_condition_can_skip_within_a_running_job() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo always
      - run: echo pr-only
        if: github.event_name == 'pull_request'
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build"], RunState::WillRun);
    assert_eq!(states["build#0"], RunState::WillRun);
    assert_eq!(states["build#1"], RunState::Skipped);
    assert!(findings.iter().any(|f| f.code == "GHA_COND_SKIP"));
}

#[test]
fn steps_are_skipped_when_their_job_is_skipped() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    if: "false"
    runs-on: ubuntu-latest
    steps: [{run: echo hi}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("n.yml"), raw);
    let (states, _findings) = evaluate(&wf, &EvalContext::default_mock());

    assert_eq!(states["build"], RunState::Skipped);
    assert_eq!(states["build#0"], RunState::Skipped);
}

fn evaluate_condition(condition: &str, ctx: &EvalContext) -> RunState {
    let yaml = format!(
        r#"
name: N
on: push
jobs:
  test:
    if: {condition}
    runs-on: ubuntu-latest
    steps: [{{run: echo hi}}]
"#
    );
    let raw = parse_workflow_str(&yaml).unwrap();
    let wf = normalize(PathBuf::from("condition.yml"), raw);
    evaluate(&wf, ctx).0["test"]
}

#[test]
fn string_functions_evaluate_case_insensitively() {
    let ctx = EvalContext::default_mock();
    assert_eq!(
        evaluate_condition(r#""contains(github.ref, 'HEADS/MAIN')""#, &ctx),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(r#""startsWith(github.ref, 'REFS/HEADS')""#, &ctx),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(r#""endsWith(github.ref, '/MAIN')""#, &ctx),
        RunState::WillRun
    );
}

#[test]
fn format_and_join_produce_comparable_values() {
    let ctx = EvalContext::default_mock();
    assert_eq!(
        evaluate_condition(
            r#""format('{0}/{1}', 'octo', 'repo') == 'octo/repo'""#,
            &ctx
        ),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(
            r#""join(fromJSON('[\"one\",\"two\"]'), ':') == 'one:two'""#,
            &ctx
        ),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(r#""join('[\"one\",\"two\"]', ',') == 'one,two'""#, &ctx),
        RunState::WillRun
    );
}

#[test]
fn json_functions_round_trip_arrays_and_objects() {
    let ctx = EvalContext::default_mock();
    assert_eq!(
        evaluate_condition(
            r#""contains(fromJSON('[\"push\",\"pull_request\"]'), github.event_name)""#,
            &ctx
        ),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(
            r#""toJSON(fromJSON('{\"ok\":true}')) == '{\"ok\":true}'""#,
            &ctx
        ),
        RunState::WillRun
    );
}

#[test]
fn ordered_comparisons_support_numbers_and_strings() {
    let ctx = EvalContext::default_mock();
    assert_eq!(
        evaluate_condition(r#""2 < 10 && 10 >= 10""#, &ctx),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(r#""'alpha' <= 'beta' && 'zeta' > 'beta'""#, &ctx),
        RunState::WillRun
    );
}

#[test]
fn extended_contexts_resolve_and_secrets_stay_unknown() {
    let mut ctx = EvalContext::default_mock();
    ctx.github.sha = "abc123".into();
    ctx.github.repository = "octo/repo".into();
    ctx.github.actor = "octocat".into();
    ctx.vars.insert("CHANNEL".into(), "stable".into());
    ctx.inputs.insert("deploy".into(), "yes".into());
    ctx.secrets.insert("TOKEN".into());

    assert_eq!(
        evaluate_condition(
            r#""github.ref_name == 'main' && github.sha == 'abc123' && github.repository == 'octo/repo' && github.actor == 'octocat' && vars.CHANNEL == 'stable' && inputs.deploy == 'yes'""#,
            &ctx
        ),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(r#""secrets.TOKEN == 'invented'""#, &ctx),
        RunState::Unknown
    );
}

#[test]
fn supplied_needs_outputs_resolve() {
    let mut ctx = EvalContext::default_mock();
    ctx.needs.insert(
        "build".into(),
        NeedStatus {
            result: "success".into(),
            outputs: BTreeMap::from([("artifact".into(), "app".into())]),
        },
    );
    assert_eq!(
        evaluate_condition(
            r#""needs.build.result == 'success' && needs.build.outputs.artifact == 'app'""#,
            &ctx
        ),
        RunState::WillRun
    );
}

#[test]
fn evaluate_wires_needs_result_from_upstream_run_state() {
    let yaml = r#"
name: N
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps: [{run: echo build}]
  deploy:
    needs: build
    if: needs.build.result == 'success'
    runs-on: ubuntu-latest
    steps: [{run: echo deploy}]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("needs.yml"), raw);
    let (states, _) = evaluate(&wf, &EvalContext::default_mock());
    assert_eq!(states["build"], RunState::WillRun);
    assert_eq!(states["deploy"], RunState::WillRun);
}

#[test]
fn hash_files_hashes_only_local_matches() {
    let root = std::env::temp_dir().join(format!("gha-see-hashfiles-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/input.txt"), b"hello").unwrap();

    let ctx = EvalContext {
        repo_root: Some(root.clone()),
        ..EvalContext::default_mock()
    };
    assert_eq!(
        evaluate_condition(r#""hashFiles('src/*.txt') != ''""#, &ctx),
        RunState::WillRun
    );
    assert_eq!(
        evaluate_condition(r#""hashFiles('../outside') != ''""#, &ctx),
        RunState::Unknown
    );

    fs::remove_dir_all(root).unwrap();
}
