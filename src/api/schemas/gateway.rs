use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct WsParams {
    pub ticket: String,
}

#[derive(Debug, Serialize)]
pub struct TicketResponse {
    pub ticket: String,
}
