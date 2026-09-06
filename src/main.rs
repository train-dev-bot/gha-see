use std::env;
use std::path::PathBuf;
use std::process;

fn usage() -> ! {
    eprintln!(
        "Usage:
  gha-see [path]              Start web UI (default) and open browser
  gha-see --web [path]        Same as default
  gha-see --web --no-open [path]
  gha-see --json [path]       Dump AnalysisView as WebView JSON
  gha-see --help"
    );
    process::exit(2);
}

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
    }

    let mut mode = Mode::Web;
    let mut open_browser = true;
    let mut path = PathBuf::from(".");

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--web" | "--ui" => {
                mode = Mode::Web;
                i += 1;
            }
            "--json" => {
                mode = Mode::Json;
                i += 1;
            }
            "--no-open" => {
                open_browser = false;
                i += 1;
            }
            other if other.starts_with('-') => {
                eprintln!("unknown flag: {other}");
                usage();
            }
            other => {
                path = PathBuf::from(other);
                i += 1;
            }
        }
    }
    let _ = &mut args;

    match mode {
        Mode::Json => match gha_see::analysis::analyze_path(&path) {
            Ok(view) => {
                let ctx = gha_see::eval::EvalContext::default_mock_for(&view.workflows);
                let web = gha_see::api::WebView::from_analysis(&view, &path, &ctx);
                match serde_json::to_string_pretty(&web) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("json error: {e}");
                        process::exit(2);
                    }
                }
            }
            Err(e) => {
                eprintln!("gha-see: {e}");
                process::exit(1);
            }
        },
        Mode::Web => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            if let Err(e) = rt.block_on(gha_see::api::run_server(path, open_browser)) {
                eprintln!("gha-see: {e}");
                process::exit(1);
            }
        }
    }
}

enum Mode {
    Web,
    Json,
}
