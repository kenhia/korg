//! A gate wired to in-process stubs of korg-api's `/mcp` and klams's `/mcp`,
//! all on loopback. The klams stub answers the way klams 0.1.55 does: SSE, with
//! an empty priming event ahead of every answer.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use korg_gate::store::pkce_s256;
use korg_gate::{router, Config, Gate};
use serde_json::{json, Value};

pub const PASSPHRASE: &str = "correct horse battery staple, twice";
pub const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";

/// Every request the stub upstream saw: its headers and parsed body.
#[derive(Clone, Default)]
pub struct Seen(pub Arc<Mutex<Vec<(HeaderMap, Value)>>>);

/// korg's real tool catalogue — the names the stub advertises, so the
/// `tools/list` filter is tested against what korg actually serves.
pub fn catalogue() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../korg-mcp/tests/tools_schema.json"
    );
    let tools: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    tools
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

/// klams's real tool catalogue, committed at `tests/klams_tools.json` as a
/// **full-scope** view (read, write, manage and admin), so an admin tool klams
/// serves is ruled on too. klams narrows `tools/list` to the caller's scopes and
/// no live identity holds `admin`, so sprint 097 dumped
/// `klams_mcp::tools::all_tool_descriptors()` from klams's source instead, after
/// checking that its descriptors for the eleven tools a `claude` identity sees
/// equal the live `tools/list` byte for byte. See `docs/gate.md` § klams.
pub fn klams_catalogue() -> Vec<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/klams_tools.json");
    let tools: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    tools
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

/// What rmcp 3.1.2 (korg-api and klams alike) answers `server/discover` that
/// arrives without MCP 2026-07-28's `Mcp-Method` header, measured 2026-11-01.
pub fn missing_mcp_method(id: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id,
            "error": { "code": -32020, "message": "missing required Mcp-Method header" } })
}

/// The `server/discover` result's version list, as both upstreams answer it.
pub const SUPPORTED_VERSIONS: &[&str] = &[
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
    "2025-11-25",
    "2026-07-28",
];

fn answer(headers: &HeaderMap, msg: &Value, catalogue: Vec<String>) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg["method"].as_str().unwrap();
    if method == "server/discover" {
        return Some(match headers.get("mcp-method") {
            Some(_) => json!({ "jsonrpc": "2.0", "id": id, "result": {
                "resultType": "complete", "supportedVersions": SUPPORTED_VERSIONS,
                "capabilities": { "tools": {} } } }),
            None => missing_mcp_method(id),
        });
    }
    let result = match method {
        "initialize" => json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} },
                                "serverInfo": { "name": "stub", "version": "0" } }),
        "tools/list" => json!({ "tools": catalogue.iter()
                                .map(|n| json!({ "name": n, "inputSchema": { "type": "object" } }))
                                .collect::<Vec<_>>() }),
        "tools/call" => json!({ "content": [{ "type": "text",
                                "text": format!("called {}", msg["params"]["name"].as_str().unwrap()) }] }),
        _ => json!({}),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

async fn stub_klams(
    axum::extract::State(seen): axum::extract::State<Seen>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let msg: Value = serde_json::from_slice(&body).unwrap();
    seen.0.lock().unwrap().push((headers.clone(), msg.clone()));
    let Some(reply) = answer(&headers, &msg, klams_catalogue()) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let sse = format!("data: \nid: 0/0\nretry: 3000\n\ndata: {reply}\nid: 0/1\n\n");
    let mut resp = sse.into_response();
    resp.headers_mut()
        .insert("content-type", "text/event-stream".parse().unwrap());
    resp.headers_mut()
        .insert("mcp-session-id", "klams-session".parse().unwrap());
    resp
}

async fn stub_mcp(
    axum::extract::State(seen): axum::extract::State<Seen>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let msg: Value = serde_json::from_slice(&body).unwrap();
    seen.0.lock().unwrap().push((headers.clone(), msg.clone()));
    let Some(reply) = answer(&headers, &msg, catalogue()) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let mut resp = Json(reply).into_response();
    resp.headers_mut()
        .insert("mcp-session-id", "stub-session".parse().unwrap());
    resp
}

pub struct Harness {
    pub base: String,
    /// What the korg stub saw.
    pub seen: Seen,
    /// What the klams stub saw.
    pub klams_seen: Seen,
    pub http: reqwest::Client,
    pub state_dir: std::path::PathBuf,
}

pub struct Options {
    /// Configure the klams door (`KORG_GATE_UPSTREAM_KLAMS`).
    pub klams: bool,
    /// Start on an existing state directory instead of a fresh one.
    pub state_dir: Option<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            klams: true,
            state_dir: None,
        }
    }
}

pub fn temp_state_dir() -> std::path::PathBuf {
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf).unwrap();
    std::env::temp_dir().join(format!(
        "korg-gate-it-{}",
        buf.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

async fn spawn(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    addr
}

impl Harness {
    /// Both doors open — the shape krgate runs once klams's door is deployed,
    /// so every korg-door test also proves the klams door changes nothing.
    pub async fn start() -> Self {
        Self::start_with(Options::default()).await
    }

    pub async fn start_with(options: Options) -> Self {
        let seen = Seen::default();
        let upstream = spawn(
            Router::new()
                .route("/mcp", post(stub_mcp))
                .with_state(seen.clone()),
        )
        .await;
        let klams_seen = Seen::default();
        let klams = spawn(
            Router::new()
                .route("/mcp", post(stub_klams))
                .with_state(klams_seen.clone()),
        )
        .await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state_dir = options.state_dir.unwrap_or_else(temp_state_dir);
        let config = Config {
            listen: addr,
            public_url: format!("http://{addr}/mcp").parse().unwrap(),
            upstream: format!("http://{upstream}/mcp").parse().unwrap(),
            klams_upstream: options
                .klams
                .then(|| format!("http://{klams}/mcp").parse().unwrap()),
            passphrase: PASSPHRASE.into(),
            state_dir: state_dir.clone(),
            client_ip_header: "x-forwarded-for".into(),
            access_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(3600),
        };
        config.validate().unwrap();
        let app = router(Arc::new(Gate::new(config).unwrap()));
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            base: format!("http://{addr}"),
            seen,
            klams_seen,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            state_dir,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub async fn register(&self) -> String {
        let resp = self
            .http
            .post(self.url("/register"))
            .header("content-type", "application/json")
            .body(
                json!({ "client_name": "Claude", "redirect_uris": [CALLBACK],
                          "token_endpoint_auth_method": "none",
                          "grant_types": ["authorization_code", "refresh_token"],
                          "response_types": ["code"] })
                .to_string(),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body: Value = resp.json_value().await;
        body["client_id"].as_str().unwrap().to_string()
    }

    /// POST the sign-in form; returns the response (a redirect on success).
    pub async fn sign_in(
        &self,
        client_id: &str,
        challenge: &str,
        passphrase: &str,
    ) -> reqwest::Response {
        self.sign_in_with(client_id, challenge, passphrase, &[])
            .await
    }

    /// The sign-in form with extra fields, e.g. a `resource`.
    pub async fn sign_in_with(
        &self,
        client_id: &str,
        challenge: &str,
        passphrase: &str,
        extra: &[(&str, &str)],
    ) -> reqwest::Response {
        let mut pairs = vec![
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", CALLBACK),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", "st-123"),
            ("passphrase", passphrase),
        ];
        pairs.extend_from_slice(extra);
        self.http
            .post(self.url("/authorize"))
            .header("content-type", "application/x-www-form-urlencoded")
            .header("x-forwarded-for", "198.51.100.7")
            .body(form(&pairs))
            .send()
            .await
            .unwrap()
    }

    pub async fn token(&self, pairs: &[(&str, &str)]) -> (u16, Value) {
        let resp = self
            .http
            .post(self.url("/token"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form(pairs))
            .send()
            .await
            .unwrap();
        let status = resp.status().as_u16();
        (status, resp.json_value().await)
    }

    /// Register, sign in, exchange: an access token the way Claude gets one.
    pub async fn access_token(&self) -> (String, String, String) {
        self.access_token_with(&[]).await
    }

    /// The klams door's resource, as its metadata names it.
    pub fn klams_resource(&self) -> String {
        self.url("/klams/mcp")
    }

    /// A klams-door token, obtained the way claude.ai would: RFC 8707
    /// `resource` on both the sign-in and the exchange.
    pub async fn klams_token(&self) -> (String, String, String) {
        let resource = self.klams_resource();
        self.access_token_with(&[("resource", &resource)]).await
    }

    /// `access_token`, with `extra` added to both the sign-in form and the
    /// code exchange.
    pub async fn access_token_with(&self, extra: &[(&str, &str)]) -> (String, String, String) {
        let client_id = self.register().await;
        let verifier = "a-verifier-that-is-long-enough-to-be-realistic-0123456789";
        let resp = self
            .sign_in_with(&client_id, &pkce_s256(verifier), PASSPHRASE, extra)
            .await;
        let code = code_from(&resp);
        let mut pairs = vec![
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", CALLBACK),
            ("client_id", client_id.as_str()),
            ("code_verifier", verifier),
        ];
        pairs.extend_from_slice(extra);
        let (status, body) = self.token(&pairs).await;
        assert_eq!(status, 200, "{body}");
        (
            client_id,
            body["access_token"].as_str().unwrap().to_string(),
            body["refresh_token"].as_str().unwrap().to_string(),
        )
    }

    pub async fn rpc(&self, token: &str, msg: Value) -> reqwest::Response {
        self.rpc_at("/mcp", token, msg).await
    }

    pub async fn rpc_at(&self, path: &str, token: &str, msg: Value) -> reqwest::Response {
        self.rpc_with(path, token, msg, &[]).await
    }

    /// `rpc_at` with extra request headers, e.g. 2026-07-28's `Mcp-Method`.
    pub async fn rpc_with(
        &self,
        path: &str,
        token: &str,
        msg: Value,
        extra: &[(&str, &str)],
    ) -> reqwest::Response {
        let mut req = self.http.post(self.url(path));
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        req.bearer_auth(token)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(msg.to_string())
            .send()
            .await
            .unwrap()
    }
}

pub fn form(pairs: &[(&str, &str)]) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        s.append_pair(k, v);
    }
    s.finish()
}

/// The `code` from a sign-in redirect, asserting it went to Claude with state.
pub fn code_from(resp: &reqwest::Response) -> String {
    assert!(
        resp.status().is_redirection(),
        "sign-in did not redirect: {}",
        resp.status()
    );
    let location = resp.headers()["location"].to_str().unwrap();
    let url = url::Url::parse(location).unwrap();
    assert_eq!(
        format!("{}{}", url.origin().ascii_serialization(), url.path()),
        CALLBACK
    );
    let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(pairs.get("state").map(String::as_str), Some("st-123"));
    pairs["code"].clone()
}

pub trait JsonValue {
    async fn json_value(self) -> Value;
}

impl JsonValue for reqwest::Response {
    async fn json_value(self) -> Value {
        let text = self.text().await.unwrap();
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
    }
}
