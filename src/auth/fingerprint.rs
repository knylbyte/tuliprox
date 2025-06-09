use std::net::SocketAddr;
use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::StatusCode;
use crate::auth::Rejection;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Fingerprint(pub String);

impl<B> FromRequestParts<B> for Fingerprint
where
    B: Send + Sync,
{
    type Rejection = Rejection;

    async fn from_request_parts(req: &mut Parts, state: &B) -> Result<Self, Self::Rejection> {
        Self::decode_request_parts(req, state).await
    }
}

impl Fingerprint {

    async fn decode_request_parts<B>(req: &mut Parts, state: &B) -> Result<Self, Rejection>
    where
        B: Send + Sync,
    {
        let ConnectInfo(addr) = ConnectInfo::<SocketAddr>::from_request_parts(req, state)
            .await
            .map_err(|_| (StatusCode::BAD_REQUEST, "IP-Addr is missing"))?;

        let user_agent = req
            .headers
            .get(axum::http::header::USER_AGENT)
            .ok_or((StatusCode::BAD_REQUEST, "User-Agent header is missing"))?
            .to_str()
            .map_err(|_| (StatusCode::BAD_REQUEST, "User-Agent header contains invalid characters"))?;

        let x_forwarded_for = req
            .headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .map(std::string::ToString::to_string);

        let key = match x_forwarded_for {
            Some(xff) => format!("{}{xff}{user_agent}", addr.ip()),
            None => format!("{}{user_agent}", addr.ip()),
        };

        Ok(Fingerprint(key))
    }
}