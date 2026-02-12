use axum::{
    http::{Response, header},
    response::IntoResponse,
};

/// Returns the `OpenAPI` specification in YAML format.
///
/// # Panics
/// Panics if the response builder fails to construct the response.
pub async fn openapi_yaml() -> impl IntoResponse {
    let spec = include_str!("../../openapi.yaml");
    let version = env!("CARGO_PKG_VERSION");
    let spec_with_version = spec.replace("version: 0.0.0", &format!("version: {version}"));

    Response::builder()
        .header(header::CONTENT_TYPE, "text/yaml")
        .body(spec_with_version)
        .expect("Failed to construct OpenAPI YAML response")
}
