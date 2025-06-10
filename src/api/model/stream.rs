use crate::api::model::stream_error::StreamError;
use axum::http::StatusCode;
use bytes::Bytes;
use futures::stream::BoxStream;
use url::Url;

pub type BoxedProviderStream = BoxStream<'static, Result<Bytes, StreamError>>;
pub type ProviderStreamHeader = Vec<(String, String)>;
pub type ProviderStreamInfo = Option<(ProviderStreamHeader, StatusCode, Option<Url>)>;

pub type ProviderStreamResponse = (Option<BoxedProviderStream>, ProviderStreamInfo);

pub type ProviderStreamFactoryResponse = (BoxedProviderStream, ProviderStreamInfo);
