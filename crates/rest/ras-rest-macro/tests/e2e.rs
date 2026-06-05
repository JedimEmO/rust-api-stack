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

struct DemoImpl;

#[async_trait::async_trait]
impl DemoTrait for DemoImpl {
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

#[tokio::test]
async fn unauth_get_round_trips() {
    let response = server().get("/api/items").await;
    response.assert_status_ok();
    let resp: ItemsResponse = response.json();

    assert_eq!(resp.items.len(), 1);
    assert_eq!(resp.items[0].name, "alpha");
}

#[tokio::test]
async fn legacy_rest_version_round_trips_through_canonical_handler() {
    let response = server()
        .post("/api/v1/items/7/rename?notify=true")
        .json(&RenameItemV1 {
            name: "renamed".to_string(),
        })
        .await;
    response.assert_status_ok();
    let resp: RenamedItemV1 = response.json();

    assert_eq!(
        resp,
        RenamedItemV1 {
            name: "renamed".to_string()
        }
    );
}

#[tokio::test]
async fn canonical_rest_version_uses_v2_path_and_types() {
    let response = server()
        .post("/api/v2/items/8/rename?notify=false")
        .json(&RenameItemV2 {
            display_name: "canonical".to_string(),
            notify: true,
        })
        .await;
    response.assert_status_ok();
    let resp: RenamedItemV2 = response.json();

    assert_eq!(
        resp,
        RenamedItemV2 {
            id: 8,
            display_name: "canonical".to_string(),
            notified: true,
        }
    );
}

#[tokio::test]
async fn auth_get_with_path_param_succeeds_with_user_token() {
    let response = server()
        .get("/api/items/7")
        .authorization_bearer("user-token")
        .await;
    response.assert_status_ok();
    let item: Item = response.json();

    assert_eq!(item.id, 7);
    assert_eq!(item.name, "item-7");
}

#[tokio::test]
async fn auth_get_rejected_without_token() {
    let response = server().get("/api/items/1").await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_post_rejected_with_insufficient_perms() {
    let response = server()
        .post("/api/items")
        .authorization_bearer("user-token")
        .json(&CreateItem {
            name: "x".to_string(),
        })
        .await;
    response.assert_status(StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn auth_post_with_admin_succeeds_and_user_id_propagates() {
    let response = server()
        .post("/api/items")
        .authorization_bearer("admin-token")
        .json(&CreateItem { name: "foo".into() })
        .await;
    response.assert_status(StatusCode::CREATED);
    let item: Item = response.json();

    assert_eq!(item.name, "foo");
    // admin-1 is 7 chars long.
    assert_eq!(item.id, 7);
}

#[tokio::test]
async fn query_params_required_and_optional_serialize_correctly() {
    // Drive the generated client over the in-process transport so the
    // serde_urlencoded query path is exercised live (required + Option-skip).
    let client = client();

    let resp = client
        .get_search("hi".to_string(), Some(3), true)
        .await
        .expect("get_search with limit failed");
    assert_eq!(resp.items.len(), 3);
    assert_eq!(resp.items[0].name, "exact:hi-0");
    assert_eq!(resp.items[2].name, "exact:hi-2");

    // `limit: None` must be skipped from the query string entirely.
    let resp = client
        .get_search("zz".to_string(), None, false)
        .await
        .expect("get_search without limit failed");
    assert_eq!(resp.items.len(), 2);
    assert_eq!(resp.items[0].name, "fuzzy:zz-0");
}

#[tokio::test]
async fn vec_query_params_serialize_as_repeated_keys() {
    // `Vec<T>` and `Option<Vec<T>>` query params must serialize as repeated
    // keys through the generated client.
    let client = client();

    let resp = client
        .get_filter(
            vec!["red".to_string(), "blue".to_string()],
            Some(vec!["featured".to_string()]),
        )
        .await
        .expect("get_filter with tags failed");
    let names: Vec<_> = resp.items.into_iter().map(|item| item.name).collect();
    assert_eq!(names, vec!["tag:red", "tag:blue", "optional:featured"]);

    let resp = client
        .get_filter(vec!["solo".to_string()], None)
        .await
        .expect("get_filter solo failed");
    let names: Vec<_> = resp.items.into_iter().map(|item| item.name).collect();
    assert_eq!(names, vec!["tag:solo"]);
}

#[tokio::test]
async fn enum_query_params_use_serde_renames_without_display() {
    // Enum query values must honor `#[serde(rename)]` (asc/desc) rather than
    // any Display/Debug formatting.
    let client = client();

    let resp = client
        .get_sorted(SortOrder::Asc)
        .await
        .expect("get_sorted asc failed");
    assert_eq!(resp.items[0].name, "order:asc");

    let resp = client
        .get_sorted(SortOrder::Desc)
        .await
        .expect("get_sorted desc failed");
    assert_eq!(resp.items[0].name, "order:desc");
}

#[tokio::test]
async fn query_params_with_body_and_auth() {
    // Combined: bool query param + JSON body + bearer auth, via the client.
    let mut client = client();
    client.set_bearer_token(Some("admin-token"));

    let item = client
        .post_items_batch(true, CreateItem { name: "alpha".into() })
        .await
        .expect("post_items_batch notify=true failed");
    assert_eq!(item.name, "alpha(notified)");

    let item = client
        .post_items_batch(false, CreateItem { name: "beta".into() })
        .await
        .expect("post_items_batch notify=false failed");
    assert_eq!(item.name, "beta(silent)");
}

#[tokio::test]
async fn query_params_with_path_param() {
    // Path param substitution + Option query param + bearer auth, via client.
    let mut client = client();
    client.set_bearer_token(Some("user-token"));

    let resp = client
        .get_items_by_id_related(42, Some("featured".to_string()))
        .await
        .expect("get_items_by_id_related with tag failed");
    assert_eq!(resp.items[0].id, 42);
    assert_eq!(resp.items[0].name, "related/featured");

    let resp = client
        .get_items_by_id_related(42, None)
        .await
        .expect("get_items_by_id_related without tag failed");
    assert_eq!(resp.items[0].name, "related/none");
}

#[tokio::test]
async fn handler_error_surfaces_to_client() {
    let response = server()
        .get("/api/items/404")
        .authorization_bearer("user-token")
        .await;
    response.assert_status(StatusCode::NOT_FOUND);
}
