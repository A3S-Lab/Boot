use a3s_boot::{BootError, BootRequest, BootResponse, HttpMethod, SseEvent};
use serde::Serialize;
use std::collections::BTreeMap;

#[test]
fn http_methods_display_and_parse_canonical_names() {
    assert_eq!(HttpMethod::Post.to_string(), "POST");
    assert_eq!("GET".parse::<HttpMethod>().unwrap(), HttpMethod::Get);
    assert_eq!("HEAD".parse::<HttpMethod>().unwrap(), HttpMethod::Head);

    let error = "TRACE".parse::<HttpMethod>().unwrap_err();

    assert!(matches!(
        error,
        BootError::MethodNotAllowed(message) if message == "TRACE"
    ));
}

#[test]
fn boot_errors_expose_http_status_codes_and_response_messages() {
    let not_found = BootError::NotFound("GET /missing".to_string());
    let method_not_allowed = BootError::MethodNotAllowed("POST /items".to_string());
    let unauthorized = BootError::Unauthorized("missing bearer token".to_string());
    let forbidden = BootError::Forbidden("GET /private".to_string());
    let bad_request = BootError::BadRequest("invalid input".to_string());
    let payload_too_large = BootError::PayloadTooLarge("request body exceeds 4 bytes".to_string());
    let unsupported_media_type =
        BootError::UnsupportedMediaType("expected JSON content type".to_string());
    let not_acceptable =
        BootError::NotAcceptable("expected client to accept JSON response".to_string());
    let internal = BootError::Internal("database failed".to_string());

    assert_eq!(not_found.http_status_code(), 404);
    assert_eq!(not_found.http_response_message(), "GET /missing");
    assert_eq!(method_not_allowed.http_status_code(), 405);
    assert_eq!(method_not_allowed.http_response_message(), "POST /items");
    assert_eq!(unauthorized.http_status_code(), 401);
    assert_eq!(unauthorized.http_response_message(), "missing bearer token");
    assert_eq!(forbidden.http_status_code(), 403);
    assert_eq!(forbidden.http_response_message(), "GET /private");
    assert_eq!(bad_request.http_status_code(), 400);
    assert_eq!(bad_request.http_response_message(), "invalid input");
    assert_eq!(payload_too_large.http_status_code(), 413);
    assert_eq!(
        payload_too_large.http_response_message(),
        "request body exceeds 4 bytes"
    );
    assert_eq!(unsupported_media_type.http_status_code(), 415);
    assert_eq!(
        unsupported_media_type.http_response_message(),
        "expected JSON content type"
    );
    assert_eq!(not_acceptable.http_status_code(), 406);
    assert_eq!(
        not_acceptable.http_response_message(),
        "expected client to accept JSON response"
    );
    assert_eq!(internal.http_status_code(), 500);
    assert_eq!(
        internal.http_response_message(),
        "internal error: database failed"
    );
}

#[test]
fn request_and_response_accessors_expose_common_adapter_fields() {
    let request = BootRequest::new(HttpMethod::Post, "/items?tag=rust").with_body("payload");
    let response = BootResponse::text_with_status(201, "created");

    assert_eq!(request.method(), HttpMethod::Post);
    assert_eq!(request.path(), "/items");
    assert_eq!(request.query_string(), Some("tag=rust"));
    assert_eq!(request.body(), b"payload");
    assert_eq!(request.into_body(), b"payload");

    assert_eq!(response.status(), 201);
    assert_eq!(response.body(), b"created");
    assert_eq!(response.into_body(), b"created");
}

#[test]
fn request_and_response_headers_are_case_insensitive() {
    let request =
        BootRequest::new(HttpMethod::Post, "/").with_header("Content-Type", "application/json");

    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(request.header("CONTENT-TYPE"), Some("application/json"));
    assert_eq!(request.content_type(), Some("application/json"));
    assert_eq!(
        request.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );

    let mut headers = BTreeMap::new();
    headers.insert("X-Trace-Id".to_string(), "abc".to_string());
    let request = BootRequest::new(HttpMethod::Get, "/").with_headers(headers);

    assert_eq!(request.header("x-trace-id"), Some("abc"));
    assert_eq!(request.header("X-TRACE-ID"), Some("abc"));

    let request = BootRequest::new(HttpMethod::Get, "/")
        .append_header("Accept", "application/json")
        .append_header("accept", "text/plain");

    assert_eq!(
        request.header_values("ACCEPT"),
        ["application/json", "text/plain"]
    );

    let response = BootResponse::text("ok").with_header("X-Boot", "ok");

    assert_eq!(response.header("x-boot"), Some("ok"));
    assert_eq!(response.header("X-BOOT"), Some("ok"));
    assert_eq!(
        response.header("CONTENT-TYPE"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(response.content_type(), Some("text/plain; charset=utf-8"));

    let mut response_headers = BTreeMap::new();
    response_headers.insert("X-Request-Id".to_string(), "req-1".to_string());
    let response = BootResponse::new(202, Vec::<u8>::new()).with_headers(response_headers);

    assert_eq!(response.header("x-request-id"), Some("req-1"));
    assert_eq!(
        response.headers.get("x-request-id").map(String::as_str),
        Some("req-1")
    );

    let response = BootResponse::new(200, Vec::<u8>::new())
        .append_header("Set-Cookie", "a=1")
        .append_header("set-cookie", "b=2");

    assert_eq!(response.header_values("SET-COOKIE"), ["a=1", "b=2"]);
}

#[test]
fn request_and_response_header_entries_include_primary_and_appended_headers() {
    let request = BootRequest::new(HttpMethod::Get, "/items")
        .with_header("X-Trace-Id", "trace-1")
        .append_header("Accept", "application/json")
        .append_header("Accept", "text/plain");
    let response = BootResponse::text("ok")
        .with_header("X-Boot", "ready")
        .append_header("Set-Cookie", "session=abc")
        .append_header("Set-Cookie", "theme=dark");

    assert_eq!(
        request.header_entries().collect::<Vec<_>>(),
        [
            ("x-trace-id", "trace-1"),
            ("accept", "application/json"),
            ("accept", "text/plain"),
        ]
    );
    assert_eq!(
        response.header_entries().collect::<Vec<_>>(),
        [
            ("content-type", "text/plain; charset=utf-8"),
            ("x-boot", "ready"),
            ("set-cookie", "session=abc"),
            ("set-cookie", "theme=dark"),
        ]
    );
}

#[test]
fn request_header_helpers_validate_header_names_and_values() {
    let valid = BootRequest::new(HttpMethod::Get, "/items")
        .with_header("X-Trace-Id", "abc-123")
        .append_header("Accept", "application/json")
        .append_header("x-mode", "fast\tsafe");
    let empty_name = BootRequest::new(HttpMethod::Get, "/items").with_header("", "value");
    let invalid_name =
        BootRequest::new(HttpMethod::Get, "/items").with_header("bad header", "value");
    let invalid_value =
        BootRequest::new(HttpMethod::Get, "/items").with_header("x-mode", "fast\nslow");

    valid.validate_headers().unwrap();
    assert!(matches!(
        empty_name.validate_headers().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid request header name \"\": header name cannot be empty"
    ));
    assert!(matches!(
        invalid_name.validate_headers().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid request header name \"bad header\": header name contains invalid characters"
    ));
    assert!(matches!(
        invalid_value.validate_headers().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid request header value for \"x-mode\": header value contains invalid characters"
    ));
}

#[test]
fn request_authorization_helpers_extract_bearer_tokens() {
    let request = BootRequest::new(HttpMethod::Get, "/private")
        .with_header("Authorization", "Bearer token-123");
    let lower_scheme =
        BootRequest::new(HttpMethod::Get, "/private").with_header("authorization", "bearer abc");
    let padded = BootRequest::new(HttpMethod::Get, "/private")
        .with_header("authorization", "  Bearer   abc  ");
    let appended = BootRequest::new(HttpMethod::Get, "/private")
        .append_header("Authorization", "Bearer appended");
    let basic =
        BootRequest::new(HttpMethod::Get, "/private").with_header("Authorization", "Basic abc");
    let empty =
        BootRequest::new(HttpMethod::Get, "/private").with_header("Authorization", "Bearer   ");
    let missing = BootRequest::new(HttpMethod::Get, "/private");

    assert_eq!(request.authorization(), Some("Bearer token-123"));
    assert_eq!(request.bearer_token(), Some("token-123"));
    assert_eq!(request.require_bearer_token().unwrap(), "token-123");
    assert_eq!(lower_scheme.bearer_token(), Some("abc"));
    assert_eq!(padded.bearer_token(), Some("abc"));
    assert_eq!(appended.authorization(), Some("Bearer appended"));
    assert_eq!(appended.bearer_token(), Some("appended"));
    assert_eq!(basic.bearer_token(), None);
    assert_eq!(empty.bearer_token(), None);
    assert_eq!(missing.authorization(), None);
    assert_eq!(missing.bearer_token(), None);
    assert!(matches!(
        missing.require_bearer_token().unwrap_err(),
        BootError::Unauthorized(message) if message == "missing bearer token"
    ));
}

#[test]
fn response_authentication_helpers_expose_www_authenticate_challenges() {
    let response =
        BootResponse::from_error(&BootError::Unauthorized("missing bearer token".to_string()));
    assert_eq!(response.status, 401);
    assert_eq!(response.www_authenticate(), None);
    assert!(response.www_authenticate_values().is_empty());

    let response = response
        .with_www_authenticate(r#"Bearer realm="api""#)
        .append_www_authenticate(r#"Basic realm="legacy""#);

    assert_eq!(response.www_authenticate(), Some(r#"Bearer realm="api""#));
    assert_eq!(
        response.www_authenticate_values(),
        [r#"Bearer realm="api""#, r#"Basic realm="legacy""#]
    );
}

#[test]
fn request_cookie_helpers_parse_cookie_headers() {
    let request = BootRequest::new(HttpMethod::Get, "/private")
        .with_header(
            "Cookie",
            r#"session=abc; theme=dark; quoted="hello world"; empty="#,
        )
        .append_header("Cookie", "session=override; flag=true");

    assert_eq!(
        request.cookie_pairs().unwrap(),
        [
            ("session".to_string(), "abc".to_string()),
            ("theme".to_string(), "dark".to_string()),
            ("quoted".to_string(), "hello world".to_string()),
            ("empty".to_string(), "".to_string()),
            ("session".to_string(), "override".to_string()),
            ("flag".to_string(), "true".to_string())
        ]
    );
    assert_eq!(request.cookie("session").unwrap().as_deref(), Some("abc"));
    assert_eq!(request.require_cookie("session").unwrap(), "abc");
    assert_eq!(
        request.cookie_values("session").unwrap(),
        ["abc".to_string(), "override".to_string()]
    );
    assert_eq!(request.cookie("missing").unwrap(), None);
    assert!(matches!(
        request.require_cookie("missing").unwrap_err(),
        BootError::Unauthorized(message) if message == "missing cookie: missing"
    ));

    let cookies = request.cookies().unwrap();
    assert_eq!(cookies.get("session").map(String::as_str), Some("abc"));
    assert_eq!(cookies.get("theme").map(String::as_str), Some("dark"));
    assert_eq!(cookies.get("flag").map(String::as_str), Some("true"));
}

#[test]
fn request_cookie_helpers_reject_malformed_cookie_pairs() {
    let missing_equals =
        BootRequest::new(HttpMethod::Get, "/private").with_header("Cookie", "session");
    let empty_name = BootRequest::new(HttpMethod::Get, "/private").with_header("Cookie", "=abc");

    assert!(matches!(
        missing_equals.cookie_pairs().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid cookie pair: session"
    ));
    assert!(matches!(
        empty_name.cookie_pairs().unwrap_err(),
        BootError::BadRequest(message) if message == "cookie name cannot be empty"
    ));
    assert!(matches!(
        missing_equals.require_cookie("session").unwrap_err(),
        BootError::BadRequest(message) if message == "invalid cookie pair: session"
    ));
}

#[test]
fn content_type_helpers_read_first_content_type_header() {
    let request = BootRequest::new(HttpMethod::Post, "/items")
        .with_content_type("application/json")
        .append_header("content-type", "application/merge-patch+json");
    let appended_request =
        BootRequest::new(HttpMethod::Post, "/items").append_header("Content-Type", "text/plain");

    assert_eq!(request.content_type(), Some("application/json"));
    assert_eq!(appended_request.content_type(), Some("text/plain"));

    let response = BootResponse::new(202, Vec::<u8>::new())
        .with_content_type("application/json")
        .append_header("content-type", "application/problem+json");
    let appended_response =
        BootResponse::new(202, Vec::<u8>::new()).append_header("Content-Type", "text/plain");

    assert_eq!(response.content_type(), Some("application/json"));
    assert!(response.is_content_type("application/json"));
    assert!(response.is_json_content_type());
    assert_eq!(appended_response.content_type(), Some("text/plain"));
    assert!(appended_response.is_content_type("text/plain"));
    assert!(!appended_response.is_json_content_type());
}

#[test]
fn content_length_helpers_read_and_set_content_length_headers() {
    let request = BootRequest::new(HttpMethod::Post, "/items")
        .with_content_length(42)
        .append_header("Content-Length", "7");
    let appended_request =
        BootRequest::new(HttpMethod::Post, "/items").append_header("Content-Length", "5");
    let missing_request = BootRequest::new(HttpMethod::Post, "/items");

    assert_eq!(request.header("content-length"), Some("42"));
    assert_eq!(request.content_length().unwrap(), Some(42));
    assert_eq!(appended_request.content_length().unwrap(), Some(5));
    assert_eq!(missing_request.content_length().unwrap(), None);

    let response = BootResponse::empty(202)
        .with_content_length(42)
        .append_header("Content-Length", "7");
    let appended_response = BootResponse::empty(202).append_header("Content-Length", "5");
    let missing_response = BootResponse::empty(202);

    assert_eq!(response.header("content-length"), Some("42"));
    assert_eq!(response.content_length().unwrap(), Some(42));
    assert_eq!(appended_response.content_length().unwrap(), Some(5));
    assert_eq!(missing_response.content_length().unwrap(), None);
}

#[test]
fn content_length_helpers_reject_invalid_content_length_headers() {
    let invalid_request =
        BootRequest::new(HttpMethod::Post, "/items").with_header("Content-Length", "12x");
    let negative_request =
        BootRequest::new(HttpMethod::Post, "/items").with_header("Content-Length", "-1");
    let invalid_response = BootResponse::empty(202).with_header("Content-Length", "12x");

    assert!(matches!(
        invalid_request.content_length().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid content-length header: 12x"
    ));
    assert!(matches!(
        negative_request.content_length().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid content-length header: -1"
    ));
    assert!(matches!(
        invalid_response.content_length().unwrap_err(),
        BootError::Internal(message) if message == "invalid content-length header: 12x"
    ));
}

#[test]
fn request_strict_content_length_validates_repeated_values_and_body_length() {
    let request = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .with_content_length(4)
        .append_header("Content-Length", "4");
    let appended_request = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .append_header("Content-Length", "4")
        .append_header("content-length", "4");
    let missing_request = BootRequest::new(HttpMethod::Post, "/items").with_body("data");

    assert_eq!(request.strict_content_length().unwrap(), Some(4));
    request.validate_content_length().unwrap();
    assert_eq!(appended_request.strict_content_length().unwrap(), Some(4));
    appended_request.validate_content_length().unwrap();
    assert_eq!(missing_request.strict_content_length().unwrap(), None);
    missing_request.validate_content_length().unwrap();
}

#[test]
fn request_strict_content_length_rejects_invalid_conflicting_and_mismatched_values() {
    let invalid_request = BootRequest::new(HttpMethod::Post, "/items")
        .with_content_length(4)
        .append_header("Content-Length", "nope");
    let conflicting_request = BootRequest::new(HttpMethod::Post, "/items")
        .with_content_length(4)
        .append_header("Content-Length", "5");
    let mismatched_request = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .with_content_length(5);

    assert!(matches!(
        invalid_request.strict_content_length().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid content-length header: nope"
    ));
    assert!(matches!(
        conflicting_request.strict_content_length().unwrap_err(),
        BootError::BadRequest(message) if message == "conflicting content-length headers: 4 != 5"
    ));
    assert!(matches!(
        mismatched_request.validate_content_length().unwrap_err(),
        BootError::BadRequest(message) if message == "content-length header does not match request body length: expected 5, got 4"
    ));
}

#[test]
fn request_body_limit_validation_checks_declared_and_actual_lengths() {
    let valid = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .with_content_length(4);
    let oversized_declared = BootRequest::new(HttpMethod::Post, "/items").with_content_length(5);
    let oversized_actual = BootRequest::new(HttpMethod::Post, "/items").with_body("large");
    let invalid_declared =
        BootRequest::new(HttpMethod::Post, "/items").with_header("Content-Length", "nope");

    valid.validate_body_limit(4).unwrap();
    assert!(matches!(
        oversized_declared.validate_body_limit(4).unwrap_err(),
        BootError::PayloadTooLarge(message) if message == "request body exceeds 4 bytes"
    ));
    assert!(matches!(
        oversized_actual.validate_body_limit(4).unwrap_err(),
        BootError::PayloadTooLarge(message) if message == "request body exceeds 4 bytes"
    ));
    assert!(matches!(
        invalid_declared.validate_body_limit(4).unwrap_err(),
        BootError::BadRequest(message) if message == "invalid content-length header: nope"
    ));
}

#[test]
fn request_validate_with_body_limit_runs_core_request_checks_in_adapter_order() {
    let valid = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .with_content_length(4);
    let invalid_header = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("bad header", "value")
        .with_body("large");
    let invalid_declared = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("Content-Length", "nope")
        .with_body("large");
    let oversized_declared = BootRequest::new(HttpMethod::Post, "/items").with_content_length(5);
    let oversized_actual = BootRequest::new(HttpMethod::Post, "/items").with_body("large");
    let mismatched = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .with_content_length(5);

    valid.validate_with_body_limit(4).unwrap();
    assert!(matches!(
        invalid_header.validate_with_body_limit(4).unwrap_err(),
        BootError::BadRequest(message) if message == "invalid request header name \"bad header\": header name contains invalid characters"
    ));
    assert!(matches!(
        invalid_declared.validate_with_body_limit(4).unwrap_err(),
        BootError::BadRequest(message) if message == "invalid content-length header: nope"
    ));
    assert!(matches!(
        oversized_declared.validate_with_body_limit(4).unwrap_err(),
        BootError::PayloadTooLarge(message) if message == "request body exceeds 4 bytes"
    ));
    assert!(matches!(
        oversized_actual.validate_with_body_limit(4).unwrap_err(),
        BootError::PayloadTooLarge(message) if message == "request body exceeds 4 bytes"
    ));
    assert!(matches!(
        mismatched.validate_with_body_limit(10).unwrap_err(),
        BootError::BadRequest(message) if message == "content-length header does not match request body length: expected 5, got 4"
    ));
}

#[test]
fn request_validate_runs_core_request_checks_in_adapter_order() {
    let valid = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .with_content_length(4);
    let invalid_header = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("bad header", "value")
        .with_header("Content-Length", "nope");
    let invalid_content_length = BootRequest::new(HttpMethod::Post, "/items")
        .with_body("data")
        .with_content_length(5);

    valid.validate().unwrap();
    assert!(matches!(
        invalid_header.validate().unwrap_err(),
        BootError::BadRequest(message) if message == "invalid request header name \"bad header\": header name contains invalid characters"
    ));
    assert!(matches!(
        invalid_content_length.validate().unwrap_err(),
        BootError::BadRequest(message) if message == "content-length header does not match request body length: expected 5, got 4"
    ));
}

#[test]
fn response_strict_content_length_validates_repeated_values_and_body_length() {
    let response = BootResponse::text("ok")
        .with_content_length(2)
        .append_header("Content-Length", "2");
    let appended_response = BootResponse::text("ok")
        .append_header("Content-Length", "2")
        .append_header("content-length", "2");
    let missing_response = BootResponse::text("ok");

    assert_eq!(response.strict_content_length().unwrap(), Some(2));
    response.validate_content_length().unwrap();
    assert_eq!(appended_response.strict_content_length().unwrap(), Some(2));
    appended_response.validate_content_length().unwrap();
    assert_eq!(missing_response.strict_content_length().unwrap(), None);
    missing_response.validate_content_length().unwrap();
}

#[test]
fn response_strict_content_length_rejects_invalid_conflicting_and_mismatched_values() {
    let invalid_response = BootResponse::text("ok")
        .with_content_length(2)
        .append_header("Content-Length", "nope");
    let conflicting_response = BootResponse::text("ok")
        .with_content_length(2)
        .append_header("Content-Length", "3");
    let mismatched_response = BootResponse::text("ok").with_content_length(3);

    assert!(matches!(
        invalid_response.strict_content_length().unwrap_err(),
        BootError::Internal(message) if message == "invalid response content-length header: nope"
    ));
    assert!(matches!(
        conflicting_response.strict_content_length().unwrap_err(),
        BootError::Internal(message) if message == "conflicting response content-length headers: 2 != 3"
    ));
    assert!(matches!(
        mismatched_response.validate_content_length().unwrap_err(),
        BootError::Internal(message) if message == "response content-length header does not match response body length: expected 3, got 2"
    ));
}

#[test]
fn response_header_helpers_validate_header_names_and_values() {
    let valid = BootResponse::text("ok")
        .with_header("X-Trace-Id", "abc-123")
        .append_header("Set-Cookie", "session=abc; Path=/")
        .append_header("x-mode", "fast\tsafe");
    let empty_name = BootResponse::text("ok").with_header("", "value");
    let invalid_name = BootResponse::text("ok").with_header("bad header", "value");
    let invalid_value = BootResponse::text("ok").with_header("x-mode", "fast\nslow");

    valid.validate_headers().unwrap();
    assert!(matches!(
        empty_name.validate_headers().unwrap_err(),
        BootError::Internal(message) if message == "invalid response header name \"\": header name cannot be empty"
    ));
    assert!(matches!(
        invalid_name.validate_headers().unwrap_err(),
        BootError::Internal(message) if message == "invalid response header name \"bad header\": header name contains invalid characters"
    ));
    assert!(matches!(
        invalid_value.validate_headers().unwrap_err(),
        BootError::Internal(message) if message == "invalid response header value for \"x-mode\": header value contains invalid characters"
    ));
}

#[test]
fn request_content_type_helpers_match_media_types() {
    let json = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("Content-Type", "Application/JSON; Charset=UTF-8");
    let problem_json = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("Content-Type", "application/problem+json; charset=utf-8");
    let text =
        BootRequest::new(HttpMethod::Post, "/items").with_header("Content-Type", "text/plain");
    let missing = BootRequest::new(HttpMethod::Post, "/items");

    assert!(json.is_content_type("application/json"));
    assert!(json.is_content_type("application/json; charset=ignored"));
    assert!(json.is_json_content_type());

    assert!(!problem_json.is_content_type("application/json"));
    assert!(problem_json.is_content_type("application/problem+json"));
    assert!(problem_json.is_json_content_type());

    assert!(text.is_content_type("TEXT/PLAIN"));
    assert!(!text.is_json_content_type());

    assert!(!missing.is_content_type("application/json"));
    assert!(!missing.is_json_content_type());
}

#[test]
fn response_content_type_helpers_match_media_types() {
    let json = BootResponse::new(200, Vec::<u8>::new())
        .with_header("Content-Type", "Application/JSON; Charset=UTF-8");
    let problem_json = BootResponse::new(200, Vec::<u8>::new())
        .with_header("Content-Type", "application/problem+json; charset=utf-8");
    let text = BootResponse::new(200, Vec::<u8>::new()).with_header("Content-Type", "text/plain");
    let missing = BootResponse::new(200, Vec::<u8>::new());

    assert!(json.is_content_type("application/json"));
    assert!(json.is_content_type("application/json; charset=ignored"));
    assert!(json.is_json_content_type());

    assert!(!problem_json.is_content_type("application/json"));
    assert!(problem_json.is_content_type("application/problem+json"));
    assert!(problem_json.is_json_content_type());

    assert!(text.is_content_type("TEXT/PLAIN"));
    assert!(!text.is_json_content_type());

    assert!(!missing.is_content_type("application/json"));
    assert!(!missing.is_json_content_type());
}

#[test]
fn request_accept_helpers_match_json_response_ranges() {
    let missing = BootRequest::new(HttpMethod::Get, "/items");
    let json = BootRequest::new(HttpMethod::Get, "/items")
        .with_header("Accept", "text/plain, application/json;q=0.5");
    let wildcard = BootRequest::new(HttpMethod::Get, "/items").with_header("Accept", "*/*");
    let application_wildcard =
        BootRequest::new(HttpMethod::Get, "/items").with_header("Accept", "application/*");
    let suffix_wildcard =
        BootRequest::new(HttpMethod::Get, "/items").with_header("Accept", "application/*+json");
    let problem_json = BootRequest::new(HttpMethod::Get, "/items")
        .with_header("Accept", "application/problem+json");
    let vendor_json = BootRequest::new(HttpMethod::Get, "/items")
        .with_header("Accept", "Application/Vnd.Api+JSON; q=0.4");
    let text = BootRequest::new(HttpMethod::Get, "/items").with_header("Accept", "text/plain");
    let rejected_exact = BootRequest::new(HttpMethod::Get, "/items")
        .with_header("Accept", "application/json;q=0, */*;q=1");
    let rejected_vendor = BootRequest::new(HttpMethod::Get, "/items")
        .with_header("Accept", "application/problem+json;q=0, application/*;q=1");
    let invalid_q =
        BootRequest::new(HttpMethod::Get, "/items").with_header("Accept", "application/json;q=NaN");
    let appended = BootRequest::new(HttpMethod::Get, "/items")
        .append_header("Accept", "text/plain")
        .append_header("Accept", "application/json");

    assert!(missing.accepts_json());
    assert!(json.accepts_json());
    assert!(wildcard.accepts_json());
    assert!(application_wildcard.accepts_json());
    assert!(suffix_wildcard.accepts_json());
    assert!(problem_json.accepts_json());
    assert!(vendor_json.accepts_json());
    assert!(invalid_q.accepts_json());
    assert!(appended.accepts_json());
    assert!(!text.accepts_json());
    assert!(!rejected_exact.accepts_json());
    assert!(!rejected_vendor.accepts_json());
    assert!(matches!(
        text.require_accepts_json().unwrap_err(),
        BootError::NotAcceptable(message) if message == "expected client to accept JSON response"
    ));
}

#[test]
fn request_accept_helpers_match_event_stream_response_ranges() {
    let missing = BootRequest::new(HttpMethod::Get, "/events");
    let event_stream = BootRequest::new(HttpMethod::Get, "/events")
        .with_header("Accept", "text/plain, text/event-stream;q=0.5");
    let wildcard = BootRequest::new(HttpMethod::Get, "/events").with_header("Accept", "*/*");
    let text_wildcard =
        BootRequest::new(HttpMethod::Get, "/events").with_header("Accept", "text/*");
    let json =
        BootRequest::new(HttpMethod::Get, "/events").with_header("Accept", "application/json");
    let rejected_exact = BootRequest::new(HttpMethod::Get, "/events")
        .with_header("Accept", "text/event-stream;q=0, */*;q=1");
    let appended = BootRequest::new(HttpMethod::Get, "/events")
        .append_header("Accept", "application/json")
        .append_header("Accept", "text/event-stream");

    assert!(missing.accepts_event_stream());
    assert!(event_stream.accepts_event_stream());
    assert!(wildcard.accepts_event_stream());
    assert!(text_wildcard.accepts_event_stream());
    assert!(appended.accepts_event_stream());
    assert!(!json.accepts_event_stream());
    assert!(!rejected_exact.accepts_event_stream());
    assert!(matches!(
        json.require_accepts_event_stream().unwrap_err(),
        BootError::NotAcceptable(message) if message == "expected client to accept text/event-stream response"
    ));
}

#[test]
fn sse_events_encode_fields_and_multiline_data() {
    let event = SseEvent::new("line 1\nline 2")
        .with_comment("warmup")
        .with_id("42")
        .with_event("cat.created")
        .with_retry(5000);

    assert_eq!(
        String::from_utf8(event.encode()).unwrap(),
        ": warmup\nid: 42\nevent: cat.created\nretry: 5000\ndata: line 1\ndata: line 2\n\n"
    );
}

#[test]
fn sse_response_sets_event_stream_headers_and_keeps_body_streaming() {
    let response = BootResponse::sse(futures_util::stream::iter([Ok(SseEvent::new("ready"))]));

    assert_eq!(response.status(), 200);
    assert!(response.is_streaming());
    assert!(response.is_event_stream());
    assert!(response.has_body());
    assert!(response.body.is_empty());
    assert_eq!(
        response.header("content-type"),
        Some("text/event-stream; charset=utf-8")
    );
    assert_eq!(response.header("cache-control"), Some("no-cache"));
    assert_eq!(response.header("connection"), Some("keep-alive"));
    response.validate().unwrap();
    assert!(response.into_sse_stream().is_some());
}

#[test]
fn response_empty_helpers_create_empty_status_responses() {
    let accepted = BootResponse::empty(202);
    let no_content = BootResponse::no_content();

    assert_eq!(accepted.status, 202);
    assert!(accepted.body.is_empty());
    assert_eq!(no_content.status, 204);
    assert!(no_content.body.is_empty());
}

#[test]
fn response_location_helpers_read_and_set_location_headers() {
    let response = BootResponse::empty(201)
        .with_location("/items/42")
        .append_header("Location", "/items/fallback");
    let appended_response = BootResponse::empty(201).append_header("Location", "/items/99");
    let missing_response = BootResponse::empty(201);

    assert_eq!(response.header("location"), Some("/items/42"));
    assert_eq!(response.location(), Some("/items/42"));
    assert_eq!(
        response.header_values("LOCATION"),
        ["/items/42", "/items/fallback"]
    );
    assert_eq!(appended_response.location(), Some("/items/99"));
    assert_eq!(missing_response.location(), None);
}

#[test]
fn response_redirect_helpers_set_status_and_location_headers() {
    let redirect = BootResponse::redirect("/login");
    let see_other = BootResponse::see_other("/items/42");
    let temporary = BootResponse::temporary_redirect("/maintenance");
    let permanent = BootResponse::permanent_redirect("/docs");
    let custom = BootResponse::redirect_with_status(301, "/moved");

    assert_eq!(redirect.status, 302);
    assert_eq!(redirect.location(), Some("/login"));
    assert!(redirect.body.is_empty());

    assert_eq!(see_other.status, 303);
    assert_eq!(see_other.location(), Some("/items/42"));

    assert_eq!(temporary.status, 307);
    assert_eq!(temporary.location(), Some("/maintenance"));

    assert_eq!(permanent.status, 308);
    assert_eq!(permanent.location(), Some("/docs"));

    assert_eq!(custom.status, 301);
    assert_eq!(custom.location(), Some("/moved"));
}

#[test]
fn response_status_predicates_classify_standard_ranges() {
    let informational = BootResponse::empty(101);
    let success = BootResponse::empty(204);
    let redirection = BootResponse::empty(304);
    let client_error = BootResponse::empty(404);
    let server_error = BootResponse::empty(503);
    let invalid_low = BootResponse::empty(99);
    let invalid_high = BootResponse::empty(700);

    assert!(informational.is_informational());
    assert!(!informational.is_error());

    assert!(success.is_success());
    assert!(!success.is_error());

    assert!(redirection.is_redirection());
    assert!(!redirection.is_error());

    assert!(client_error.is_client_error());
    assert!(client_error.is_error());

    assert!(server_error.is_server_error());
    assert!(server_error.is_error());

    for response in [invalid_low, invalid_high] {
        assert!(!response.is_informational());
        assert!(!response.is_success());
        assert!(!response.is_redirection());
        assert!(!response.is_client_error());
        assert!(!response.is_server_error());
        assert!(!response.is_error());
    }
}

#[test]
fn response_status_helpers_validate_http_status_code_ranges() {
    let minimum = BootResponse::empty(100);
    let maximum = BootResponse::empty(999);
    let too_low = BootResponse::empty(99);
    let too_high = BootResponse::empty(1000);

    assert!(minimum.is_valid_status());
    assert!(maximum.is_valid_status());
    assert!(!too_low.is_valid_status());
    assert!(!too_high.is_valid_status());

    minimum.validate_status().unwrap();
    maximum.validate_status().unwrap();
    assert!(matches!(
        too_low.validate_status().unwrap_err(),
        BootError::Internal(message) if message == "invalid response status 99"
    ));
    assert!(matches!(
        too_high.validate_status().unwrap_err(),
        BootError::Internal(message) if message == "invalid response status 1000"
    ));
}

#[test]
fn response_body_helpers_report_body_presence_and_status_body_rules() {
    let ok = BootResponse::text("ok");
    let empty_ok = BootResponse::empty(200);
    let informational = BootResponse::empty(101);
    let no_content = BootResponse::no_content();
    let not_modified = BootResponse::empty(304);
    let client_error = BootResponse::text_with_status(404, "missing");
    let server_error = BootResponse::text_with_status(500, "failed");
    let invalid_low = BootResponse::empty(99);
    let invalid_high = BootResponse::empty(700);
    let invalid_no_content = BootResponse::text_with_status(204, "not empty");

    assert!(ok.has_body());
    assert!(!empty_ok.has_body());

    assert!(!informational.allows_body());
    assert!(!no_content.allows_body());
    assert!(!not_modified.allows_body());

    assert!(ok.allows_body());
    assert!(empty_ok.allows_body());
    assert!(client_error.allows_body());
    assert!(server_error.allows_body());
    assert!(invalid_low.allows_body());
    assert!(invalid_high.allows_body());

    ok.validate_body_allowed().unwrap();
    no_content.validate_body_allowed().unwrap();
    assert!(matches!(
        invalid_no_content.validate_body_allowed().unwrap_err(),
        BootError::Internal(message) if message == "response status 204 must not include a body"
    ));
}

#[test]
fn response_validate_runs_core_response_checks_in_adapter_order() {
    let valid = BootResponse::text("ok").with_content_length(2);
    let invalid_status = BootResponse::text("ok")
        .with_status(99)
        .with_header("Content-Length", "nope");
    let invalid_content_length = BootResponse::text_with_status(204, "ok")
        .with_content_length(3)
        .with_header("bad header", "value");
    let invalid_body_status = BootResponse::text_with_status(204, "ok").with_content_length(2);
    let invalid_header = BootResponse::text("ok").with_header("bad header", "value");

    valid.validate().unwrap();
    assert!(matches!(
        invalid_status.validate().unwrap_err(),
        BootError::Internal(message) if message == "invalid response status 99"
    ));
    assert!(matches!(
        invalid_content_length.validate().unwrap_err(),
        BootError::Internal(message) if message == "response content-length header does not match response body length: expected 3, got 2"
    ));
    assert!(matches!(
        invalid_body_status.validate().unwrap_err(),
        BootError::Internal(message) if message == "response status 204 must not include a body"
    ));
    assert!(matches!(
        invalid_header.validate().unwrap_err(),
        BootError::Internal(message) if message == "invalid response header name \"bad header\": header name contains invalid characters"
    ));
}

#[test]
fn response_status_helpers_preserve_content_types() {
    #[derive(Debug, Serialize)]
    struct CreatedItem<'a> {
        id: u64,
        name: &'a str,
    }

    let text = BootResponse::text_with_status(201, "created");
    let json = BootResponse::json_with_status(
        202,
        &CreatedItem {
            id: 42,
            name: "boot",
        },
    )
    .unwrap();

    assert_eq!(text.status, 201);
    assert_eq!(
        text.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(text.body, b"created");
    assert_eq!(json.status, 202);
    assert_eq!(json.header("content-type"), Some("application/json"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&json.body).unwrap(),
        serde_json::json!({ "id": 42, "name": "boot" })
    );
}

#[test]
fn response_body_readers_decode_text_and_json() {
    #[derive(Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
    struct CreatedItem {
        id: u64,
        name: String,
    }

    let text = BootResponse::text("created");
    let json = BootResponse::json(&CreatedItem {
        id: 42,
        name: "boot".to_string(),
    })
    .unwrap();

    assert_eq!(text.body_text().unwrap(), "created");
    assert_eq!(
        json.body_json::<CreatedItem>().unwrap(),
        CreatedItem {
            id: 42,
            name: "boot".to_string()
        }
    );
}

#[test]
fn response_body_readers_report_invalid_response_bodies_as_internal_errors() {
    let text_error = BootResponse::new(200, vec![0xff]).body_text().unwrap_err();
    let json_error = BootResponse::text("not json")
        .body_json::<serde_json::Value>()
        .unwrap_err();

    assert!(matches!(text_error, BootError::Internal(_)));
    assert!(matches!(json_error, BootError::Internal(_)));
}

#[test]
fn response_from_error_uses_http_error_mapping() {
    let bad_request = BootResponse::from_error(&BootError::BadRequest("invalid input".to_string()));
    let unauthorized =
        BootResponse::from_error(&BootError::Unauthorized("missing bearer token".to_string()));
    let unsupported_media_type = BootResponse::from_error(&BootError::UnsupportedMediaType(
        "expected json".to_string(),
    ));
    let not_acceptable = BootResponse::from_error(&BootError::NotAcceptable(
        "expected accept json".to_string(),
    ));
    let internal = BootResponse::from_error(&BootError::Internal("database failed".to_string()));

    assert_eq!(bad_request.status, 400);
    assert_eq!(
        bad_request.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(bad_request.body, b"invalid input");
    assert_eq!(unauthorized.status, 401);
    assert_eq!(unauthorized.body, b"missing bearer token");
    assert_eq!(unsupported_media_type.status, 415);
    assert_eq!(unsupported_media_type.body, b"expected json");
    assert_eq!(not_acceptable.status, 406);
    assert_eq!(not_acceptable.body, b"expected accept json");
    assert_eq!(internal.status, 500);
    assert_eq!(
        internal.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(internal.body, b"internal error: database failed");
}

#[test]
fn request_text_rejects_invalid_utf8_as_bad_request() {
    let error = BootRequest::new(HttpMethod::Post, "/")
        .with_body(vec![0xff])
        .text()
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}

#[test]
fn request_body_helpers_preserve_content_types() {
    #[derive(Debug, Serialize)]
    struct CreateItem<'a> {
        name: &'a str,
    }

    let text = BootRequest::new(HttpMethod::Post, "/items").with_text("created");
    let json = BootRequest::new(HttpMethod::Post, "/items")
        .with_json(&CreateItem { name: "boot" })
        .unwrap();

    assert_eq!(
        text.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(text.text().unwrap(), "created");
    assert_eq!(json.header("content-type"), Some("application/json"));
    assert_eq!(
        json.json::<serde_json::Value>().unwrap(),
        serde_json::json!({ "name": "boot" })
    );
}

#[test]
fn request_json_with_content_type_requires_json_media_type() {
    #[derive(Debug, Serialize, serde::Deserialize, PartialEq, Eq)]
    struct CreateItem {
        name: String,
    }

    let json = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("Content-Type", "application/vnd.api+json; charset=utf-8")
        .with_body(r#"{"name":"boot"}"#);
    let missing = BootRequest::new(HttpMethod::Post, "/items").with_body(r#"{"name":"boot"}"#);
    let text = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("Content-Type", "text/plain")
        .with_body(r#"{"name":"boot"}"#);
    let invalid_json = BootRequest::new(HttpMethod::Post, "/items")
        .with_header("Content-Type", "application/json")
        .with_body("{");

    assert_eq!(
        json.json_with_content_type::<CreateItem>().unwrap(),
        CreateItem {
            name: "boot".to_string()
        }
    );

    assert!(matches!(
        missing.json_with_content_type::<CreateItem>().unwrap_err(),
        BootError::UnsupportedMediaType(message) if message == "expected JSON content type"
    ));
    assert!(matches!(
        text.json_with_content_type::<CreateItem>().unwrap_err(),
        BootError::UnsupportedMediaType(message) if message == "expected JSON content type, got text/plain"
    ));
    assert!(matches!(
        invalid_json
            .json_with_content_type::<CreateItem>()
            .unwrap_err(),
        BootError::BadRequest(_)
    ));
}

#[test]
fn request_query_values_preserve_repeated_query_params() {
    let request = BootRequest::new(
        HttpMethod::Get,
        "/search?tag=rust&tag=web%20api&tag=a%2Bb&q=boot+framework",
    );

    assert_eq!(request.query_value("tag").unwrap().as_deref(), Some("rust"));
    assert_eq!(
        request.query_value("q").unwrap().as_deref(),
        Some("boot framework")
    );
    assert!(request.query_value("missing").unwrap().is_none());
    assert_eq!(
        request.query_values("tag").unwrap(),
        ["rust", "web api", "a+b"]
    );
    assert_eq!(request.query_values("q").unwrap(), ["boot framework"]);
    assert!(request.query_values("missing").unwrap().is_empty());
    assert_eq!(
        request.query_pairs().unwrap(),
        [
            ("tag".to_string(), "rust".to_string()),
            ("tag".to_string(), "web api".to_string()),
            ("tag".to_string(), "a+b".to_string()),
            ("q".to_string(), "boot framework".to_string())
        ]
    );

    let request = BootRequest::new(HttpMethod::Get, "/search")
        .with_query_param("tag", "rust")
        .with_query_param("q", "boot framework");

    assert_eq!(
        request.query_pairs().unwrap(),
        [
            ("q".to_string(), "boot framework".to_string()),
            ("tag".to_string(), "rust".to_string())
        ]
    );
}

#[test]
fn request_query_values_reject_invalid_utf8() {
    let request = BootRequest::new(HttpMethod::Get, "/search?tag=%FF");

    let error = request.query_values("tag").unwrap_err();
    let pairs_error = request.query_pairs().unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
    assert!(matches!(pairs_error, BootError::BadRequest(_)));
}

#[test]
fn request_query_values_reject_invalid_percent_triplets() {
    let request = BootRequest::new(HttpMethod::Get, "/search?tag=%ZZ");

    let value_error = request.query_value("tag").unwrap_err();
    let values_error = request.query_values("tag").unwrap_err();
    let pairs_error = request.query_pairs().unwrap_err();

    assert!(matches!(value_error, BootError::BadRequest(_)));
    assert!(matches!(values_error, BootError::BadRequest(_)));
    assert!(matches!(pairs_error, BootError::BadRequest(_)));
}

#[test]
fn typed_query_rejects_invalid_percent_triplets() {
    #[allow(dead_code)]
    #[derive(Debug, serde::Deserialize)]
    struct SearchQuery {
        tag: String,
    }

    let request = BootRequest::new(HttpMethod::Get, "/search?tag=%ZZ");

    let error = request.query::<SearchQuery>().unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}
