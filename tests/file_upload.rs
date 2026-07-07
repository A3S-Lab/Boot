#![cfg(feature = "file-upload")]

use a3s_boot::{BootError, BootRequest, HttpMethod, MultipartOptions};

#[tokio::test]
async fn multipart_form_parses_fields_and_files() {
    let request = multipart_request(
        "upload-boundary",
        concat!(
            "--upload-boundary\r\n",
            "Content-Disposition: form-data; name=\"title\"\r\n",
            "\r\n",
            "Milo\r\n",
            "--upload-boundary\r\n",
            "Content-Disposition: form-data; name=\"tag\"\r\n",
            "\r\n",
            "orange\r\n",
            "--upload-boundary\r\n",
            "Content-Disposition: form-data; name=\"tag\"\r\n",
            "\r\n",
            "sleepy\r\n",
            "--upload-boundary\r\n",
            "Content-Disposition: form-data; name=\"avatar\"; filename=\"milo.txt\"\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "hello from file\r\n",
            "--upload-boundary--\r\n",
        ),
    );

    let form = request.multipart_form().await.unwrap();
    let title = form.field("title").unwrap();
    let avatar = form.file("avatar").unwrap();

    assert_eq!(form.fields().len(), 3);
    assert_eq!(form.files().len(), 1);
    assert_eq!(title.name(), "title");
    assert_eq!(title.value(), "Milo");
    assert_eq!(form.field_values("tag"), vec!["orange", "sleepy"]);
    assert_eq!(avatar.name(), "avatar");
    assert_eq!(avatar.file_name(), "milo.txt");
    assert_eq!(avatar.content_type(), Some("text/plain"));
    assert_eq!(avatar.bytes(), b"hello from file");
    assert_eq!(avatar.text().unwrap(), "hello from file");
    assert_eq!(avatar.size(), "hello from file".len());
}

#[tokio::test]
async fn multipart_form_rejects_non_multipart_requests() {
    let request = BootRequest::new(HttpMethod::Post, "/upload")
        .with_content_type("application/json")
        .with_body("{}");

    let error = request.multipart_form().await.unwrap_err();

    assert!(matches!(
        error,
        BootError::UnsupportedMediaType(message)
            if message == "expected multipart/form-data content type, got application/json"
    ));
}

#[tokio::test]
async fn multipart_form_rejects_missing_boundaries() {
    let request = BootRequest::new(HttpMethod::Post, "/upload")
        .with_content_type("multipart/form-data")
        .with_body("");

    let error = request.multipart_form().await.unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message.contains("invalid multipart boundary"))
    );
}

#[tokio::test]
async fn multipart_form_enforces_body_and_field_limits() {
    let request = multipart_request(
        "limit-boundary",
        concat!(
            "--limit-boundary\r\n",
            "Content-Disposition: form-data; name=\"title\"\r\n",
            "\r\n",
            "larger than limit\r\n",
            "--limit-boundary--\r\n",
        ),
    );

    let body_error = request
        .multipart_form_with_options(MultipartOptions::new().with_max_body_size(8))
        .await
        .unwrap_err();
    let field_error = request
        .multipart_form_with_options(MultipartOptions::new().with_max_field_size(4))
        .await
        .unwrap_err();

    assert!(matches!(
        body_error,
        BootError::PayloadTooLarge(message) if message == "multipart body exceeds 8 bytes"
    ));
    assert!(matches!(
        field_error,
        BootError::PayloadTooLarge(message) if message == "multipart field exceeds 4 bytes"
    ));
}

#[tokio::test]
async fn multipart_form_enforces_file_and_count_limits() {
    let request = multipart_request(
        "file-boundary",
        concat!(
            "--file-boundary\r\n",
            "Content-Disposition: form-data; name=\"avatar\"; filename=\"one.txt\"\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "abcdef\r\n",
            "--file-boundary\r\n",
            "Content-Disposition: form-data; name=\"avatar\"; filename=\"two.txt\"\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "ghijkl\r\n",
            "--file-boundary--\r\n",
        ),
    );

    let file_error = request
        .multipart_form_with_options(MultipartOptions::new().with_max_file_size(4))
        .await
        .unwrap_err();
    let count_error = request
        .multipart_form_with_options(MultipartOptions::new().with_max_files(1))
        .await
        .unwrap_err();

    assert!(matches!(
        file_error,
        BootError::PayloadTooLarge(message) if message == "multipart file exceeds 4 bytes"
    ));
    assert!(matches!(
        count_error,
        BootError::PayloadTooLarge(message) if message == "multipart files exceeds 1 entries"
    ));
}

fn multipart_request(boundary: &str, body: &'static str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, "/upload")
        .with_content_type(format!("multipart/form-data; boundary={boundary}"))
        .with_body(body)
}
