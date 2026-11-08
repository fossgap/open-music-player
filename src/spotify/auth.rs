//! Browser OAuth with PKCE. Only refresh tokens go into the desktop secret store.
use crate::{model::stable_id, storage};
use oauth2::{CsrfToken, PkceCodeChallenge};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const REDIRECT: &str = "http://127.0.0.1:5588/login";
const SCOPES: &str = "streaming user-read-private playlist-read-private playlist-read-collaborative playlist-modify-private playlist-modify-public";

#[derive(Serialize, Deserialize)]
pub struct Stored {
    pub refresh_token: String,
    pub client_id: String,
    pub account_id: String,
}

#[derive(Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default = "hour")]
    pub expires_in: u64,
}
fn hour() -> u64 {
    3600
}

#[derive(Clone)]
pub struct Job {
    pub epoch: u64,
    pub current: Arc<AtomicU64>,
}
impl Job {
    pub fn check(&self) -> Result<(), String> {
        if self.current.load(Ordering::SeqCst) == self.epoch {
            Ok(())
        } else {
            Err("Spotify request cancelled.".into())
        }
    }
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        "io.github.openmusic.Player.spotify",
        &stable_id(&storage::data_dir().to_string_lossy()),
    )
    .map_err(|_| {
        "Could not open the desktop keyring. Start and unlock a Secret Service keyring.".into()
    })
}
pub fn read() -> Result<Stored, String> {
    let value = entry()?.get_password().map_err(|_| "Sign in to Spotify again. Saved credentials are missing or the desktop keyring is locked.")?;
    serde_json::from_str(&value)
        .map_err(|_| "Saved Spotify credentials are invalid. Sign in again.".into())
}
pub fn save(stored: &Stored) -> Result<(), String> {
    entry()?
        .set_password(
            &serde_json::to_string(stored).map_err(|_| "Could not save Spotify credentials.")?,
        )
        .map_err(|_| {
            "Could not save Spotify sign-in. Unlock your desktop keyring and try again.".into()
        })
}
pub fn delete() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Could not remove Spotify credentials. Unlock your desktop keyring and disconnect again.".into()),
    }
}

pub fn refresh(http: &Client, stored: &Stored) -> Result<Token, String> {
    token(
        http,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &stored.refresh_token),
            ("client_id", &stored.client_id),
        ],
    )
}
fn token(http: &Client, form: &[(&str, &str)]) -> Result<Token, String> {
    let response = http
        .post("https://accounts.spotify.com/api/token")
        .form(form)
        .send()
        .map_err(|_| "Could not reach Spotify sign-in. Check your connection and try again.")?;
    if !response.status().is_success() {
        return Err("Spotify rejected the sign-in. Connect your account again.".into());
    }
    let token: Token = response
        .json()
        .map_err(|_| "Invalid response from Spotify sign-in.")?;
    if token.access_token.is_empty() {
        return Err("Spotify returned an empty access token.".into());
    }
    Ok(token)
}

/// Ignore unrelated/forged callbacks; never accept a code without the matching state.
fn callback(target: &str, expected: &str) -> Result<Option<String>, String> {
    if !target.starts_with("/login?") {
        return Ok(None);
    }
    let url = url::Url::parse(&format!("http://127.0.0.1:5588{target}"))
        .map_err(|_| "Invalid Spotify callback.")?;
    let pairs: Vec<_> = url.query_pairs().collect();
    let values = |name: &str| {
        pairs
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_ref())
            .collect::<Vec<_>>()
    };
    if values("state") != [expected] {
        return Ok(None);
    }
    if !values("error").is_empty() {
        return Err("Spotify sign-in was declined. You can try again.".into());
    }
    let codes = values("code");
    if codes.len() != 1 || codes[0].is_empty() {
        return Err("Spotify did not return an authorization code.".into());
    }
    Ok(Some(codes[0].to_string()))
}

pub fn login(
    http: &Client,
    client_id: &str,
    job: &Job,
    open: impl Fn(String),
) -> Result<Token, String> {
    let listener = TcpListener::bind("127.0.0.1:5588").map_err(
        |_| "Spotify sign-in port 5588 is in use. Close the other sign-in window and try again.",
    )?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let state = CsrfToken::new_random();
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut authorize = url::Url::parse("https://accounts.spotify.com/authorize").unwrap();
    authorize.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT),
        ("scope", SCOPES),
        ("state", state.secret()),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge.as_str()),
    ]);
    open(authorize.to_string());
    let deadline = Instant::now() + Duration::from_secs(180);
    let code = loop {
        job.check()?;
        if Instant::now() >= deadline {
            return Err("Spotify sign-in timed out. Choose Connect to try again.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut bytes = Vec::new();
                let mut chunk = [0; 1024];
                while bytes.len() < 8192 && !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let mut words = request.lines().next().unwrap_or("").split_whitespace();
                let answer = if words.next() == Some("GET") {
                    callback(words.next().unwrap_or(""), state.secret())
                } else {
                    Ok(None)
                };
                let valid = !matches!(&answer, Ok(None));
                let body = if valid {
                    "Sign-in received. Return to Open Music Player to finish connecting."
                } else {
                    "This is not a valid Open Music Player sign-in callback."
                };
                let status = if valid { "200 OK" } else { "400 Bad Request" };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                if let Some(code) = answer? {
                    break code;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(80))
            }
            Err(_) => return Err("Could not receive Spotify sign-in. Try connecting again.".into()),
        }
    };
    job.check()?;
    token(
        http,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT),
            ("client_id", client_id),
            ("code_verifier", verifier.secret()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callbacks_require_matching_unique_state_and_code() {
        assert_eq!(
            callback("/login?state=abc&code=123", "abc").unwrap(),
            Some("123".into())
        );
        for target in [
            "/login?code=123",
            "/login?state=wrong&code=123",
            "/login?state=abc&state=wrong&code=123",
            "/favicon.ico",
        ] {
            assert_eq!(callback(target, "abc").unwrap(), None);
        }
        assert!(callback("/login?state=abc&error=access_denied", "abc").is_err());
        assert!(callback("/login?state=abc&code=1&code=2", "abc").is_err());
    }
}
