//! HTTP API + JSON DTOs for the local web UI.

mod dto;
mod fs;
mod server;

pub use dto::{EvalContextDto, WebView};
pub use fs::{list_directory, FsEntry, FsListResponse};
pub use server::{build_router, run_server, AppState};
