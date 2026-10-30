//! The OAuth authorization server half of the gate.
//!
//! Shaped by Claude's connector contract
//! (<https://claude.com/docs/connectors/building/authentication.md>): DCR with
//! a public client, S256 PKCE advertised and enforced, a form-urlencoded token
//! endpoint, rotating refresh tokens, and `invalid_grant` for a dead one. One
//! scope, `korg:read`, because read is all the gate ever grants.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::Url;

use crate::config::CLAUDE_CALLBACK;
use crate::store::{Gate as Lock, GrantError, RegisterError};
use crate::AppState;

pub const SCOPE: &str = "korg:read";
const MAX_CLIENT_NAME: usize = 120;

pub async fn protected_resource(State(app): State<AppState>) -> Json<Value> {
    Json(json!({
        "resource": app.config.resource(),
        "authorization_servers": [app.config.issuer()],
        "bearer_methods_supported": ["header"],
        "scopes_supported": [SCOPE],
        "resource_name": "korg (read-only)",
    }))
}

pub async fn server_metadata(State(app): State<AppState>) -> Json<Value> {
    let c = &app.config;
    Json(json!({
        "issuer": c.issuer(),
        "authorization_endpoint": c.endpoint("/authorize"),
        "token_endpoint": c.endpoint("/token"),
        "registration_endpoint": c.endpoint("/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        // `offline_access` listed so Claude asks for it and expects a refresh
        // token; the gate issues one on every exchange regardless.
        "scopes_supported": [SCOPE, "offline_access"],
    }))
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    let mut resp = (
        status,
        Json(json!({ "error": error, "error_description": description })),
    )
        .into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// RFC 7591 dynamic client registration. Unauthenticated by nature, so the
/// only thing it can be trusted to do is refuse any redirect URI but Claude's.
pub async fn register(State(app): State<AppState>, body: axum::body::Bytes) -> Response {
    let Ok(Value::Object(meta)) = serde_json::from_slice::<Value>(&body) else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "body must be a JSON object",
        );
    };
    let redirect_uris: Vec<String> = match meta.get("redirect_uris") {
        Some(Value::Array(items)) if !items.is_empty() => {
            match items
                .iter()
                .map(|v| v.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()
            {
                Some(uris) => uris,
                None => {
                    return oauth_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_redirect_uri",
                        "redirect_uris must be strings",
                    )
                }
            }
        }
        _ => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_redirect_uri",
                "redirect_uris is required",
            )
        }
    };
    if let Some(bad) = redirect_uris.iter().find(|u| *u != CLAUDE_CALLBACK) {
        tracing::warn!(redirect_uri = %bad, "registration refused: redirect not allowlisted");
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            "only the claude.ai connector callback may be registered",
        );
    }
    if let Some(method) = meta.get("token_endpoint_auth_method") {
        if method != "none" {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "only public clients (token_endpoint_auth_method \"none\") are supported",
            );
        }
    }
    if let Some(Value::Array(grants)) = meta.get("grant_types") {
        let ok = grants
            .iter()
            .all(|g| g == "authorization_code" || g == "refresh_token");
        if !ok {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_client_metadata",
                "grant_types may only be authorization_code and refresh_token",
            );
        }
    }
    let client_name = meta
        .get("client_name")
        .and_then(Value::as_str)
        .map(|n| n.chars().take(MAX_CLIENT_NAME).collect::<String>());

    match app
        .store
        .register(client_name.clone(), redirect_uris.clone())
    {
        Ok(Ok((client_id, client))) => {
            tracing::info!(client = %short(&client_id), name = ?client_name, "client registered");
            let mut body = json!({
                "client_id": client_id,
                "client_id_issued_at": client.issued_at,
                "redirect_uris": client.redirect_uris,
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
            });
            if let Some(name) = client.client_name {
                body["client_name"] = json!(name);
            }
            (StatusCode::CREATED, Json(body)).into_response()
        }
        Ok(Err(RegisterError::Full)) => oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "client table is full",
        ),
        Err(e) => {
            tracing::error!(error = %e, "persisting a registration failed");
            oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "")
        }
    }
}

/// The authorization request, validated. Held by both halves of the sign-in
/// page so the POST re-checks everything the GET did — the form's hidden
/// fields are client-supplied and prove nothing.
struct AuthRequest {
    client_id: String,
    client_name: Option<String>,
    redirect_uri: String,
    code_challenge: String,
    state: Option<String>,
}

enum AuthRejection {
    /// The client or redirect cannot be trusted: show an error, never redirect.
    Page(StatusCode, &'static str),
    /// The redirect is trusted; the error goes back to it (RFC 6749 §4.1.2.1).
    Redirect(Box<Response>),
}

fn validate_auth(
    app: &AppState,
    p: &HashMap<String, String>,
) -> Result<AuthRequest, AuthRejection> {
    let get = |k: &str| p.get(k).filter(|v| !v.is_empty()).cloned();
    let client_id = get("client_id").ok_or(AuthRejection::Page(
        StatusCode::BAD_REQUEST,
        "Missing client_id.",
    ))?;
    let client = app.store.client(&client_id).ok_or(AuthRejection::Page(
        StatusCode::BAD_REQUEST,
        "Unknown client. Remove the connector in claude.ai and add it again.",
    ))?;
    let redirect_uri = get("redirect_uri").ok_or(AuthRejection::Page(
        StatusCode::BAD_REQUEST,
        "Missing redirect_uri.",
    ))?;
    if redirect_uri != CLAUDE_CALLBACK || !client.redirect_uris.contains(&redirect_uri) {
        return Err(AuthRejection::Page(
            StatusCode::BAD_REQUEST,
            "That redirect URI is not allowed.",
        ));
    }
    let state = get("state");
    let back = |error: &str| {
        AuthRejection::Redirect(Box::new(redirect_with(
            &redirect_uri,
            &[("error", error)],
            state.as_deref(),
        )))
    };
    if get("response_type").as_deref() != Some("code") {
        return Err(back("unsupported_response_type"));
    }
    let Some(code_challenge) = get("code_challenge") else {
        return Err(back("invalid_request"));
    };
    if get("code_challenge_method").as_deref() != Some("S256") {
        return Err(back("invalid_request"));
    }
    if let Some(resource) = get("resource") {
        if resource != app.config.resource() {
            return Err(back("invalid_target"));
        }
    }
    Ok(AuthRequest {
        client_id,
        client_name: client.client_name,
        redirect_uri,
        code_challenge,
        state,
    })
}

fn redirect_with(base: &str, pairs: &[(&str, &str)], state: Option<&str>) -> Response {
    let mut url = Url::parse(base).expect("redirect URIs are validated before use");
    {
        let mut q = url.query_pairs_mut();
        for (k, v) in pairs {
            q.append_pair(k, v);
        }
        if let Some(s) = state {
            q.append_pair("state", s);
        }
    }
    Redirect::to(url.as_str()).into_response()
}

pub async fn authorize_page(
    State(app): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    match validate_auth(&app, &params) {
        Ok(req) => sign_in_page(&req, None, StatusCode::OK),
        Err(AuthRejection::Page(status, msg)) => error_page(status, msg),
        Err(AuthRejection::Redirect(r)) => *r,
    }
}

pub async fn authorize_submit(
    State(app): State<AppState>,
    headers: HeaderMap,
    peer: Option<ConnectInfo<SocketAddr>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let req = match validate_auth(&app, &form) {
        Ok(req) => req,
        Err(AuthRejection::Page(status, msg)) => return error_page(status, msg),
        Err(AuthRejection::Redirect(r)) => return *r,
    };
    let ip = client_ip(&app, &headers, peer.map(|c| c.0));

    if let Lock::Locked { retry_after } = app.store.check_lockout() {
        tracing::warn!(%ip, client = %short(&req.client_id), "sign-in refused: locked out");
        let msg = format!(
            "Too many failed attempts. Try again in {} seconds.",
            retry_after.as_secs().max(1)
        );
        return sign_in_page(&req, Some(&msg), StatusCode::TOO_MANY_REQUESTS);
    }

    let given = form.get("passphrase").map(String::as_str).unwrap_or("");
    // Compare digests so the comparison is constant-time over equal lengths and
    // leaks nothing about the passphrase's length either.
    let ok: bool = Sha256::digest(given.as_bytes())
        .ct_eq(&Sha256::digest(app.config.passphrase.as_bytes()))
        .into();
    if !ok {
        let lock = app.store.sign_in_failed();
        tracing::warn!(%ip, client = %short(&req.client_id), lockout_secs = ?lock.map(|d| d.as_secs()), "sign-in failed");
        return sign_in_page(&req, Some("Wrong passphrase."), StatusCode::UNAUTHORIZED);
    }
    app.store.sign_in_succeeded();
    tracing::info!(%ip, client = %short(&req.client_id), "sign-in succeeded");
    let code = app.store.issue_code(
        &req.client_id,
        &req.redirect_uri,
        &req.code_challenge,
        SCOPE,
    );
    redirect_with(&req.redirect_uri, &[("code", &code)], req.state.as_deref())
}

/// The original client address: the last entry of the trusted proxy's header
/// (tailscaled appends it; entries to its left are whatever the caller sent),
/// falling back to the TCP peer.
fn client_ip(app: &AppState, headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    headers
        .get(app.config.client_ip_header.as_str())
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| peer.map(|p| p.ip().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

pub async fn token(
    State(app): State<AppState>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let get = |k: &str| form.get(k).map(String::as_str).filter(|v| !v.is_empty());
    if let Some(resource) = get("resource") {
        if resource != app.config.resource() {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_target",
                "unknown resource",
            );
        }
    }
    let result = match get("grant_type") {
        Some("authorization_code") => {
            let (Some(code), Some(redirect_uri), Some(client_id), Some(verifier)) = (
                get("code"),
                get("redirect_uri"),
                get("client_id"),
                get("code_verifier"),
            ) else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "code, redirect_uri, client_id and code_verifier are required",
                );
            };
            app.store
                .exchange_code(code, client_id, redirect_uri, verifier)
        }
        Some("refresh_token") => {
            let Some(refresh) = get("refresh_token") else {
                return oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "refresh_token is required",
                );
            };
            app.store.refresh(refresh, get("client_id"))
        }
        _ => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "authorization_code or refresh_token",
            )
        }
    };
    match result {
        Ok(Ok(tokens)) => {
            let mut resp = Json(json!({
                "access_token": tokens.access_token,
                "token_type": "Bearer",
                "expires_in": tokens.expires_in,
                "refresh_token": tokens.refresh_token,
                "scope": tokens.scope,
            }))
            .into_response();
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            resp
        }
        Ok(Err(why)) => {
            tracing::warn!(reason = ?why, "token request refused");
            let description = match why {
                GrantError::UnknownRefresh | GrantError::Expired => "grant is not live",
                _ => "grant does not match",
            };
            oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", description)
        }
        Err(e) => {
            tracing::error!(error = %e, "persisting a grant failed");
            oauth_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", "")
        }
    }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn page(status: StatusCode, body: String) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\">\
         <title>korg gate</title>\
         <style>body{{font:16px system-ui,sans-serif;max-width:28rem;margin:3rem auto;padding:0 1rem}}\
         input,button{{font:inherit;padding:.5rem;width:100%;box-sizing:border-box;margin:.25rem 0}}\
         .err{{color:#b00}}</style></head><body>{body}</body></html>"
    );
    let mut resp = (status, Html(html)).into_response();
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // Nobody may frame the sign-in page (clickjacking the passphrase form).
    // `form-action` names claude.ai too: browsers apply it to the redirect that
    // answers the form POST, and that redirect goes to Claude's callback.
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; \
             form-action 'self' https://claude.ai; frame-ancestors 'none'",
        ),
    );
    resp
}

fn error_page(status: StatusCode, msg: &str) -> Response {
    page(
        status,
        format!("<h1>korg gate</h1><p class=\"err\">{}</p>", escape(msg)),
    )
}

fn sign_in_page(req: &AuthRequest, error: Option<&str>, status: StatusCode) -> Response {
    let host = Url::parse(&req.redirect_uri)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    let who = req.client_name.as_deref().unwrap_or("An unnamed client");
    let hidden = |k: &str, v: &str| {
        format!(
            "<input type=\"hidden\" name=\"{k}\" value=\"{}\">",
            escape(v)
        )
    };
    let mut fields = String::new();
    fields += &hidden("response_type", "code");
    fields += &hidden("client_id", &req.client_id);
    fields += &hidden("redirect_uri", &req.redirect_uri);
    fields += &hidden("code_challenge", &req.code_challenge);
    fields += &hidden("code_challenge_method", "S256");
    if let Some(s) = &req.state {
        fields += &hidden("state", s);
    }
    let error = error
        .map(|e| format!("<p class=\"err\">{}</p>", escape(e)))
        .unwrap_or_default();
    page(
        status,
        format!(
            "<h1>korg gate</h1>\
             <p><strong>{}</strong> wants <strong>read-only</strong> access to korg. \
             You will be sent back to <strong>{}</strong>.</p>{error}\
             <form method=\"post\" action=\"/authorize\">{fields}\
             <label for=\"p\">Passphrase</label>\
             <input id=\"p\" name=\"passphrase\" type=\"password\" autocomplete=\"current-password\" autofocus required>\
             <button type=\"submit\">Allow read access</button></form>",
            escape(who),
            escape(&host),
        ),
    )
}
