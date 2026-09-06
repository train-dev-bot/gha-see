use std::path::PathBuf;

use axum::serve;
use gha_see::analysis::analyze_path;
use gha_see::api::{build_router, AppState};

#[tokio::test]
async fn get_index_and_view_from_samples() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("samples");
    let view = analyze_path(&root).expect("samples dir should analyze");
    let state = AppState::from_analysis(root, view);
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        serve(listener, app).await.expect("server");
    });

    let base = format!("http://{addr}");
    let (index_status, index_body, view_status, workflows_len) =
        tokio::task::spawn_blocking(move || {
            let index = reqwest::blocking::get(format!("{base}/")).expect("GET /");
            let index_status = index.status().as_u16();
            let index_body = index.text().expect("index body");
            let view = reqwest::blocking::get(format!("{base}/api/view")).expect("GET /api/view");
            let view_status = view.status().as_u16();
            let json: serde_json::Value = view.json().expect("view json");
            let workflows_len = json
                .get("workflows")
                .and_then(|w| w.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            (index_status, index_body, view_status, workflows_len)
        })
        .await
        .expect("http client");

    server.abort();

    assert_eq!(index_status, 200);
    assert!(
        index_body.contains("<title>gha-see</title>"),
        "index body should include <title>gha-see</title>, got: {index_body}"
    );
    assert_eq!(view_status, 200);
    assert!(
        workflows_len >= 1,
        "GET /api/view workflows should be non-empty"
    );
}
