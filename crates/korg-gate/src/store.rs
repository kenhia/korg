//! The gate's state: registered clients, grants, and the sign-in lockout.
//!
//! Two lifetimes, on purpose:
//!
//! - **Persisted** (a JSON file in `KORG_GATE_STATE_DIR`, written atomically):
//!   registered clients and refresh tokens. A restart then costs the phone
//!   nothing — its next call gets a 401, it refreshes, and carries on.
//! - **Memory only**: authorization codes (minutes), access tokens (an hour)
//!   and the lockout counter. Losing these on restart is harmless: a lost
//!   access token is refreshed, and a lost code means signing in again.
//!
//! Tokens are stored as SHA-256 digests, never raw, so the state file is not a
//! bag of live credentials if it ever leaks.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Registration is unauthenticated (that is what DCR is), so the client table
/// is bounded: claude.ai registers afresh on every new connection, and an
/// attacker could register forever.
pub const MAX_CLIENTS: usize = 64;
const CODE_TTL: Duration = Duration::from_secs(300);
/// Failures allowed before the lockout starts.
const FREE_FAILURES: u32 = 5;
const LOCKOUT_BASE: Duration = Duration::from_secs(30);
const LOCKOUT_CAP: Duration = Duration::from_secs(3600);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Client {
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
    pub issued_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RefreshGrant {
    client_id: String,
    scope: String,
    expires_at: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Persisted {
    clients: BTreeMap<String, Client>,
    /// Keyed by the SHA-256 hex digest of the refresh token.
    refresh: BTreeMap<String, RefreshGrant>,
}

struct PendingCode {
    client_id: String,
    redirect_uri: String,
    challenge: String,
    scope: String,
    expires: Instant,
}

struct AccessGrant {
    client_id: String,
    expires: Instant,
}

#[derive(Default)]
struct Lockout {
    failures: u32,
    until: Option<Instant>,
}

struct Inner {
    persisted: Persisted,
    codes: HashMap<String, PendingCode>,
    access: HashMap<String, AccessGrant>,
    lockout: Lockout,
}

pub struct Store {
    path: PathBuf,
    access_ttl: Duration,
    refresh_ttl: Duration,
    inner: Mutex<Inner>,
}

/// What a successful code exchange or refresh hands back.
#[derive(Debug)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub scope: String,
}

#[derive(Debug, PartialEq)]
pub enum RegisterError {
    /// Every slot is held by a client with a live refresh token.
    Full,
}

/// Why a token request failed. Every variant is `invalid_grant` on the wire
/// (RFC 6749 §5.2); the distinction is for the log.
#[derive(Debug, PartialEq)]
pub enum GrantError {
    UnknownCode,
    Expired,
    ClientMismatch,
    RedirectMismatch,
    PkceMismatch,
    UnknownRefresh,
}

/// The sign-in lockout's verdict on an attempt, decided *before* the
/// passphrase is looked at.
#[derive(Debug, PartialEq)]
pub enum Gate {
    Open,
    Locked { retry_after: Duration },
}

pub fn random_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).expect("the OS random source is available");
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn digest(token: &str) -> String {
    let hash = Sha256::digest(token.as_bytes());
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// RFC 7636 S256: `BASE64URL(SHA256(verifier)) == challenge`.
pub fn pkce_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Store {
    pub fn open(
        state_dir: &Path,
        access_ttl: Duration,
        refresh_ttl: Duration,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(state_dir)
            .with_context(|| format!("create state dir {}", state_dir.display()))?;
        let path = state_dir.join("state.json");
        let persisted = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("parse {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Persisted::default(),
            Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
        };
        Ok(Self {
            path,
            access_ttl,
            refresh_ttl,
            inner: Mutex::new(Inner {
                persisted,
                codes: HashMap::new(),
                access: HashMap::new(),
                lockout: Lockout::default(),
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves plain data behind, never a
        // half-written file (saves are atomic), so recovering is sound.
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Temp file in the same directory, then rename: a power cut leaves the old
    /// file or the new one, never a torn one.
    fn save(&self, persisted: &Persisted) -> anyhow::Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(persisted)?;
        std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("rename onto {}", self.path.display()))?;
        Ok(())
    }

    pub fn register(
        &self,
        client_name: Option<String>,
        redirect_uris: Vec<String>,
    ) -> anyhow::Result<Result<(String, Client), RegisterError>> {
        let mut inner = self.lock();
        let now = unix_now();
        inner.persisted.refresh.retain(|_, g| g.expires_at > now);
        if inner.persisted.clients.len() >= MAX_CLIENTS {
            // Evict the oldest client nobody holds a live grant for.
            let held: std::collections::HashSet<&String> = inner
                .persisted
                .refresh
                .values()
                .map(|g| &g.client_id)
                .collect();
            let victim = inner
                .persisted
                .clients
                .iter()
                .filter(|(id, _)| !held.contains(id))
                .min_by_key(|(_, c)| c.issued_at)
                .map(|(id, _)| id.clone());
            match victim {
                Some(id) => {
                    inner.persisted.clients.remove(&id);
                }
                None => return Ok(Err(RegisterError::Full)),
            }
        }
        let client_id = random_token();
        let client = Client {
            client_name,
            redirect_uris,
            issued_at: now,
        };
        inner
            .persisted
            .clients
            .insert(client_id.clone(), client.clone());
        self.save(&inner.persisted)?;
        Ok(Ok((client_id, client)))
    }

    pub fn client(&self, client_id: &str) -> Option<Client> {
        self.lock().persisted.clients.get(client_id).cloned()
    }

    pub fn client_count(&self) -> usize {
        self.lock().persisted.clients.len()
    }

    pub fn check_lockout(&self) -> Gate {
        let inner = self.lock();
        match inner.lockout.until {
            Some(until) if until > Instant::now() => Gate::Locked {
                retry_after: until - Instant::now(),
            },
            _ => Gate::Open,
        }
    }

    /// Record a failed sign-in; returns the lockout now in force, if any.
    pub fn sign_in_failed(&self) -> Option<Duration> {
        let mut inner = self.lock();
        inner.lockout.failures += 1;
        let over = inner.lockout.failures.saturating_sub(FREE_FAILURES);
        if over == 0 {
            return None;
        }
        let delay = LOCKOUT_BASE
            .saturating_mul(1u32 << (over - 1).min(16))
            .min(LOCKOUT_CAP);
        inner.lockout.until = Some(Instant::now() + delay);
        Some(delay)
    }

    pub fn sign_in_succeeded(&self) {
        self.lock().lockout = Lockout::default();
    }

    pub fn issue_code(
        &self,
        client_id: &str,
        redirect_uri: &str,
        challenge: &str,
        scope: &str,
    ) -> String {
        let code = random_token();
        let mut inner = self.lock();
        let now = Instant::now();
        inner.codes.retain(|_, c| c.expires > now);
        inner.codes.insert(
            digest(&code),
            PendingCode {
                client_id: client_id.to_string(),
                redirect_uri: redirect_uri.to_string(),
                challenge: challenge.to_string(),
                scope: scope.to_string(),
                expires: now + CODE_TTL,
            },
        );
        code
    }

    /// Exchange an authorization code. The code is consumed whether or not the
    /// exchange succeeds — a code is single-use, and a failed PKCE check is
    /// exactly the case where it must not get a second try.
    pub fn exchange_code(
        &self,
        code: &str,
        client_id: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> anyhow::Result<Result<Tokens, GrantError>> {
        let mut inner = self.lock();
        let Some(pending) = inner.codes.remove(&digest(code)) else {
            return Ok(Err(GrantError::UnknownCode));
        };
        if pending.expires <= Instant::now() {
            return Ok(Err(GrantError::Expired));
        }
        if pending.client_id != client_id {
            return Ok(Err(GrantError::ClientMismatch));
        }
        if pending.redirect_uri != redirect_uri {
            return Ok(Err(GrantError::RedirectMismatch));
        }
        let computed = pkce_s256(verifier);
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            computed.as_bytes(),
            pending.challenge.as_bytes(),
        )) {
            return Ok(Err(GrantError::PkceMismatch));
        }
        let tokens = self.mint(&mut inner, client_id, &pending.scope);
        self.save(&inner.persisted)?;
        Ok(Ok(tokens))
    }

    /// Rotate a refresh token: the old one dies in the same step that mints its
    /// replacement (OAuth 2.1 for public clients).
    pub fn refresh(
        &self,
        refresh_token: &str,
        client_id: Option<&str>,
    ) -> anyhow::Result<Result<Tokens, GrantError>> {
        let mut inner = self.lock();
        let key = digest(refresh_token);
        let Some(grant) = inner.persisted.refresh.get(&key).cloned() else {
            return Ok(Err(GrantError::UnknownRefresh));
        };
        if client_id.is_some_and(|c| c != grant.client_id) {
            return Ok(Err(GrantError::ClientMismatch));
        }
        inner.persisted.refresh.remove(&key);
        if grant.expires_at <= unix_now() {
            self.save(&inner.persisted)?;
            return Ok(Err(GrantError::Expired));
        }
        let tokens = self.mint(&mut inner, &grant.client_id, &grant.scope);
        self.save(&inner.persisted)?;
        Ok(Ok(tokens))
    }

    fn mint(&self, inner: &mut Inner, client_id: &str, scope: &str) -> Tokens {
        let access_token = random_token();
        let refresh_token = random_token();
        let now = Instant::now();
        inner.access.retain(|_, a| a.expires > now);
        inner.access.insert(
            digest(&access_token),
            AccessGrant {
                client_id: client_id.to_string(),
                expires: now + self.access_ttl,
            },
        );
        let unix = unix_now();
        inner.persisted.refresh.retain(|_, g| g.expires_at > unix);
        inner.persisted.refresh.insert(
            digest(&refresh_token),
            RefreshGrant {
                client_id: client_id.to_string(),
                scope: scope.to_string(),
                expires_at: unix + self.refresh_ttl.as_secs(),
            },
        );
        Tokens {
            access_token,
            refresh_token,
            expires_in: self.access_ttl.as_secs(),
            scope: scope.to_string(),
        }
    }

    /// The client an access token was issued to, if it is live.
    pub fn authenticate(&self, access_token: &str) -> Option<String> {
        let inner = self.lock();
        inner
            .access
            .get(&digest(access_token))
            .filter(|a| a.expires > Instant::now())
            .map(|a| a.client_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("korg-gate-{name}-{}", random_token()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn store(dir: &Path) -> Store {
        Store::open(dir, Duration::from_secs(60), Duration::from_secs(600)).unwrap()
    }

    #[test]
    fn pkce_is_unpadded_base64url_of_sha256() {
        // Expected value computed independently:
        // python3 -c "import hashlib,base64; print(base64.urlsafe_b64encode(
        //   hashlib.sha256(b'dBjftJeZ4CVP-mJ92K1-AlUQZsHBk3vSqyWjeuQ0MKc').digest()).rstrip(b'='))"
        assert_eq!(
            pkce_s256("dBjftJeZ4CVP-mJ92K1-AlUQZsHBk3vSqyWjeuQ0MKc"),
            "oeT35Xq3LcYsJnQ0t5Rw5z32FlJ-AlYLXw-D5KDJ8nE"
        );
    }

    #[test]
    fn clients_and_refresh_tokens_survive_a_restart_and_access_tokens_do_not() {
        let dir = temp_dir("restart");
        let s = store(&dir);
        let (id, _) = s.register(None, vec!["x".into()]).unwrap().unwrap();
        let code = s.issue_code(&id, "x", &pkce_s256("v"), "korg:read");
        let tokens = s.exchange_code(&code, &id, "x", "v").unwrap().unwrap();
        assert!(s.authenticate(&tokens.access_token).is_some());
        drop(s);

        let s = store(&dir);
        assert!(s.client(&id).is_some());
        assert!(s.authenticate(&tokens.access_token).is_none());
        let fresh = s
            .refresh(&tokens.refresh_token, Some(&id))
            .unwrap()
            .unwrap();
        assert!(s.authenticate(&fresh.access_token).is_some());
        // The state file holds digests, never the tokens themselves.
        let raw = std::fs::read_to_string(dir.join("state.json")).unwrap();
        assert!(!raw.contains(&fresh.refresh_token));
        assert!(raw.contains(&digest(&fresh.refresh_token)));
    }

    #[test]
    fn a_code_is_single_use_even_after_a_failed_pkce_check() {
        let dir = temp_dir("code");
        let s = store(&dir);
        let (id, _) = s.register(None, vec!["x".into()]).unwrap().unwrap();
        let code = s.issue_code(&id, "x", &pkce_s256("right"), "korg:read");
        assert_eq!(
            s.exchange_code(&code, &id, "x", "wrong")
                .unwrap()
                .unwrap_err(),
            GrantError::PkceMismatch
        );
        assert_eq!(
            s.exchange_code(&code, &id, "x", "right")
                .unwrap()
                .unwrap_err(),
            GrantError::UnknownCode
        );
    }

    #[test]
    fn the_client_table_is_bounded_and_spares_clients_holding_grants() {
        let dir = temp_dir("bound");
        let s = store(&dir);
        let (held, _) = s.register(None, vec!["x".into()]).unwrap().unwrap();
        let code = s.issue_code(&held, "x", &pkce_s256("v"), "korg:read");
        s.exchange_code(&code, &held, "x", "v").unwrap().unwrap();
        for _ in 0..MAX_CLIENTS * 2 {
            s.register(None, vec!["x".into()]).unwrap().unwrap();
        }
        assert_eq!(s.client_count(), MAX_CLIENTS);
        assert!(
            s.client(&held).is_some(),
            "a client with a live grant was evicted"
        );
    }

    #[test]
    fn lockout_starts_after_the_free_failures_and_grows() {
        let dir = temp_dir("lock");
        let s = store(&dir);
        for _ in 0..FREE_FAILURES {
            assert_eq!(s.sign_in_failed(), None);
        }
        let first = s.sign_in_failed().unwrap();
        let second = s.sign_in_failed().unwrap();
        assert!(second > first);
        assert!(matches!(s.check_lockout(), Gate::Locked { .. }));
        s.sign_in_succeeded();
        assert_eq!(s.check_lockout(), Gate::Open);
    }
}
