//! The usage and duration hooks see every request that passes authorization,
//! and the duration hook reports whether the request succeeded.

use std::sync::{Arc, Mutex};

use axum_test::multipart::MultipartForm;
use ras_file_core::{DownloadResponse, FileError, FileRequestContext, JsonResponse};
use ras_file_macro::file_service;
use serde::{Deserialize, Serialize};

mod support;
use support::{MockAuthProvider, mock_http_server};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct UploadResponse {
    title: String,
}

file_service!({
    service_name: HookFiles,
    base_path: "/files",
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 2048,
            reject_unknown_fields: true,
            parts: [
                text title {
                    required: true,
                    max_bytes: 1024,
                },
            ],
        } -> UploadResponse,
        DOWNLOAD UNAUTHORIZED download/{id: String},
        DOWNLOAD WITH_PERMISSIONS(["admin"]) guarded/{id: String},
    ]
});

struct HookFilesImpl;

#[async_trait::async_trait]
impl HookFilesTrait for HookFilesImpl {
    type UploadState = Option<String>;

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &HookFilesUploadPath,
    ) -> ras_file_core::FileResult<Self::UploadState> {
        Ok(None)
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &HookFilesUploadPath,
        state: &mut Self::UploadState,
        part: &mut HookFilesUploadPart<'_>,
    ) -> ras_file_core::FileResult<()> {
        match part {
            HookFilesUploadPart::Title(title) => *state = Some(title.clone()),
            HookFilesUploadPart::__Lifetime(_) => {}
        }
        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &HookFilesUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> ras_file_core::FileResult<JsonResponse<UploadResponse>> {
        Ok(JsonResponse::ok(UploadResponse {
            title: state.ok_or_else(|| FileError::bad_request("title missing"))?,
        }))
    }

    async fn upload_abort(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &HookFilesUploadPath,
        _state: Self::UploadState,
        _error: &FileError,
    ) {
    }

    async fn download_by_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        path: HookFilesDownloadByIdPath,
    ) -> ras_file_core::FileResult<DownloadResponse> {
        if path.id == "missing" {
            return Err(FileError::NotFound);
        }
        Ok(DownloadResponse::bytes("content"))
    }

    async fn guarded_by_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: HookFilesGuardedByIdPath,
    ) -> ras_file_core::FileResult<DownloadResponse> {
        Ok(DownloadResponse::bytes("secret"))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Completed {
    route: String,
    user: Option<String>,
    success: bool,
}

#[derive(Clone, Default)]
struct Recorded {
    started: Arc<Mutex<Vec<String>>>,
    completed: Arc<Mutex<Vec<Completed>>>,
}

impl Recorded {
    fn started(&self) -> Vec<String> {
        self.started.lock().unwrap().clone()
    }

    fn completed(&self) -> Vec<Completed> {
        self.completed.lock().unwrap().clone()
    }
}

fn server() -> (axum_test::TestServer, Recorded) {
    let recorded = Recorded::default();
    let started = recorded.started.clone();
    let completed = recorded.completed.clone();

    let router = HookFilesBuilder::<HookFilesImpl, MockAuthProvider>::new(HookFilesImpl)
        .auth_provider(MockAuthProvider::default())
        .with_usage_tracker(move |_headers, _user, method, path| {
            started.lock().unwrap().push(format!("{method} {path}"));
            async {}
        })
        .with_method_duration_tracker(move |method, path, user, _duration, success| {
            completed.lock().unwrap().push(Completed {
                route: format!("{method} {path}"),
                user: user.map(|user| user.user_id.clone()),
                success,
            });
            async {}
        })
        .build();

    (mock_http_server(router), recorded)
}

fn completed(route: &str, user: Option<&str>, success: bool) -> Completed {
    Completed {
        route: route.to_string(),
        user: user.map(str::to_string),
        success,
    }
}

#[tokio::test]
async fn a_successful_download_is_recorded_as_a_success() {
    let (server, recorded) = server();

    server.get("/files/download/abc").await.assert_status_ok();

    assert_eq!(recorded.started(), vec!["GET /download/{id}"]);
    assert_eq!(
        recorded.completed(),
        vec![completed("GET /download/{id}", None, true)]
    );
}

#[tokio::test]
async fn a_handler_error_is_recorded_as_a_failure() {
    let (server, recorded) = server();

    server
        .get("/files/download/missing")
        .await
        .assert_status_not_found();

    assert_eq!(recorded.started(), vec!["GET /download/{id}"]);
    assert_eq!(
        recorded.completed(),
        vec![completed("GET /download/{id}", None, false)]
    );
}

#[tokio::test]
async fn upload_success_and_failure_are_both_recorded() {
    let (server, recorded) = server();

    server
        .post("/files/upload")
        .multipart(MultipartForm::new().add_text("title", "demo"))
        .await
        .assert_status_ok();
    server
        .post("/files/upload")
        .multipart(MultipartForm::new().add_text("unknown", "x"))
        .expect_failure()
        .await;

    assert_eq!(recorded.started(), vec!["POST /upload", "POST /upload"]);
    assert_eq!(
        recorded.completed(),
        vec![
            completed("POST /upload", None, true),
            completed("POST /upload", None, false),
        ]
    );
}

#[tokio::test]
async fn an_authorized_request_reports_its_user() {
    let (server, recorded) = server();

    server
        .get("/files/guarded/abc")
        .authorization_bearer("admin-token")
        .await
        .assert_status_ok();

    assert_eq!(
        recorded.completed(),
        vec![completed("GET /guarded/{id}", Some("admin-1"), true)]
    );
}

#[tokio::test]
async fn rejected_requests_reach_neither_hook() {
    let (server, recorded) = server();

    server.get("/files/guarded/abc").expect_failure().await;
    server
        .get("/files/guarded/abc")
        .authorization_bearer("user-token")
        .expect_failure()
        .await;

    assert!(recorded.started().is_empty(), "{:?}", recorded.started());
    assert!(
        recorded.completed().is_empty(),
        "{:?}",
        recorded.completed()
    );
}
