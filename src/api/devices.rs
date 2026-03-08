use crate::api::AppState;
use crate::api::middleware::AuthUser;
use crate::api::schemas::devices::{CreateDeviceRequest, DeviceListResponse, DeviceResponse};
use crate::error::{AppError, Result};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use std::convert::TryInto;
use uuid::Uuid;

/// Creates a new device for the authenticated user and returns a full JWT.
///
/// # Errors
/// Returns `AppError::BadRequest` if validation fails or keys are malformed.
pub(crate) async fn create_device(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(payload): Json<CreateDeviceRequest>,
) -> Result<impl IntoResponse> {
    payload.validate().map_err(AppError::BadRequest)?;

    let session = state
        .device_service
        .create_device(
            auth_user.user_id,
            payload.name,
            payload.identity_key.try_into().map_err(AppError::BadRequest)?,
            payload.registration_id,
            payload.signed_pre_key.try_into().map_err(AppError::BadRequest)?,
            payload
                .one_time_pre_keys
                .into_iter()
                .map(TryInto::try_into)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(AppError::BadRequest)?,
        )
        .await?;

    let auth_response = crate::api::auth::map_session(session);
    Ok((StatusCode::CREATED, Json(auth_response)))
}

/// Lists all devices for the authenticated user.
///
/// # Errors
/// Returns `AppError::Database` if the query fails.
pub(crate) async fn list_devices(auth_user: AuthUser, State(state): State<AppState>) -> Result<impl IntoResponse> {
    let devices = state.device_service.list_devices(auth_user.user_id).await?;

    let response = DeviceListResponse {
        devices: devices
            .into_iter()
            .map(|d| DeviceResponse {
                device_id: d.id.to_string(),
                name: d.name,
                created_at: d
                    .created_at
                    .map(|ts| ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default())
                    .unwrap_or_default(),
            })
            .collect(),
    };

    Ok(Json(response))
}

/// Deletes a device owned by the authenticated user.
///
/// # Errors
/// Returns `AppError::NotFound` if the device doesn't exist or isn't owned by the user.
pub(crate) async fn delete_device(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    state.device_service.delete_device(device_id, auth_user.user_id).await?;
    Ok(StatusCode::OK)
}
