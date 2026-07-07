#![cfg(feature = "compression")]

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, CompressionOptions, HttpMethod, RouteDefinition,
};
use flate2::read::GzDecoder;
use serde_json::json;
use std::io::Read;

#[tokio::test]
async fn compression_interceptor_gzips_accepted_responses() {
    let app = BootApplication::builder()
        .use_global_compression(CompressionOptions::new().with_min_size(1))
        .route(
            RouteDefinition::get("/hello", |_| async {
                Ok(BootResponse::text("hello hello hello hello"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/hello").with_header("accept-encoding", "gzip"))
        .await
        .unwrap();

    assert_eq!(response.header("content-encoding"), Some("gzip"));
    assert_eq!(response.header("vary"), Some("accept-encoding"));
    assert_eq!(
        decode_gzip(response.body()).unwrap(),
        "hello hello hello hello"
    );
}

#[tokio::test]
async fn compression_interceptor_skips_when_gzip_is_not_accepted() {
    let app = BootApplication::builder()
        .use_global_compression(CompressionOptions::new().with_min_size(1))
        .route(
            RouteDefinition::get("/hello", |_| async {
                Ok(BootResponse::text("hello hello hello hello"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let no_header = app
        .call(BootRequest::new(HttpMethod::Get, "/hello"))
        .await
        .unwrap();
    let rejected = app
        .call(
            BootRequest::new(HttpMethod::Get, "/hello")
                .with_header("accept-encoding", "gzip;q=0, br"),
        )
        .await
        .unwrap();

    assert_eq!(no_header.header("content-encoding"), None);
    assert_eq!(no_header.body_text().unwrap(), "hello hello hello hello");
    assert_eq!(rejected.header("content-encoding"), None);
    assert_eq!(rejected.body_text().unwrap(), "hello hello hello hello");
}

#[tokio::test]
async fn compression_interceptor_respects_min_size_and_existing_encoding() {
    let app = BootApplication::builder()
        .use_global_compression(CompressionOptions::new().with_min_size(32))
        .route(
            RouteDefinition::get("/small", |_| async { Ok(BootResponse::text("small")) }).unwrap(),
        )
        .route(
            RouteDefinition::get("/encoded", |_| async {
                Ok(BootResponse::text("already encoded").with_header("content-encoding", "br"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let small = app
        .call(BootRequest::new(HttpMethod::Get, "/small").with_header("accept-encoding", "gzip"))
        .await
        .unwrap();
    let encoded = app
        .call(BootRequest::new(HttpMethod::Get, "/encoded").with_header("accept-encoding", "gzip"))
        .await
        .unwrap();

    assert_eq!(small.header("content-encoding"), None);
    assert_eq!(small.body_text().unwrap(), "small");
    assert_eq!(encoded.header("content-encoding"), Some("br"));
    assert_eq!(encoded.body_text().unwrap(), "already encoded");
}

#[tokio::test]
async fn compression_interceptor_updates_content_length_when_body_changes() {
    let app = BootApplication::builder()
        .use_global_compression(CompressionOptions::new().with_min_size(1))
        .route(
            RouteDefinition::get("/json", |_| async {
                let response = BootResponse::json(&json!({
                    "message": "hello hello hello hello"
                }))?;
                let content_length = response.body().len() as u64;
                Ok(response.with_content_length(content_length))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/json").with_header("accept-encoding", "gzip"))
        .await
        .unwrap();

    response.validate_content_length().unwrap();
    assert_eq!(response.header("content-encoding"), Some("gzip"));
    assert_eq!(
        response.content_length().unwrap(),
        Some(response.body().len() as u64)
    );
    assert_eq!(
        decode_gzip(response.body()).unwrap(),
        r#"{"message":"hello hello hello hello"}"#
    );
}

fn decode_gzip(bytes: &[u8]) -> std::io::Result<String> {
    let mut decoder = GzDecoder::new(bytes);
    let mut output = String::new();
    decoder.read_to_string(&mut output)?;
    Ok(output)
}
