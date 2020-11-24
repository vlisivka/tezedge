// Copyright (c) SimpleStaking and Tezedge Contributors
// SPDX-License-Identifier: MIT
#![forbid(unsafe_code)]

use hyper::{Body, Response, StatusCode};
use slog::{error, Logger};

pub use services::mempool_services::MempoolOperations;

pub mod encoding;
mod helpers;
pub mod rpc_actor;
mod server;
mod services;

/// Crate level custom result
pub(crate) type ServiceResult = Result<Response<Body>, Box<dyn std::error::Error + Sync + Send>>;

/// Generate options response with supported methods, headers
pub(crate) fn options() -> ServiceResult {
    Ok(Response::builder()
        .status(StatusCode::from_u16(200)?)
        .header(hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS, PUT")
        .body(Body::empty())?)
}

/// Function to generate JSON response from serializable object
pub(crate) fn make_json_response<T: serde::Serialize>(content: &T) -> ServiceResult {
    Ok(Response::builder()
        .header(hyper::header::CONTENT_TYPE, "application/json")
        // TODO: add to config
        .header(hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS, PUT")
        .body(Body::from(serde_json::to_string(content)?))?)
}

/// Function to generate JSON response from a stream
pub(crate) fn make_json_stream_response<T: futures::Stream<Item=Result<String, serde_json::Error>> + Send + 'static>(content: T) -> ServiceResult {
    Ok(Response::builder()
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS, PUT")
        .header(hyper::header::TRANSFER_ENCODING, "chunked")
        .body(Body::wrap_stream(content))?)
}

/// Returns result as a JSON response.
pub(crate) fn result_to_json_response<T: serde::Serialize>(res: Result<T, failure::Error>, log: &Logger) -> ServiceResult {
    match res {
        Ok(t) => make_json_response(&t),
        Err(err) => {
            error!(log, "Failed to execute RPC function"; "reason" => format!("{:?}", &err));
            error(err)
        }
    }
}

/// Returns optional result as a JSON response.
pub(crate) fn result_option_to_json_response<T: serde::Serialize>(res: Result<Option<T>, failure::Error>, log: &Logger) -> ServiceResult {
    match res {
        Ok(opt) => match opt {
            Some(t) => make_json_response(&t),
            None => not_found()
        }
        Err(err) => {
            error!(log, "Failed to execute RPC function"; "reason" => format!("{:?}", &err));
            error(err)
        }
    }
}

/// Generate empty response
pub(crate) fn empty() -> ServiceResult {
    Ok(Response::builder()
        .status(StatusCode::from_u16(204)?)
        .body(Body::empty())?)
}

/// Generate 404 response
pub(crate) fn not_found() -> ServiceResult {
    Ok(Response::builder()
        .status(StatusCode::from_u16(404)?)
        .body(Body::from("not found"))?)
}

/// Generate 500 error
pub(crate) fn error(error: failure::Error) -> ServiceResult {
    Ok(Response::builder()
        .status(StatusCode::from_u16(500)?)
        .header(hyper::header::CONTENT_TYPE, "text/plain")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type")
        .header(hyper::header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type")
        .header(hyper::header::TRANSFER_ENCODING, "chunked")
        .body(Body::from(format!("{:?}", error)))?)
}
