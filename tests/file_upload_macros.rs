#![cfg(all(feature = "macros", feature = "file-upload"))]

use std::sync::Arc;

use a3s_boot::{
    controller, injectable, BootApplication, BootError, BootRequest, ControllerDefinition,
    HttpMethod, Module, OpenApiInfo, Result, UploadedFile,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[injectable]
#[derive(Debug)]
struct UploadController;

#[controller("/uploads")]
impl UploadController {
    #[a3s_boot::post("/avatar", status = 201)]
    async fn avatar(
        &self,
        #[a3s_boot::uploaded_file("avatar")] avatar: UploadedFile,
    ) -> Result<UploadResult> {
        Ok(UploadResult {
            count: 1,
            file_names: vec![avatar.file_name().to_string()],
            bytes: avatar.size(),
        })
    }

    #[a3s_boot::post("/optional", status = 200)]
    async fn optional_avatar(
        &self,
        #[a3s_boot::uploaded_file("avatar")] avatar: Option<UploadedFile>,
    ) -> Result<UploadResult> {
        Ok(UploadResult {
            count: usize::from(avatar.is_some()),
            file_names: avatar
                .as_ref()
                .map(|file| vec![file.file_name().to_string()])
                .unwrap_or_default(),
            bytes: avatar.as_ref().map_or(0, UploadedFile::size),
        })
    }

    #[a3s_boot::post("/photos", status = 201)]
    async fn photos(
        &self,
        #[a3s_boot::uploaded_files("photos")] photos: Vec<UploadedFile>,
    ) -> Result<UploadResult> {
        Ok(UploadResult {
            count: photos.len(),
            file_names: photos
                .iter()
                .map(|file| file.file_name().to_string())
                .collect(),
            bytes: photos.iter().map(UploadedFile::size).sum(),
        })
    }
}

#[derive(Debug)]
struct UploadModule;

impl Module for UploadModule {
    fn name(&self) -> &'static str {
        "uploads"
    }

    fn controllers(&self, _module_ref: &a3s_boot::ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(UploadController).controller()?])
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
struct UploadResult {
    count: usize,
    file_names: Vec<String>,
    bytes: usize,
}

#[tokio::test]
async fn uploaded_file_macros_extract_single_optional_and_repeated_files() {
    let app = BootApplication::builder()
        .import(UploadModule)
        .build()
        .unwrap();

    let avatar = app
        .call(multipart_request(
            "/uploads/avatar",
            "avatar-boundary",
            concat!(
                "--avatar-boundary\r\n",
                "Content-Disposition: form-data; name=\"avatar\"; filename=\"milo.txt\"\r\n",
                "Content-Type: text/plain\r\n",
                "\r\n",
                "hello\r\n",
                "--avatar-boundary--\r\n",
            ),
        ))
        .await
        .unwrap();

    assert_eq!(avatar.status(), 201);
    assert_eq!(
        avatar.body_json::<UploadResult>().unwrap(),
        UploadResult {
            count: 1,
            file_names: vec!["milo.txt".to_string()],
            bytes: 5,
        }
    );

    let optional = app
        .call(multipart_request(
            "/uploads/optional",
            "optional-boundary",
            concat!(
                "--optional-boundary\r\n",
                "Content-Disposition: form-data; name=\"title\"\r\n",
                "\r\n",
                "no avatar\r\n",
                "--optional-boundary--\r\n",
            ),
        ))
        .await
        .unwrap();

    assert_eq!(optional.status(), 200);
    assert_eq!(
        optional.body_json::<UploadResult>().unwrap(),
        UploadResult {
            count: 0,
            file_names: Vec::new(),
            bytes: 0,
        }
    );

    let photos = app
        .call(multipart_request(
            "/uploads/photos",
            "photos-boundary",
            concat!(
                "--photos-boundary\r\n",
                "Content-Disposition: form-data; name=\"photos\"; filename=\"one.txt\"\r\n",
                "Content-Type: text/plain\r\n",
                "\r\n",
                "one\r\n",
                "--photos-boundary\r\n",
                "Content-Disposition: form-data; name=\"photos\"; filename=\"two.txt\"\r\n",
                "Content-Type: text/plain\r\n",
                "\r\n",
                "two!\r\n",
                "--photos-boundary--\r\n",
            ),
        ))
        .await
        .unwrap();

    assert_eq!(photos.status(), 201);
    assert_eq!(
        photos.body_json::<UploadResult>().unwrap(),
        UploadResult {
            count: 2,
            file_names: vec!["one.txt".to_string(), "two.txt".to_string()],
            bytes: 7,
        }
    );
}

#[tokio::test]
async fn uploaded_file_macro_rejects_missing_required_file() {
    let app = BootApplication::builder()
        .import(UploadModule)
        .build()
        .unwrap();

    let error = app
        .call(multipart_request(
            "/uploads/avatar",
            "missing-boundary",
            concat!(
                "--missing-boundary\r\n",
                "Content-Disposition: form-data; name=\"title\"\r\n",
                "\r\n",
                "no avatar\r\n",
                "--missing-boundary--\r\n",
            ),
        ))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::BadRequest(message) if message == "missing uploaded file: avatar"
    ));
}

#[test]
fn uploaded_file_macros_generate_multipart_openapi_request_bodies() {
    let app = BootApplication::builder()
        .import(UploadModule)
        .build()
        .unwrap();
    let document = serde_json::to_value(app.openapi(OpenApiInfo::new("Uploads", "1.0.0"))).unwrap();

    let avatar = &document["paths"]["/uploads/avatar"]["post"]["requestBody"]["content"]
        ["multipart/form-data"]["schema"];
    assert_eq!(
        avatar["properties"]["avatar"],
        json!({ "type": "string", "format": "binary" })
    );
    assert_eq!(avatar["required"], json!(["avatar"]));

    let optional = &document["paths"]["/uploads/optional"]["post"]["requestBody"]["content"]
        ["multipart/form-data"]["schema"];
    assert_eq!(
        optional["properties"]["avatar"],
        json!({ "type": "string", "format": "binary" })
    );
    assert!(optional["required"].is_null());

    let photos = &document["paths"]["/uploads/photos"]["post"]["requestBody"]["content"]
        ["multipart/form-data"]["schema"];
    assert_eq!(
        photos["properties"]["photos"],
        json!({
            "type": "array",
            "items": { "type": "string", "format": "binary" }
        })
    );
    assert_eq!(photos["required"], json!(["photos"]));
}

fn multipart_request(path: &str, boundary: &str, body: &'static str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path)
        .with_header("accept", "application/json")
        .with_content_type(format!("multipart/form-data; boundary={boundary}"))
        .with_body(body)
}
