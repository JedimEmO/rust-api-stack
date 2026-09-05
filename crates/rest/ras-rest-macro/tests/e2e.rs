//! End-to-end test: in-memory axum-test request -> axum router -> trait impl
//! -> response. Covers GET, POST with body, path params, query params, and
//! auth-related rejection paths.

use axum::http::StatusCode;
use ras_auth_core::AuthenticatedUser;
use ras_rest_core::{RestError, RestResponse, RestResult};
use ras_rest_macro::rest_service;
use serde::{Deserialize, Serialize};

mod support;
use support::{MockAuthProvider, axum_transport, mock_http_server, mock_http_server_arc};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct Item {
    id: u32,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct CreateItem {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct ItemsResponse {
    items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct WhoamiResponse {
    caller: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct RenameItemV1 {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
struct RenameItemV2 {
    display_name: String,
    notify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
struct RenamedItemV1 {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
struct RenamedItemV2 {
    id: u32,
    display_name: String,
    notified: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
enum SortOrder {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

rest_service!({
    service_name: Demo,
    base_path: "/api",
    openapi: false,
    serve_docs: false,
    endpoints: [
        /// List all items.
        GET UNAUTHORIZED items() -> ItemsResponse,
        GET OPTIONAL_AUTH whoami() -> WhoamiResponse,
        POST OPTIONAL_AUTH whoami/echo(CreateItem) -> WhoamiResponse,
        GET WITH_PERMISSIONS(["user"]) items/{id: u32}() -> Item,
        POST WITH_PERMISSIONS(["admin"]) items(CreateItem) -> Item,
        GET UNAUTHORIZED search ? q: String & limit: Option<u32> & exact: bool () -> ItemsResponse,
        GET UNAUTHORIZED filter ? tags: Vec<String> & optional_tags: Option<Vec<String>> () -> ItemsResponse,
        GET UNAUTHORIZED sorted ? order: SortOrder () -> ItemsResponse,
        POST WITH_PERMISSIONS(["admin"]) items/batch ? notify: bool (CreateItem) -> Item,
        GET WITH_PERMISSIONS(["user"]) items/{id: u32}/related ? tag: Option<String> () -> ItemsResponse,
        POST UNAUTHORIZED v2/items/{id: u32}/rename ? notify: bool (RenameItemV2) -> RenamedItemV2 {
            version: v2,
            versions: [
                v1 {
                    path: v1/items/{id: u32}/rename,
                    query: [notify: Option<bool>],
                    body: RenameItemV1,
                    response: RenamedItemV1,
                    migration: RenameItemCompat,
                },
            ],
        },
        // Versioned OPTIONAL_AUTH endpoint — exercises the legacy/migration handler
        // arm with caller wiring (otherwise instantiated by nothing in the workspace).
        POST OPTIONAL_AUTH v2/items/{id: u32}/touch ? notify: bool (RenameItemV2) -> RenamedItemV2 {
            version: v2,
            versions: [
                v1 {
                    path: v1/items/{id: u32}/touch,
                    query: [notify: Option<bool>],
                    body: RenameItemV1,
                    response: RenamedItemV1,
                    migration: TouchCompat,
                },
            ],
        },
    ]
});

struct RenameItemCompat;

impl
    ras_rest_core::VersionMigration<
        DemoPostV2ItemsByIdRenameV1Request,
        DemoPostV2ItemsByIdRenameV2Request,
    > for RenameItemCompat
{
    type Error = std::convert::Infallible;

    fn migrate(
        value: DemoPostV2ItemsByIdRenameV1Request,
    ) -> Result<DemoPostV2ItemsByIdRenameV2Request, Self::Error> {
        Ok(DemoPostV2ItemsByIdRenameV2Request {
            path: DemoPostV2ItemsByIdRenameV2Path { id: value.path.id },
            query: DemoPostV2ItemsByIdRenameV2Query {
                notify: value.query.notify.unwrap_or(false),
            },
            body: RenameItemV2 {
                display_name: value.body.name,
                notify: value.query.notify.unwrap_or(false),
            },
        })
    }
}

impl ras_rest_core::VersionMigration<RenamedItemV2, RenamedItemV1> for RenameItemCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenamedItemV2) -> Result<RenamedItemV1, Self::Error> {
        Ok(RenamedItemV1 {
            name: value.display_name,
        })
    }
}

struct TouchCompat;

impl
    ras_rest_core::VersionMigration<
        DemoPostV2ItemsByIdTouchV1Request,
        DemoPostV2ItemsByIdTouchV2Request,
    > for TouchCompat
{
    type Error = std::convert::Infallible;

    fn migrate(
        value: DemoPostV2ItemsByIdTouchV1Request,
    ) -> Result<DemoPostV2ItemsByIdTouchV2Request, Self::Error> {
        Ok(DemoPostV2ItemsByIdTouchV2Request {
            path: DemoPostV2ItemsByIdTouchV2Path { id: value.path.id },
            query: DemoPostV2ItemsByIdTouchV2Query {
                notify: value.query.notify.unwrap_or(false),
            },
            body: RenameItemV2 {
                display_name: value.body.name,
                notify: value.query.notify.unwrap_or(false),
            },
        })
    }
}

impl ras_rest_core::VersionMigration<RenamedItemV2, RenamedItemV1> for TouchCompat {
    type Error = std::convert::Infallible;

    fn migrate(value: RenamedItemV2) -> Result<RenamedItemV1, Self::Error> {
        Ok(RenamedItemV1 {
            name: value.display_name,
        })
    }
}

fn caller_label(caller: &ras_auth_core::Caller) -> String {
    match caller {
        ras_auth_core::Caller::Authenticated(user) => user.user_id.clone(),
        ras_auth_core::Caller::Anonymous => "anonymous".to_string(),
    }
}

struct DemoImpl;

#[async_trait::async_trait]
impl DemoTrait for DemoImpl {
    async fn get_whoami(&self, caller: ras_auth_core::Caller) -> RestResult<WhoamiResponse> {
        Ok(RestResponse::ok(WhoamiResponse {
            caller: caller_label(&caller),
        }))
    }

    async fn post_whoami_echo(
        &self,
        caller: ras_auth_core::Caller,
        body: CreateItem,
    ) -> RestResult<WhoamiResponse> {
        Ok(RestResponse::ok(WhoamiResponse {
            caller: format!("{}:{}", caller_label(&caller), body.name),
        }))
    }

    async fn get_items(&self) -> RestResult<ItemsResponse> {
        Ok(RestResponse::ok(ItemsResponse {
            items: vec![Item {
                id: 1,
                name: "alpha".into(),
            }],
        }))
    }

    async fn get_items_by_id(&self, _user: &AuthenticatedUser, id: u32) -> RestResult<Item> {
        if id == 404 {
            Err(RestError::not_found("missing"))
        } else {
            Ok(RestResponse::ok(Item {
                id,
                name: format!("item-{id}"),
            }))
        }
    }

    async fn post_items(&self, user: &AuthenticatedUser, body: CreateItem) -> RestResult<Item> {
        // Use the user_id length so we can verify the user actually arrived.
        Ok(RestResponse::created(Item {
            id: user.user_id.len() as u32,
            name: body.name,
        }))
    }

    async fn get_search(
        &self,
        q: String,
        limit: Option<u32>,
        exact: bool,
    ) -> RestResult<ItemsResponse> {
        let n = limit.unwrap_or(2);
        let prefix = if exact { "exact" } else { "fuzzy" };
        let items = (0..n)
            .map(|i| Item {
                id: i,
                name: format!("{prefix}:{q}-{i}"),
            })
            .collect();
        Ok(RestResponse::ok(ItemsResponse { items }))
    }

    async fn get_filter(
        &self,
        tags: Vec<String>,
        optional_tags: Option<Vec<String>>,
    ) -> RestResult<ItemsResponse> {
        let mut items: Vec<Item> = tags
            .into_iter()
            .enumerate()
            .map(|(idx, tag)| Item {
                id: idx as u32,
                name: format!("tag:{tag}"),
            })
            .collect();

        let offset = items.len();
        items.extend(
            optional_tags
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .map(|(idx, tag)| Item {
                    id: (offset + idx) as u32,
                    name: format!("optional:{tag}"),
                }),
        );

        Ok(RestResponse::ok(ItemsResponse { items }))
    }

    async fn get_sorted(&self, order: SortOrder) -> RestResult<ItemsResponse> {
        let label = match order {
            SortOrder::Asc => "asc",
            SortOrder::Desc => "desc",
        };

        Ok(RestResponse::ok(ItemsResponse {
            items: vec![Item {
                id: 0,
                name: format!("order:{label}"),
            }],
        }))
    }

    async fn post_items_batch(
        &self,
        _user: &AuthenticatedUser,
        notify: bool,
        body: CreateItem,
    ) -> RestResult<Item> {
        // Encode the bool query param into the response so we can assert on it.
        let suffix = if notify { "(notified)" } else { "(silent)" };
        Ok(RestResponse::created(Item {
            id: 0,
            name: format!("{}{suffix}", body.name),
        }))
    }

    async fn get_items_by_id_related(
        &self,
        _user: &AuthenticatedUser,
        id: u32,
        tag: Option<String>,
    ) -> RestResult<ItemsResponse> {
        let label = tag.unwrap_or_else(|| "none".into());
        Ok(RestResponse::ok(ItemsResponse {
            items: vec![Item {
                id,
                name: format!("related/{label}"),
            }],
        }))
    }

    async fn post_v2_items_by_id_rename(
        &self,
        id: u32,
        notify: bool,
        request: RenameItemV2,
    ) -> RestResult<RenamedItemV2> {
        Ok(RestResponse::ok(RenamedItemV2 {
            id,
            display_name: request.display_name,
            notified: notify || request.notify,
        }))
    }

    async fn post_v2_items_by_id_touch(
        &self,
        caller: ras_auth_core::Caller,
        id: u32,
        notify: bool,
        request: RenameItemV2,
    ) -> RestResult<RenamedItemV2> {
        // Encode the resolved caller so the legacy/migration arm's caller wiring is asserted.
        Ok(RestResponse::ok(RenamedItemV2 {
            id,
            display_name: format!("{}:{}", caller_label(&caller), request.display_name),
            notified: notify || request.notify,
        }))
    }
}

fn router() -> axum::Router {
    DemoBuilder::new(DemoImpl)
        .auth_provider(MockAuthProvider::default())
        .build()
}

fn server() -> axum_test::TestServer {
    mock_http_server(router())
}

/// A generated `DemoClient` wired over an in-process [`AxumTestTransport`].
/// The `server_url` is a placeholder origin — the test transport strips the
/// scheme+authority and routes by path+query against the in-memory router.
fn client() -> DemoClient {
    let server = mock_http_server_arc(router());
    let transport = axum_transport(server);
    DemoClientBuilder::new("http://in-memory.test")
        .build_with_transport(transport)
        .expect("failed to build DemoClient over AxumTestTransport")
}

#[path = "e2e/auth.rs"]
mod auth;
#[path = "e2e/client.rs"]
mod client;
#[path = "e2e/parameters.rs"]
mod parameters;
#[path = "e2e/versioning.rs"]
mod versioning;
