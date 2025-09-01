use crate::error::{Error, ErrorInfo, ErrorSetInfo};
use gloo_storage::{LocalStorage, Storage};
use log::error;
use reqwasm::http::Request;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use web_sys::window;

enum RequestMethod {
    Get,
    Post,
    Put,
    // PATCH,
    Delete,
}

//const API_ROOT: &str = "/api/v1";

const TOKEN_KEY: &str = "tuliprox.token";
pub fn get_token() -> Option<String> {
    LocalStorage::get(TOKEN_KEY).ok()
}

pub fn set_token(token: Option<&str>) {
    if let Some(t) = token {
        LocalStorage::set(TOKEN_KEY, String::from(t)).expect("failed to set");
    } else {
        LocalStorage::delete(TOKEN_KEY);
    }
}

/// build all kinds of http request: post/get/delete etc.
async fn request<B, T>(method: RequestMethod, url: &str, body: B, content_type: Option<String>, response_type: Option<String>) -> Result<T, Error>
where
    T: DeserializeOwned + 'static + std::fmt::Debug,
    B: Serialize + std::fmt::Debug,
{
    let c_type = content_type.as_ref().map_or("application/json", |c| c.as_str());
    let r_type = response_type.as_ref().map_or("application/json", |c| c.as_str());
    let mut request = match method {
        RequestMethod::Get => Request::get(url),
        RequestMethod::Post => Request::post(url).body(serde_json::to_string(&body).unwrap()),
        RequestMethod::Put => Request::put(url).body(serde_json::to_string(&body).unwrap()),
        // RequestMethod::PATCH =>  Request::patch(&url).body(serde_json::to_string(&body).unwrap()),
        RequestMethod::Delete => Request::delete(url),
    }.header("Content-Type", c_type);
    if let Some(token) = get_token() {
        request = request.header("Authorization", format!("Bearer {token}").as_str());
    }
    match request.send().await {
        Ok(response) => {
            match response.status() {
                200 => {
                    if r_type.eq("application/json") {
                        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<()>() {
                            // `T = ()` valid
                            let _ = response.text().await;
                            serde_json::from_str("null").map_err(|_| Error::DeserializeError)
                        } else {
                            let data: Result<T, _> = response.json::<T>().await;
                            if let Ok(data) = data {
                                Ok(data)
                            } else {
                                Err(Error::DeserializeError)
                            }
                        }
                    } else {
                        match response.text().await {
                            Ok(content) => {
                                match serde_json::from_value::<T>(Value::String(content)) {
                                    Ok(parsed) => Ok(parsed),
                                    Err(_err) => Err(Error::DeserializeError)
                                }
                            }
                            Err(_err) => Err(Error::RequestError),
                        }
                    }
                }
                400 =>  {
                    let data: Result<ErrorInfo, _> = response.json::<ErrorInfo>().await;
                    if let Ok(data) = data {
                        Err(Error::BadRequest(data.error))
                    } else {
                        Err(Error::BadRequest("400".to_string()))
                    }
                },
                401 => Err(Error::Unauthorized),
                403 => Err(Error::Forbidden),
                404 => Err(Error::NotFound),
                500 => Err(Error::InternalServerError),
                422 => {
                    let data: Result<ErrorSetInfo, _> = response.json::<ErrorSetInfo>().await;
                    if let Ok(data) = data {
                        Err(Error::UnprocessableEntity(data))
                    } else {
                        Err(Error::DeserializeError)
                    }
                }
                _ => Err(Error::RequestError),
            }
        }
        Err(e) => {
            error!("{e}");
            Err(Error::RequestError)
        }
    }
}

/// Delete request
pub async fn request_delete<T>(url: &str, content_type: Option<String>, response_type: Option<String>) -> Result<T, Error>
where
    T: DeserializeOwned + 'static + std::fmt::Debug,
{
    request(RequestMethod::Delete, url, (), content_type, response_type).await
}

/// Get request
pub async fn request_get<T>(url: &str, content_type: Option<String>, response_type: Option<String>) -> Result<T, Error>
where
    T: DeserializeOwned + 'static + std::fmt::Debug,
{
    request(RequestMethod::Get, url, (), content_type, response_type).await
}

// pub async fn request_get_api<T>(url: &str) -> Result<T, Error>
// where
//     T: DeserializeOwned + 'static + std::fmt::Debug,
// {
//     request(RequestMethod::Get, format!("{API_ROOT}{url}").as_str(), ()).await
// }

/// Post request with a body
pub async fn request_post<B, T>(url: &str, body: B, content_type: Option<String>, response_type: Option<String>) -> Result<T, Error>
where
    T: DeserializeOwned + 'static + std::fmt::Debug,
    B: Serialize + std::fmt::Debug,
{
    request(RequestMethod::Post, url, body, content_type, response_type).await
}

/// Put request with a body
pub async fn request_put<B, T>(url: &str, body: B, content_type: Option<String>, response_type: Option<String>) -> Result<T, Error>
where
    T: DeserializeOwned + 'static + std::fmt::Debug,
    B: Serialize + std::fmt::Debug,
{
    request(RequestMethod::Put, url, body, content_type, response_type).await
}

/// Set limit for pagination
pub fn limit(count: u32, p: u32) -> String {
    let offset = if p > 0 { p * count } else { 0 };
    format!("limit={count}&offset={offset}")
}

pub fn get_base_href() -> String {
    let mut href = window()
        .and_then(|w| w.document())
        .and_then(|doc| doc.query_selector("base").ok().flatten())
        .and_then(|base| base.get_attribute("href"))
        .map_or_else(|| "/".to_owned(), |s| s.trim().to_owned());

    if !href.ends_with('/') {
        href.push('/');
    }

    href
}