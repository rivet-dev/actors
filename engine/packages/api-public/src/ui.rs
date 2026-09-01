use axum::{
	http::{StatusCode, header},
	response::{IntoResponse, Response},
};
use include_dir::{Dir, include_dir};
use rivet_api_builder::extract::Path;

static UI_DIR: Dir<'_> = include_dir!("$OUT_DIR/ui");

#[tracing::instrument(skip_all)]
pub async fn serve_index() -> Response {
	if let Some(index_file) = UI_DIR.get_file("index.html") {
		([(header::CONTENT_TYPE, "text/html")], index_file.contents()).into_response()
	} else {
		(StatusCode::NOT_FOUND, "index.html not found").into_response()
	}
}

#[tracing::instrument(skip_all)]
pub async fn serve_ui(Path(path): Path<String>) -> Response {
	let file_path = path.trim_start_matches('/');

	if let Some(res) = serve_file(file_path) {
		res
	} else {
		serve_index().await
	}
}

#[tracing::instrument(skip_all)]
pub async fn serve_asset(Path(path): Path<String>) -> Response {
	let file_path = format!("assets/{}", path.trim_start_matches('/'));

	serve_file(&file_path).unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn serve_file(file_path: &str) -> Option<Response> {
	let file = UI_DIR.get_file(file_path)?;

	let content_type = match file_path.split('.').last() {
		Some("html") => "text/html",
		Some("css") => "text/css",
		Some("js") => "application/javascript",
		Some("json") => "application/json",
		Some("png") => "image/png",
		Some("jpg") | Some("jpeg") => "image/jpeg",
		Some("svg") => "image/svg+xml",
		Some("ico") => "image/x-icon",
		Some("woff") => "font/woff",
		Some("woff2") => "font/woff2",
		Some("ttf") => "font/ttf",
		Some("eot") => "application/vnd.ms-fontobject",
		_ => "application/octet-stream",
	};

	Some(([(header::CONTENT_TYPE, content_type)], file.contents()).into_response())
}
