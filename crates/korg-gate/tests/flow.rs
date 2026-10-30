//! The whole OAuth flow, end to end, the way claude.ai drives it — and the
//! proxied read it buys (WI 4147's code-half Done list).

mod common;

use common::*;
use korg_gate::store::pkce_s256;
use serde_json::{json, Value};

#[tokio::test]
async fn unauthenticated_mcp_gets_the_exact_401_shape() {
    let h = Harness::start().await;
    let resp = h
        .http
        .post(h.url("/mcp"))
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    assert_eq!(
        resp.headers()["www-authenticate"].to_str().unwrap(),
        format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
            h.base
        )
    );
    // A bogus token is the same 401, not a different failure.
    let resp = h
        .rpc(
            "not-a-token",
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        )
        .await;
    assert_eq!(resp.status(), 401);
    for method in ["GET", "DELETE"] {
        let resp = h
            .http
            .request(method.parse().unwrap(), h.url("/mcp"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "{method}");
    }
    assert!(
        h.seen.0.lock().unwrap().is_empty(),
        "an unauthenticated call reached korg"
    );
}

#[tokio::test]
async fn full_oauth_flow_then_a_proxied_read_reaches_korg_stamped() {
    let h = Harness::start().await;

    // 1. Discovery: protected-resource metadata, then the server's.
    let prm: Value = h
        .http
        .get(h.url("/.well-known/oauth-protected-resource"))
        .send()
        .await
        .unwrap()
        .json_value()
        .await;
    assert_eq!(prm["resource"], format!("{}/mcp", h.base));
    let issuer = prm["authorization_servers"][0]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(issuer, h.base);
    let asm: Value = h
        .http
        .get(format!("{issuer}/.well-known/oauth-authorization-server"))
        .send()
        .await
        .unwrap()
        .json_value()
        .await;
    assert_eq!(asm["issuer"], issuer);
    assert_eq!(asm["code_challenge_methods_supported"], json!(["S256"]));
    assert_eq!(
        asm["token_endpoint_auth_methods_supported"],
        json!(["none"])
    );
    assert_eq!(asm["registration_endpoint"], format!("{issuer}/register"));

    // 2. DCR.
    let client_id = h.register().await;

    // 3. The sign-in page renders for a valid request.
    let verifier = "dBjftJeZ4CVP-mJ92K1-AlUQZsHBk3vSqyWjeuQ0MKc";
    let challenge = pkce_s256(verifier);
    let page = h
        .http
        .get(h.url("/authorize"))
        .query(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", CALLBACK),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", "st-123"),
            ("scope", "korg:read offline_access"),
            ("resource", &format!("{}/mcp", h.base)),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    let html = page.text().await.unwrap();
    assert!(
        html.contains("claude.ai"),
        "the page must name the redirect host"
    );
    assert!(html.contains("read-only"));

    // 4. Passphrase → code (redirected to Claude, with state).
    let code = code_from(&h.sign_in(&client_id, &challenge, PASSPHRASE).await);

    // 5. Code + PKCE → tokens.
    let (status, tokens) = h
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", CALLBACK),
            ("client_id", &client_id),
            ("code_verifier", verifier),
            ("resource", &format!("{}/mcp", h.base)),
        ])
        .await;
    assert_eq!(status, 200, "{tokens}");
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["expires_in"], 3600);
    let access = tokens["access_token"].as_str().unwrap().to_string();
    let refresh = tokens["refresh_token"].as_str().unwrap().to_string();

    // The code is spent.
    let (status, body) = h
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", CALLBACK),
            ("client_id", &client_id),
            ("code_verifier", verifier),
        ])
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("invalid_grant"))
    );

    // 6. Refresh rotates: a new pair, and the old refresh token is dead.
    let (status, rotated) = h
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client_id),
        ])
        .await;
    assert_eq!(status, 200, "{rotated}");
    let new_refresh = rotated["refresh_token"].as_str().unwrap();
    assert_ne!(new_refresh, refresh);
    let (status, dead) = h
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client_id),
        ])
        .await;
    assert_eq!(
        (status, dead["error"].as_str()),
        (400, Some("invalid_grant"))
    );

    // Both access tokens still work until they expire; the rotated one is used.
    let rotated_access = rotated["access_token"].as_str().unwrap();
    assert_ne!(rotated_access, access);

    // 7. A proxied read reaches the stub upstream, stamped, without the gate's
    //    bearer token, and the session header comes back.
    let resp = h
        .rpc(
            rotated_access,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                   "params":{"name":"get_work_item","arguments":{"wi_number":1}}}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["mcp-session-id"], "stub-session");
    let body = resp.json_value().await;
    assert_eq!(body["result"]["content"][0]["text"], "called get_work_item");

    let seen = h.seen.0.lock().unwrap();
    let (headers, msg) = seen.last().expect("the read reached korg");
    assert_eq!(headers["x-homelab-agent"], "claude-duet");
    assert!(
        headers.get("authorization").is_none(),
        "the gate token leaked upstream"
    );
    assert_eq!(msg["params"]["name"], "get_work_item");
}

#[tokio::test]
async fn a_wrong_redirect_uri_is_refused_at_registration() {
    let h = Harness::start().await;
    for uris in [
        json!(["https://evil.example/cb"]),
        json!([CALLBACK, "https://evil.example/cb"]),
        json!(["https://claude.ai/api/mcp/auth_callback/"]),
        json!(["http://localhost:3118/callback"]),
        json!([]),
    ] {
        let resp = h
            .http
            .post(h.url("/register"))
            .header("content-type", "application/json")
            .body(json!({ "redirect_uris": uris }).to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "{uris}");
        let body = resp.json_value().await;
        assert_eq!(body["error"], "invalid_redirect_uri", "{uris}");
    }
}

#[tokio::test]
async fn authorize_never_redirects_to_an_unregistered_uri() {
    let h = Harness::start().await;
    let client_id = h.register().await;
    let resp = h
        .http
        .get(h.url("/authorize"))
        .query(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", "https://evil.example/cb"),
            ("code_challenge", "x"),
            ("code_challenge_method", "S256"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert!(resp.headers().get("location").is_none());

    // PKCE is mandatory, and its absence is reported back to Claude.
    let resp = h
        .http
        .get(h.url("/authorize"))
        .query(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", CALLBACK),
            ("code_challenge_method", "plain"),
            ("code_challenge", "x"),
        ])
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_redirection());
    assert!(resp.headers()["location"]
        .to_str()
        .unwrap()
        .contains("error=invalid_request"));
}

#[tokio::test]
async fn a_wrong_verifier_or_passphrase_gets_nothing() {
    let h = Harness::start().await;
    let client_id = h.register().await;
    let challenge = pkce_s256("the-right-verifier");

    let resp = h
        .sign_in(&client_id, &challenge, "not the passphrase at all")
        .await;
    assert_eq!(resp.status(), 401);
    assert!(resp.headers().get("location").is_none());

    let code = code_from(&h.sign_in(&client_id, &challenge, PASSPHRASE).await);
    let (status, body) = h
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", CALLBACK),
            ("client_id", &client_id),
            ("code_verifier", "the-wrong-verifier"),
        ])
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("invalid_grant"))
    );
}

#[tokio::test]
async fn repeated_wrong_passphrases_lock_out_even_the_right_one() {
    let h = Harness::start().await;
    let client_id = h.register().await;
    let challenge = pkce_s256("v");
    for _ in 0..6 {
        h.sign_in(&client_id, &challenge, "guess guess guess guess")
            .await;
    }
    let resp = h.sign_in(&client_id, &challenge, PASSPHRASE).await;
    assert_eq!(resp.status(), 429);
    assert!(resp.headers().get("location").is_none());
}

#[tokio::test]
async fn token_endpoint_takes_form_bodies_and_names_bad_grants() {
    let h = Harness::start().await;
    let (status, body) = h.token(&[("grant_type", "client_credentials")]).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("unsupported_grant_type"))
    );
    let (status, body) = h
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", "never-issued"),
        ])
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("invalid_grant"))
    );
}

#[tokio::test]
async fn clients_and_refresh_tokens_survive_a_gate_restart() {
    // The store-level test covers the mechanics; this checks the state lands
    // in the configured directory at all.
    let h = Harness::start().await;
    let _ = h.access_token().await;
    let raw = std::fs::read_to_string(h.state_dir.join("state.json")).unwrap();
    let state: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(state["clients"].as_object().unwrap().len(), 1);
    assert_eq!(state["refresh"].as_object().unwrap().len(), 1);
}
