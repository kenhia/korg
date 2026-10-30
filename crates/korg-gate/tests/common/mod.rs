//! A gate wired to an in-process stub of korg-api's `/mcp`, both on loopback.

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

async fn stub_mcp(
    axum::extract::State(seen): axum::extract::State<Seen>,
    headers: HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let msg: Value = serde_json::from_slice(&body).unwrap();
    seen.0.lock().unwrap().push((headers, msg.clone()));
    let id = msg.get("id").cloned();
    let Some(id) = id else {
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match msg["method"].as_str().unwrap() {
        "initialize" => json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} },
                                "serverInfo": { "name": "stub", "version": "0" } }),
        "tools/list" => json!({ "tools": catalogue().iter()
                                .map(|n| json!({ "name": n, "inputSchema": { "type": "object" } }))
                                .collect::<Vec<_>>() }),
        "tools/call" => json!({ "content": [{ "type": "text",
                                "text": format!("called {}", msg["params"]["name"].as_str().unwrap()) }] }),
        _ => json!({}),
    };
    let mut resp = Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response();
    resp.headers_mut()
        .insert("mcp-session-id", "stub-session".parse().unwrap());
    resp
}

pub struct Harness {
    pub base: String,
    pub seen: Seen,
    pub http: reqwest::Client,
    pub state_dir: std::path::PathBuf,
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
    pub async fn start() -> Self {
        let seen = Seen::default();
        let upstream = spawn(
            Router::new()
                .route("/mcp", post(stub_mcp))
                .with_state(seen.clone()),
        )
        .await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut buf = [0u8; 8];
        getrandom::getrandom(&mut buf).unwrap();
        let state_dir = std::env::temp_dir().join(format!(
            "korg-gate-it-{}",
            buf.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        let config = Config {
            listen: addr,
            public_url: format!("http://{addr}/mcp").parse().unwrap(),
            upstream: format!("http://{upstream}/mcp").parse().unwrap(),
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
        self.http
            .post(self.url("/authorize"))
            .header("content-type", "application/x-www-form-urlencoded")
            .header("x-forwarded-for", "198.51.100.7")
            .body(form(&[
                ("response_type", "code"),
                ("client_id", client_id),
                ("redirect_uri", CALLBACK),
                ("code_challenge", challenge),
                ("code_challenge_method", "S256"),
                ("state", "st-123"),
                ("passphrase", passphrase),
            ]))
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
        let client_id = self.register().await;
        let verifier = "a-verifier-that-is-long-enough-to-be-realistic-0123456789";
        let resp = self
            .sign_in(&client_id, &pkce_s256(verifier), PASSPHRASE)
            .await;
        let code = code_from(&resp);
        let (status, body) = self
            .token(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", CALLBACK),
                ("client_id", &client_id),
                ("code_verifier", verifier),
            ])
            .await;
        assert_eq!(status, 200, "{body}");
        (
            client_id,
            body["access_token"].as_str().unwrap().to_string(),
            body["refresh_token"].as_str().unwrap().to_string(),
        )
    }

    pub async fn rpc(&self, token: &str, msg: Value) -> reqwest::Response {
        self.http
            .post(self.url("/mcp"))
            .bearer_auth(token)
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
