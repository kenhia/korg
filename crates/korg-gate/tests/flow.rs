//! The whole OAuth flow, end to end, the way claude.ai drives it — and the
//! proxied read it buys (WI 4147's code-half Done list). Every test here runs
//! with the klams door open too (sprint 094), so the korg-door tests are also
//! the proof that adding it changed nothing at `/mcp`.

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
    assert_eq!(
        prm["resource_name"], "korg",
        "sprint 095: no longer read-only"
    );
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
    // Sprint 095: korg's door takes writes, so its page no longer says
    // read-only.
    assert!(html.contains("wants access to korg's reads and the ruled write set."));
    assert!(html.contains(">Allow access</button>"));
    assert!(!html.contains("read-only"));

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

// ---- Sprint 094: `server/discover`, and the klams door ----

#[tokio::test]
async fn server_discover_is_forwarded_and_the_upstream_answer_stands() {
    // WI 4243, option (a): the gate admits MCP 2026-07-28's mandatory probe and
    // lets each upstream answer it, so it tracks the upstream's MCP era rather
    // than claiming one. Both upstreams run rmcp 3.1.2, which implements
    // 2026-07-28: a probe without `Mcp-Method` gets its header error, and one
    // with it gets the real answer — the gate adds and removes nothing.
    assert!(korg_gate::allowlist::method_allowed("server/discover"));
    let h = Harness::start().await;
    let (_, token, _) = h.access_token().await;
    let probe = json!({"jsonrpc":"2.0","id":"d1","method":"server/discover","params":{}});

    let resp = h.rpc(&token, probe.clone()).await;
    assert_eq!(resp.json_value().await, missing_mcp_method(json!("d1")));
    let body = h
        .rpc_with(
            "/mcp",
            &token,
            probe.clone(),
            &[("mcp-method", "server/discover")],
        )
        .await
        .json_value()
        .await;
    assert_eq!(
        body["result"]["supportedVersions"],
        json!(SUPPORTED_VERSIONS)
    );
    {
        let seen = h.seen.0.lock().unwrap();
        assert_eq!(seen.len(), 2, "both probes reached korg");
        let (headers, msg) = seen.last().unwrap();
        assert_eq!(msg, &probe);
        assert_eq!(headers["mcp-method"], "server/discover");
        assert_eq!(headers["x-homelab-agent"], "claude-duet");
    }

    // Same rule on the klams door.
    let (_, token, _) = h.klams_token().await;
    let text = h
        .rpc_with(
            "/klams/mcp",
            &token,
            probe.clone(),
            &[("mcp-method", "server/discover")],
        )
        .await
        .text()
        .await
        .unwrap();
    assert!(text.contains("supportedVersions"), "{text}");
    assert_eq!(h.klams_seen.0.lock().unwrap().last().unwrap().1, probe);
}

#[tokio::test]
async fn routing_headers_are_the_gates_own_and_must_agree_with_the_body() {
    // 2026-07-28 routes by `Mcp-Method` / `Mcp-Name` headers. Forwarding the
    // caller's would let the header and the checked body disagree, so the
    // gate refuses a mismatch itself and forwards values derived from the body.
    let h = Harness::start().await;
    let (_, token, _) = h.access_token().await;
    let call = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                      "params":{"name":"get_item","arguments":{}}});
    for (extra, why) in [
        (
            vec![("mcp-method", "tools/list")],
            "Mcp-Method header `tools/list` does not match body method `tools/call`",
        ),
        (
            vec![("mcp-method", "tools/call"), ("mcp-name", "delete_area")],
            "Mcp-Name header `delete_area` does not match body value `get_item`",
        ),
    ] {
        let resp = h.rpc_with("/mcp", &token, call.clone(), &extra).await;
        assert_eq!(resp.status(), 400);
        let body = resp.json_value().await;
        assert_eq!(body["id"], 1);
        assert_eq!(body["error"]["code"], -32020);
        assert_eq!(body["error"]["message"], why);
    }
    assert!(
        h.seen.0.lock().unwrap().is_empty(),
        "a mismatch reached korg"
    );

    // Agreeing headers pass, as the gate's own values.
    let resp = h
        .rpc_with(
            "/mcp",
            &token,
            call.clone(),
            &[("mcp-method", "tools/call"), ("mcp-name", "get_item")],
        )
        .await;
    assert_eq!(resp.status(), 200);
    // And a 2025-era call carries neither, exactly as before.
    h.rpc(&token, call).await;
    let seen = h.seen.0.lock().unwrap();
    assert_eq!(seen[0].0["mcp-method"], "tools/call");
    assert_eq!(seen[0].0["mcp-name"], "get_item");
    assert!(seen[1].0.get("mcp-method").is_none());
    assert!(seen[1].0.get("mcp-name").is_none());
}

#[tokio::test]
async fn klams_door_full_flow_then_a_read_reaches_klams_stamped() {
    let h = Harness::start().await;
    let resource = h.klams_resource();

    // 1. The 401 points at the klams door's own metadata, with its scope.
    let resp = h
        .http
        .post(h.url("/klams/mcp"))
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    assert_eq!(
        resp.headers()["www-authenticate"].to_str().unwrap(),
        format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/klams/mcp\", \
             scope=\"klams:read\"",
            h.base
        )
    );

    // 2. Its metadata: its own resource, the same single issuer.
    let prm: Value = h
        .http
        .get(h.url("/.well-known/oauth-protected-resource/klams/mcp"))
        .send()
        .await
        .unwrap()
        .json_value()
        .await;
    assert_eq!(prm["resource"], resource);
    assert_eq!(prm["authorization_servers"], json!([h.base]));
    assert_eq!(prm["scopes_supported"], json!(["klams:read"]));
    assert_eq!(prm["resource_name"], "klams (read-only)");

    // 3. The sign-in page names klams and carries the resource to the POST.
    let client_id = h.register().await;
    let verifier = "dBjftJeZ4CVP-mJ92K1-AlUQZsHBk3vSqyWjeuQ0MKc";
    let challenge = pkce_s256(verifier);
    let html = h
        .http
        .get(h.url("/authorize"))
        .query(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", CALLBACK),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", "st-123"),
            ("scope", "klams:read offline_access"),
            ("resource", &resource),
        ])
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("wants <strong>read-only</strong> access to klams."));
    assert!(html.contains(">Allow read access</button>"));
    assert!(html.contains(&format!(
        "<input type=\"hidden\" name=\"resource\" value=\"{resource}\">"
    )));

    // 4. Sign in and exchange, both naming the klams resource.
    let code = code_from(
        &h.sign_in_with(
            &client_id,
            &challenge,
            PASSPHRASE,
            &[("resource", &resource)],
        )
        .await,
    );
    let (status, tokens) = h
        .token(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", CALLBACK),
            ("client_id", &client_id),
            ("code_verifier", verifier),
            ("resource", &resource),
        ])
        .await;
    assert_eq!(status, 200, "{tokens}");
    assert_eq!(tokens["scope"], "klams:read");
    let access = tokens["access_token"].as_str().unwrap();

    // 5. A read reaches klams, stamped, without the gate's token; klams's SSE
    //    answer and session header come back.
    let resp = h
        .rpc_at(
            "/klams/mcp",
            access,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                   "params":{"name":"memory_search","arguments":{"query":"kaed"}}}),
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["mcp-session-id"], "klams-session");
    assert!(resp.text().await.unwrap().contains("called memory_search"));
    {
        let seen = h.klams_seen.0.lock().unwrap();
        let (headers, msg) = seen.last().expect("the read reached klams");
        assert_eq!(headers["x-homelab-agent"], "claude-duet");
        assert!(headers.get("authorization").is_none());
        assert_eq!(msg["params"]["name"], "memory_search");
    }
    assert!(
        h.seen.0.lock().unwrap().is_empty(),
        "klams traffic reached korg"
    );
}

#[tokio::test]
async fn a_token_opens_only_the_door_it_was_issued_for() {
    let h = Harness::start().await;
    let (_, korg, _) = h.access_token().await;
    let (_, klams, _) = h.klams_token().await;
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});

    let resp = h.rpc_at("/klams/mcp", &korg, list.clone()).await;
    assert_eq!(resp.status(), 401, "a korg token opened the klams door");
    assert!(resp.headers()["www-authenticate"]
        .to_str()
        .unwrap()
        .contains("/.well-known/oauth-protected-resource/klams/mcp"));
    let resp = h.rpc(&klams, list.clone()).await;
    assert_eq!(resp.status(), 401, "a klams token opened the korg door");
    assert_eq!(
        resp.headers()["www-authenticate"].to_str().unwrap(),
        format!(
            "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
            h.base
        )
    );
    for method in ["GET", "DELETE"] {
        let resp = h
            .http
            .request(method.parse().unwrap(), h.url("/klams/mcp"))
            .bearer_auth(&korg)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "{method}");
    }
    assert!(h.seen.0.lock().unwrap().is_empty());
    assert!(h.klams_seen.0.lock().unwrap().is_empty());

    // Each still works at its own door.
    assert_eq!(h.rpc(&korg, list.clone()).await.status(), 200);
    assert_eq!(h.rpc_at("/klams/mcp", &klams, list).await.status(), 200);
}

#[tokio::test]
async fn refresh_keeps_the_door_and_refuses_another_resource_without_spending_the_token() {
    let h = Harness::start().await;
    let (client_id, _, refresh) = h.klams_token().await;
    let korg_resource = format!("{}/mcp", h.base);

    let (status, body) = h
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client_id),
            ("resource", &korg_resource),
        ])
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (400, Some("invalid_target"))
    );

    // Still live: no resource named, and the grant's own door is kept.
    let (status, rotated) = h
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client_id),
        ])
        .await;
    assert_eq!(status, 200, "{rotated}");
    assert_eq!(rotated["scope"], "klams:read");
    let access = rotated["access_token"].as_str().unwrap();
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    assert_eq!(
        h.rpc_at("/klams/mcp", access, list.clone()).await.status(),
        200
    );
    assert_eq!(h.rpc(access, list).await.status(), 401);
}

#[tokio::test]
async fn a_sign_in_without_a_resource_takes_its_door_from_the_scope() {
    let h = Harness::start().await;
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    // A client that read the klams door's metadata but omits RFC 8707.
    let (_, token, _) = h
        .access_token_with(&[("scope", "klams:read offline_access")])
        .await;
    assert_eq!(
        h.rpc_at("/klams/mcp", &token, list.clone()).await.status(),
        200
    );
    // Asking for both is korg's, as is asking for nothing (the pre-094 rule).
    for scope in ["korg:read klams:read", ""] {
        let (_, token, _) = h.access_token_with(&[("scope", scope)]).await;
        assert_eq!(h.rpc(&token, list.clone()).await.status(), 200, "{scope:?}");
    }
}

#[tokio::test]
async fn without_a_klams_upstream_the_klams_door_does_not_exist() {
    let h = Harness::start_with(Options {
        klams: false,
        state_dir: None,
    })
    .await;
    for path in [
        "/klams/mcp",
        "/.well-known/oauth-protected-resource/klams/mcp",
    ] {
        let resp = h.http.get(h.url(path)).send().await.unwrap();
        assert_eq!(resp.status(), 404, "{path}");
    }
    let client_id = h.register().await;
    let resp = h
        .http
        .get(h.url("/authorize"))
        .query(&[
            ("response_type", "code"),
            ("client_id", &client_id),
            ("redirect_uri", CALLBACK),
            ("code_challenge", "x"),
            ("code_challenge_method", "S256"),
            ("resource", &h.klams_resource()),
        ])
        .send()
        .await
        .unwrap();
    assert!(resp.headers()["location"]
        .to_str()
        .unwrap()
        .contains("error=invalid_target"));
    // A scope alone cannot open a door that is not there.
    let (_, token, _) = h.access_token_with(&[("scope", "klams:read")]).await;
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    assert_eq!(h.rpc(&token, list).await.status(), 200);
}

#[tokio::test]
async fn a_refresh_token_from_the_pre_094_gate_keeps_working_at_the_korg_door() {
    // The state file exactly as the one-door gate wrote it (no `door` field):
    // the phone's existing connector must refresh and read as before.
    let dir = temp_state_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let refresh = "a-refresh-token-the-one-door-gate-issued";
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let state = json!({
        "clients": { "old-client": { "client_name": "Claude",
                                     "redirect_uris": [CALLBACK], "issued_at": now - 86400 } },
        "refresh": { korg_gate::store::digest(refresh):
                     { "client_id": "old-client", "scope": "korg:read", "expires_at": now + 86400 } },
    });
    std::fs::write(dir.join("state.json"), state.to_string()).unwrap();
    let h = Harness::start_with(Options {
        klams: true,
        state_dir: Some(dir),
    })
    .await;

    let (status, body) = h
        .token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", "old-client"),
        ])
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["scope"], "korg:read");
    assert_eq!(body["expires_in"], 3600);
    let access = body["access_token"].as_str().unwrap();
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    let listed = h.rpc(access, list.clone()).await.json_value().await;
    assert_eq!(
        listed["result"]["tools"].as_array().unwrap().len(),
        korg_gate::allowlist::READ_TOOLS.len() + korg_gate::allowlist::WRITE_TOOLS.len()
    );
    assert_eq!(h.rpc_at("/klams/mcp", access, list).await.status(), 401);
}
