use gha_see::findings::{
    condition_skips, condition_unknown, cycle, deferred, expr_syntax, matrix_cap, matrix_empty,
    matrix_unsupported, missing_need, permissions_write_all, uses_fetch, uses_missing, uses_remote,
    whatif_limit, workflow_call_cycle, yaml_parse, FindingTarget, Severity,
};
use std::path::PathBuf;

#[test]
fn cycle_finding_has_coach_text() {
    let f = cycle(PathBuf::from("x.yml"), &["a".into(), "b".into()]);
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(f.code, "GHA_CYCLE");
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn missing_need_finding_has_coach_text() {
    let f = missing_need(PathBuf::from("x.yml"), "b", "a");
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(f.code, "GHA_MISSING_NEED");
    assert_eq!(
        f.target,
        FindingTarget::Job {
            file: PathBuf::from("x.yml"),
            job: "b".to_string(),
        }
    );
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn yaml_parse_finding_has_coach_text() {
    let f = yaml_parse(
        PathBuf::from("bad.yml"),
        "mapping values are not allowed here",
    );
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(f.code, "GHA_YAML");
    assert_eq!(f.target, FindingTarget::File(PathBuf::from("bad.yml")));
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn expr_syntax_finding_has_coach_text() {
    let target = FindingTarget::Step {
        file: PathBuf::from("x.yml"),
        job: "build".to_string(),
        step_idx: 0,
    };
    let f = expr_syntax(target.clone(), "unmatched `${{`");
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.code, "GHA_EXPR_SYNTAX");
    assert_eq!(f.target, target);
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn deferred_finding_has_coach_text() {
    let target = FindingTarget::Job {
        file: PathBuf::from("x.yml"),
        job: "build".to_string(),
    };
    let f = deferred(target.clone(), "strategy.matrix not evaluated in v1");
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.code, "GHA_DEFERRED");
    assert_eq!(f.target, target);
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn condition_unknown_finding_has_coach_text() {
    let target = FindingTarget::Job {
        file: PathBuf::from("x.yml"),
        job: "build".to_string(),
    };
    let f = condition_unknown(target.clone());
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.code, "GHA_COND_UNKNOWN");
    assert_eq!(f.target, target);
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn condition_skips_finding_has_coach_text() {
    let target = FindingTarget::Job {
        file: PathBuf::from("x.yml"),
        job: "build".to_string(),
    };
    let f = condition_skips(target.clone(), "github.event_name == 'pull_request'");
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.code, "GHA_COND_SKIP");
    assert_eq!(f.target, target);
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn matrix_empty_finding_has_coach_text() {
    let f = matrix_empty(PathBuf::from("x.yml"), "test");
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.code, "GHA_MATRIX_EMPTY");
    assert_eq!(
        f.target,
        FindingTarget::Job {
            file: PathBuf::from("x.yml"),
            job: "test".to_string(),
        }
    );
    assert!(!f.message.is_empty());
    assert!(!f.coach.is_empty());
}

#[test]
fn matrix_cap_finding_has_coach_text() {
    let f = matrix_cap(PathBuf::from("x.yml"), "test", 320, 256);
    assert_eq!(f.severity, Severity::Warning);
    assert_eq!(f.code, "GHA_MATRIX_CAP");
    assert!(f.message.contains("320"));
    assert!(f.message.contains("256"));
    assert!(!f.coach.is_empty());
}

#[test]
fn matrix_unsupported_finding_has_coach_text() {
    let f = matrix_unsupported(
        PathBuf::from("x.yml"),
        "test",
        "strategy.matrix is not a mapping",
    );
    assert_eq!(f.severity, Severity::Info);
    assert_eq!(f.code, "GHA_MATRIX_UNSUPPORTED");
    assert!(f.message.contains("strategy.matrix is not a mapping"));
    assert!(!f.coach.is_empty());
}

#[test]
fn file_target_variant_is_constructible() {
    let target = FindingTarget::File(PathBuf::from("x.yml"));
    match target {
        FindingTarget::File(p) => assert_eq!(p, PathBuf::from("x.yml")),
        _ => panic!("expected File variant"),
    }
}

#[test]
fn uses_findings_have_expected_severity_and_coach_text() {
    let target = FindingTarget::Job {
        file: PathBuf::from("x.yml"),
        job: "call".to_string(),
    };
    let remote = uses_remote(target.clone(), "owner/repo@v1");
    assert_eq!(remote.code, "GHA_USES_REMOTE");
    assert_eq!(remote.severity, Severity::Info);
    assert!(!remote.coach.is_empty());

    let missing = uses_missing(target, "./missing");
    assert_eq!(missing.code, "GHA_USES_MISSING");
    assert_eq!(missing.severity, Severity::Error);
    assert!(!missing.coach.is_empty());

    let fetch = uses_fetch(
        FindingTarget::Job {
            file: PathBuf::from("x.yml"),
            job: "call".to_string(),
        },
        "owner/repo@v1",
        "HTTP 404",
    );
    assert_eq!(fetch.code, "GHA_USES_FETCH");
    assert_eq!(fetch.severity, Severity::Warning);
    assert!(fetch.message.contains("HTTP 404"));
    assert!(!fetch.coach.is_empty());
}

#[test]
fn workflow_call_cycle_finding_has_coach_text() {
    let file = PathBuf::from("a.yml");
    let finding = workflow_call_cycle(
        file.clone(),
        &[
            PathBuf::from("a.yml"),
            PathBuf::from("b.yml"),
            PathBuf::from("a.yml"),
        ],
    );
    assert_eq!(finding.code, "GHA_WORKFLOW_CALL");
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.target, FindingTarget::File(file));
    assert!(!finding.coach.is_empty());
}

#[test]
fn whatif_limit_is_an_info_finding() {
    let target = FindingTarget::Job {
        file: PathBuf::from("x.yml"),
        job: "build".to_string(),
    };
    let finding = whatif_limit(
        target.clone(),
        "service containers are displayed but not started",
    );
    assert_eq!(finding.code, "GHA_WHATIF_LIMIT");
    assert!(finding.message.starts_with("What-if limit: "));
    assert_eq!(finding.severity, Severity::Info);
    assert_eq!(finding.target, target);
    assert!(!finding.coach.is_empty());
}

#[test]
fn write_all_permissions_on_pull_requests_warns() {
    let target = FindingTarget::File(PathBuf::from("x.yml"));
    let finding = permissions_write_all(target.clone());
    assert_eq!(finding.code, "GHA_PERMISSIONS_WRITE_ALL");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.target, target);
    assert!(!finding.message.is_empty());
    assert!(!finding.coach.is_empty());
}
