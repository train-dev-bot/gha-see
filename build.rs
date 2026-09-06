//! Ensure `web/dist` exists for `rust-embed` before the crate compiles.
//!
//! Without this, an empty or missing `web/dist` at first build embeds nothing
//! permanently until a clean rebuild — Cargo does not notice later asset adds.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let web_dir = manifest_dir.join("web");
    let dist_dir = web_dir.join("dist");
    let dist_index = dist_dir.join("index.html");

    println!("cargo:rerun-if-env-changed=GHA_SEE_SKIP_WEB_BUILD");
    println!("cargo:rerun-if-env-changed=GHA_SEE_FORCE_WEB_BUILD");
    for path in [
        "web/package.json",
        "web/package-lock.json",
        "web/vite.config.ts",
        "web/tsconfig.json",
        "web/tsconfig.app.json",
        "web/tsconfig.node.json",
        "web/index.html",
        "web/dist/index.html",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    rerun_if_changed_walk(&web_dir.join("src"));
    rerun_if_changed_walk(&dist_dir.join("assets"));

    if env::var_os("GHA_SEE_SKIP_WEB_BUILD").is_some() {
        if !dist_index.is_file() {
            panic!(
                "web/dist/index.html is missing and GHA_SEE_SKIP_WEB_BUILD is set; \
                 run `npm run build` in web/ or unset the env var"
            );
        }
        return;
    }

    // Packaged / cloned trees already contain web/dist. Do not require Node
    // unless the caller explicitly forces a rebuild.
    if env::var_os("GHA_SEE_FORCE_WEB_BUILD").is_none() && dist_index.is_file() {
        return;
    }

    if !needs_web_build(&web_dir, &dist_index) {
        return;
    }

    println!("cargo:warning=building web UI (npm) into web/dist …");
    ensure_npm_available();
    ensure_node_modules(&web_dir);
    run_npm_build(&web_dir);

    if !dist_index.is_file() {
        panic!("web/dist/index.html missing after `npm run build` — frontend build failed");
    }
}

fn needs_web_build(web_dir: &Path, dist_index: &Path) -> bool {
    if !dist_index.is_file() {
        return true;
    }
    let Ok(dist_mtime) = fs::metadata(dist_index).and_then(|m| m.modified()) else {
        return true;
    };

    let mut inputs: Vec<PathBuf> = vec![
        web_dir.join("package.json"),
        web_dir.join("package-lock.json"),
        web_dir.join("vite.config.ts"),
        web_dir.join("tsconfig.json"),
        web_dir.join("tsconfig.app.json"),
        web_dir.join("tsconfig.node.json"),
        web_dir.join("index.html"),
    ];
    collect_files(&web_dir.join("src"), &mut inputs);

    inputs.iter().any(|path| is_newer(path, dist_mtime))
}

fn is_newer(path: &Path, than: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|mtime| mtime > than)
        .unwrap_or(false)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn rerun_if_changed_walk(dir: &Path) {
    if !dir.exists() {
        println!("cargo:rerun-if-changed={}", dir.display());
        return;
    }
    let mut files = Vec::new();
    collect_files(dir, &mut files);
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn ensure_npm_available() {
    let ok = Command::new("npm")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        panic!(
            "web/dist is missing or stale, but `npm` was not found on PATH. \
             Install Node.js or run `npm run build` in web/ once, then rebuild."
        );
    }
}

fn ensure_node_modules(web_dir: &Path) {
    if web_dir.join("node_modules").is_dir() {
        return;
    }
    let status = Command::new("npm")
        .args(["ci"])
        .current_dir(web_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn `npm ci` in web/: {e}"));
    if !status.success() {
        let fallback = Command::new("npm")
            .args(["install"])
            .current_dir(web_dir)
            .status()
            .unwrap_or_else(|e| panic!("failed to spawn `npm install` in web/: {e}"));
        if !fallback.success() {
            panic!("`npm ci` / `npm install` failed in web/ (status {fallback})");
        }
    }
}

fn run_npm_build(web_dir: &Path) {
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(web_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn `npm run build` in web/: {e}"));
    if !status.success() {
        panic!("`npm run build` failed in web/ (status {status})");
    }
}
