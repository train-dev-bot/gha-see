use std::path::PathBuf;

#[test]
fn curated_samples_all_analyze() {
    let samples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples");
    let view = gha_see::analysis::analyze_path(&samples).expect("samples directory should analyze");
    let reusable = gha_see::analysis::analyze_path(&samples.join("09_local_reusable"))
        .expect("local reusable sample should analyze");

    assert_eq!(view.workflows.len(), 12);
    assert!(view.workflows.iter().all(|workflow| workflow.parse_ok));
    assert!(view
        .workflows
        .iter()
        .any(|wf| { wf.path.file_name().and_then(|n| n.to_str()) == Some("10_cicd_rust.yml") }));
    assert!(view.workflows.iter().any(|wf| {
        wf.path.file_name().and_then(|n| n.to_str()) == Some("11_cicd_typescript.yml")
    }));
    assert_eq!(reusable.workflows.len(), 2);
    assert!(reusable.workflows.iter().all(|workflow| workflow.parse_ok));
    assert!(view
        .bindings
        .iter()
        .any(|(_, binding)| binding.raw == "needs.build.outputs.artifact_name"));
}
