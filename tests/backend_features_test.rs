use std::fs;
use std::path::PathBuf;

use gha_see::analysis::analyze_yaml_source;
use gha_see::api::{list_directory, WebView};
use gha_see::eval::EvalContext;
use gha_see::expr::scan_workflow;
use gha_see::ir::normalize;
use gha_see::parse::parse_workflow_str;

#[test]
fn bindings_record_the_consuming_step_index() {
    let raw = parse_workflow_str(
        r#"
jobs:
  build:
    runs-on: ubuntu-latest
    outputs:
      artifact: value
    steps:
      - run: echo build
  deploy:
    needs: build
    runs-on: ubuntu-latest
    if: ${{ needs.build.outputs.artifact }}
    steps:
      - run: echo ${{ needs.build.outputs.artifact }}
"#,
    )
    .unwrap();
    let workflow = normalize(PathBuf::from("workflow.yml"), raw);
    let (bindings, findings) = scan_workflow(&workflow);

    assert!(findings.is_empty());
    assert!(bindings.iter().any(|binding| {
        binding.consumer_job == "deploy" && binding.consumer_step_idx.is_none()
    }));
    assert!(bindings.iter().any(|binding| {
        binding.consumer_job == "deploy" && binding.consumer_step_idx == Some(0)
    }));
}

#[test]
fn web_view_serializes_binding_step_index_as_camel_case() {
    let view = analyze_yaml_source(
        "binding.yml",
        r#"
jobs:
  build:
    runs-on: ubuntu-latest
    outputs:
      artifact: value
    steps:
      - run: echo build
  deploy:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - run: echo ${{ needs.build.outputs.artifact }}
"#,
    );
    let context = EvalContext::default_mock_for(&view.workflows);
    let web_view = WebView::from_analysis(&view, PathBuf::from("(scratch)").as_path(), &context);
    let json = serde_json::to_value(web_view).unwrap();

    assert_eq!(json["bindings"][0]["consumerStepIdx"], 0);
}

#[test]
fn analyzes_yaml_source_under_a_safe_scratch_path() {
    let view = analyze_yaml_source(
        "../paste.yml",
        "jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hello\n",
    );

    assert_eq!(view.workflows.len(), 1);
    assert_eq!(
        view.workflows[0].path,
        PathBuf::from("(scratch)/..paste.yml")
    );
    assert_eq!(
        view.workflows[0].raw_source.as_deref(),
        Some("jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hello\n")
    );
}

#[test]
fn lists_directories_before_workflow_and_regular_files() {
    let root = std::env::temp_dir().join(format!(
        "gha_see_fs_list_{}_{}",
        std::process::id(),
        "directory_order"
    ));
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("workflow.yaml"), "jobs: {}").unwrap();
    fs::write(root.join("notes.txt"), "notes").unwrap();
    let root = fs::canonicalize(&root).unwrap();

    let listing = list_directory(&root).unwrap();

    assert_eq!(listing.path, root.display().to_string());
    assert_eq!(
        listing.parent,
        root.parent().map(|path| path.display().to_string())
    );
    assert_eq!(
        listing
            .entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("nested", "dir"),
            ("workflow.yaml", "workflow"),
            ("notes.txt", "file")
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn up_from_relative_directory_lists_parent() {
    let base = std::env::temp_dir().join(format!(
        "gha_see_fs_up_{}_{}",
        std::process::id(),
        "relative_parent"
    ));
    let child = base.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::write(child.join("workflow.yml"), "jobs: {}").unwrap();

    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&base).unwrap();
    let listing = list_directory(std::path::Path::new("child"));
    let _ = std::env::set_current_dir(&previous);

    let listing = listing.expect("relative child directory should list");
    let parent = listing
        .parent
        .as_deref()
        .expect("relative child must expose a real parent");
    assert!(
        !parent.is_empty(),
        "parent must not be the empty path that fails list"
    );
    let parent_listing = list_directory(std::path::Path::new(parent)).expect("Up target");
    assert!(
        parent_listing
            .entries
            .iter()
            .any(|entry| entry.name == "child" && entry.kind == "dir"),
        "parent listing should include child"
    );

    let _ = fs::remove_dir_all(base);
}
