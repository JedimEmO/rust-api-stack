use ras_file_core::{DownloadResponse, FileRequestContext, JsonResponse};
use ras_file_macro::file_service;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct UploadResponse {
    id: String,
}

file_service!({
    service_name: SimpleService,
    base_path: "/simple",
    endpoints: [
        UPLOAD UNAUTHORIZED upload multipart {
            max_total_bytes: 1024,
            parts: [
                text title {
                    required: true,
                    max_bytes: 128,
                },
            ],
        } -> UploadResponse,
        DOWNLOAD UNAUTHORIZED download/{id: String},
    ]
});

struct SimpleImpl;

#[async_trait::async_trait]
impl SimpleServiceTrait for SimpleImpl {
    type UploadState = Option<String>;

    async fn upload_begin(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &SimpleServiceUploadPath,
    ) -> ras_file_core::FileResult<Self::UploadState> {
        Ok(None)
    }

    async fn upload_part(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &SimpleServiceUploadPath,
        state: &mut Self::UploadState,
        part: &mut SimpleServiceUploadPart<'_>,
    ) -> ras_file_core::FileResult<()> {
        match part {
            SimpleServiceUploadPart::Title(title) => *state = Some(title.clone()),
            SimpleServiceUploadPart::__Lifetime(_) => {}
        }
        Ok(())
    }

    async fn upload_finish(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: &SimpleServiceUploadPath,
        state: Self::UploadState,
        _summary: ras_file_core::UploadSummary,
    ) -> ras_file_core::FileResult<JsonResponse<UploadResponse>> {
        Ok(JsonResponse::ok(UploadResponse {
            id: state.unwrap_or_default(),
        }))
    }

    async fn download_by_id(
        &self,
        _ctx: &FileRequestContext<'_>,
        _path: SimpleServiceDownloadByIdPath,
    ) -> ras_file_core::FileResult<DownloadResponse> {
        Ok(DownloadResponse::bytes("ok"))
    }
}

#[test]
fn v2_simple_service_expands() {
    let _ = SimpleServiceBuilder::<SimpleImpl, support::NoAuth>::new(SimpleImpl).build();
}

mod support {
    #[derive(Clone)]
    pub struct NoAuth;

    impl ras_auth_core::AuthProvider for NoAuth {
        fn authenticate(&self, _token: String) -> ras_auth_core::AuthFuture<'_> {
            Box::pin(async { Err(ras_auth_core::AuthError::InvalidToken) })
        }
    }
}
