use gha_see::ir::{normalize, Permissions, SupportTier};
use gha_see::matrix::MatrixNote;
use gha_see::parse::parse_workflow_str;
use std::path::PathBuf;

#[test]
fn matrix_job_expands_to_supported_instances() {
    // A plain scalar `strategy.matrix` (as opposed to a dynamic/expression
    // matrix — see `dynamic_matrix_job_stays_deferred`) is expandable in
    // Phase A, so the job itself is `Supported` and gets one instance per
    // combination rather than being `Deferred` as a whole.
    let raw = parse_workflow_str(include_str!("fixtures/deferred_matrix.yml")).unwrap();
    let wf = normalize(PathBuf::from("deferred_matrix.yml"), raw);
    let job = wf.jobs.get("test").unwrap();
    assert_eq!(job.support, SupportTier::Supported);
    assert!(job.deferred_reasons.is_empty());
    assert_eq!(job.matrix_note, None);

    assert_eq!(wf.instances.len(), 2);
    for id in ["test (node=18)", "test (node=20)"] {
        let instance = wf
            .instances
            .get(id)
            .unwrap_or_else(|| panic!("missing instance {id}"));
        assert_eq!(instance.support, SupportTier::Supported);
        assert_eq!(instance.base_id, "test");
    }
}

#[test]
fn dynamic_matrix_job_stays_deferred() {
    let yaml = r#"
name: DynamicMatrix
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix: ${{ fromJson(needs.setup.outputs.matrix) }}
    steps:
      - run: echo hi
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("dynamic_matrix.yml"), raw);
    let job = wf.jobs.get("test").unwrap();
    assert_eq!(job.support, SupportTier::Deferred);
    assert!(matches!(job.matrix_note, Some(MatrixNote::Unsupported(_))));

    assert_eq!(wf.instances.len(), 1);
    let instance = &wf.instances["test"];
    assert_eq!(instance.support, SupportTier::Deferred);
    assert!(instance.matrix.is_empty());
}

#[test]
fn happy_job_supported() {
    let raw = parse_workflow_str(include_str!("fixtures/happy.yml")).unwrap();
    let wf = normalize(PathBuf::from("happy.yml"), raw);
    assert_eq!(wf.jobs["build"].support, SupportTier::Supported);
    assert!(wf.jobs["build"].deferred_reasons.is_empty());
    assert_eq!(wf.jobs["build"].runs_on.as_deref(), Some("ubuntu-latest"));

    // Jobs without `strategy.matrix` get exactly one instance whose id
    // matches the base job id.
    assert_eq!(wf.instances.len(), 1);
    let instance = &wf.instances["build"];
    assert_eq!(instance.instance_id, "build");
    assert_eq!(instance.base_id, "build");
    assert!(instance.matrix.is_empty());
}

#[test]
fn reusable_workflow_job_is_deferred() {
    let yaml = r#"
name: Reusable
on: push
jobs:
  call-it:
    uses: octo-org/octo-repo/.github/workflows/reusable.yml@main
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("reusable.yml"), raw);
    let job = wf.jobs.get("call-it").unwrap();
    assert_eq!(job.support, SupportTier::Deferred);
    assert!(job.deferred_reasons.is_empty());
    assert_eq!(
        job.uses.as_deref(),
        Some("octo-org/octo-repo/.github/workflows/reusable.yml@main")
    );
}

#[test]
fn non_string_runs_on_is_deferred_with_best_effort_stringify() {
    let yaml = r#"
name: Labels
on: push
jobs:
  build:
    runs-on: [self-hosted, linux]
    steps:
      - run: echo hi
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("labels.yml"), raw);
    let job = wf.jobs.get("build").unwrap();
    assert_eq!(job.support, SupportTier::Deferred);
    assert!(job.deferred_reasons.iter().any(|r| r.contains("runs-on")));
    assert_eq!(job.runs_on.as_deref(), Some("self-hosted, linux"));
}

#[test]
fn job_with_concurrency_is_typed_and_supported() {
    let yaml = r#"
name: Concurrency
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    concurrency:
      group: build-${{ github.ref }}
      cancel-in-progress: true
    steps:
      - run: echo hi
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("concurrency.yml"), raw);
    let job = wf.jobs.get("build").unwrap();
    assert_eq!(job.support, SupportTier::Supported);
    assert!(job.deferred_reasons.is_empty());
    let concurrency = job.concurrency.as_ref().expect("typed concurrency");
    assert_eq!(concurrency.group, "build-${{ github.ref }}");
    assert_eq!(concurrency.cancel_in_progress, Some(true));
    assert_eq!(
        wf.instances["build"].concurrency.as_ref().unwrap().group,
        concurrency.group
    );
}

#[test]
fn job_with_services_is_typed_and_supported() {
    let yaml = r#"
name: Services
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:15
        ports: [5432]
        env:
          POSTGRES_DB: app
        options: --health-cmd pg_isready
    steps:
      - run: echo hi
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("services.yml"), raw);
    let job = wf.jobs.get("build").unwrap();
    assert_eq!(job.support, SupportTier::Supported);
    assert!(job.deferred_reasons.is_empty());
    let service = &job.services["postgres"];
    assert_eq!(service.image.as_deref(), Some("postgres:15"));
    assert_eq!(service.ports, vec!["5432"]);
    assert_eq!(service.env["POSTGRES_DB"], "app");
    assert_eq!(service.options.as_deref(), Some("--health-cmd pg_isready"));
    assert!(wf.instances["build"].services.contains_key("postgres"));
}

#[test]
fn workflow_and_job_extras_are_typed_and_defaults_inherit_into_steps() {
    let yaml = r#"
name: Extras
on: pull_request
concurrency: workflow-${{ github.ref }}
permissions: write-all
defaults:
  run:
    shell: bash
    working-directory: scripts
jobs:
  deploy:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      deployments: write
    environment:
      name: production
      url: https://example.test
    defaults:
      run:
        working-directory: deploy
    steps:
      - run: ./deploy.sh
      - run: ./verify.ps1
        shell: pwsh
        working-directory: verify
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("extras.yml"), raw);

    let workflow_concurrency = wf.concurrency.as_ref().expect("workflow concurrency");
    assert_eq!(workflow_concurrency.group, "workflow-${{ github.ref }}");
    assert_eq!(workflow_concurrency.cancel_in_progress, None);
    assert!(matches!(wf.permissions, Some(Permissions::WriteAll)));
    let workflow_defaults = wf.defaults_run.as_ref().expect("workflow defaults.run");
    assert_eq!(workflow_defaults.shell.as_deref(), Some("bash"));
    assert_eq!(
        workflow_defaults.working_directory.as_deref(),
        Some("scripts")
    );

    let job = &wf.jobs["deploy"];
    let Permissions::Map(permission_map) = job.permissions.as_ref().expect("job permissions")
    else {
        panic!("expected permission map");
    };
    assert_eq!(permission_map["contents"], "read");
    assert_eq!(permission_map["deployments"], "write");
    let environment = job.environment.as_ref().expect("job environment");
    assert_eq!(environment.name, "production");
    assert_eq!(environment.url.as_deref(), Some("https://example.test"));
    assert_eq!(
        job.defaults_run
            .as_ref()
            .unwrap()
            .working_directory
            .as_deref(),
        Some("deploy")
    );

    assert_eq!(job.steps[0].shell.as_deref(), Some("bash"));
    assert_eq!(job.steps[0].working_directory.as_deref(), Some("deploy"));
    assert_eq!(job.steps[1].shell.as_deref(), Some("pwsh"));
    assert_eq!(job.steps[1].working_directory.as_deref(), Some("verify"));

    let instance = &wf.instances["deploy"];
    assert_eq!(instance.environment.as_ref().unwrap().name, "production");
    assert!(matches!(instance.permissions, Some(Permissions::Map(_))));
}

#[test]
fn local_reusable_uses_awaits_cold_path_resolution() {
    let yaml = r#"
name: LocalReusable
on: push
jobs:
  call-it:
    uses: ./.github/workflows/reusable.yml
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("local_reusable.yml"), raw);
    let job = wf.jobs.get("call-it").unwrap();
    assert_eq!(job.support, SupportTier::Supported);
    assert!(job.deferred_reasons.is_empty());
    assert_eq!(
        job.uses.as_deref(),
        Some("./.github/workflows/reusable.yml")
    );
}

#[test]
fn mapping_runs_on_falls_back_to_none() {
    let yaml = r#"
name: GroupRunner
on: push
jobs:
  build:
    runs-on:
      group: ubuntu-runners
    steps:
      - run: echo hi
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = normalize(PathBuf::from("group.yml"), raw);
    let job = wf.jobs.get("build").unwrap();
    assert_eq!(job.support, SupportTier::Deferred);
    assert_eq!(job.runs_on.as_deref(), Some("ubuntu-runners"));
}
