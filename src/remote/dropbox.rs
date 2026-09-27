use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use ureq::Agent;
use ureq::http::Response;

use super::{ManifestConflict, Remote, Rev};
use crate::credentials::{self, Secret};

/// Dropbox app key (a public identifier; PKCE needs no secret). The app must be created
/// with "App folder" access so grch only ever sees its own folder.
/// Overridable for development with GRCH_DROPBOX_APP_KEY.
const APP_KEY: &str = "";

const AUTH_URL: &str = "https://www.dropbox.com/oauth2/authorize";
const TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";
const API: &str = "https://api.dropboxapi.com/2";
const CONTENT: &str = "https://content.dropboxapi.com/2";

const MANIFEST_PATH: &str = "/manifest.7z";
const OBJECTS_DIR: &str = "/objects";

/// `files/upload` accepts at most 150 MB; larger files go through an upload session.
const SINGLE_UPLOAD_LIMIT: u64 = 150 * 1024 * 1024;
/// Session chunk size. Dropbox asks for a multiple of 4 MiB.
const CHUNK_SIZE: usize = 32 * 1024 * 1024;

const RETRIES: u32 = 3;

pub struct Dropbox {
    agent: Agent,
    app_key: String,
    refresh_token: String,
    access: Mutex<Option<(String, Instant)>>,
}

fn app_key() -> Result<String> {
    let key = std::env::var("GRCH_DROPBOX_APP_KEY").unwrap_or_else(|_| APP_KEY.to_string());
    if key.is_empty() {
        bail!("no Dropbox app key: set GRCH_DROPBOX_APP_KEY or build grch with one");
    }
    Ok(key)
}

fn agent() -> Agent {
    Agent::config_builder()
        // Dropbox reports errors as JSON bodies on 4xx / 5xx; read them instead of failing early.
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .build()
        .into()
}

fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        for c in &chars[..chunk.len() + 1] {
            out.push(TABLE[*c as usize] as char);
        }
    }
    out
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// OAuth 2.0 authorization code flow with PKCE. No redirect URI is registered, so Dropbox
/// shows the code on a page for the user to paste; that keeps grch free of a local HTTP
/// listener and works the same on every machine.
pub fn login() -> Result<()> {
    let app_key = app_key()?;
    let mut verifier_bytes = [0u8; 32];
    getrandom::fill(&mut verifier_bytes).context("generate PKCE verifier")?;
    let verifier = base64url(&verifier_bytes);
    let challenge = base64url(&Sha256::digest(verifier.as_bytes()));

    println!("Open this URL in a browser, allow access, then paste the code shown:");
    println!();
    println!(
        "  {AUTH_URL}?client_id={app_key}&response_type=code&token_access_type=offline&code_challenge_method=S256&code_challenge={challenge}"
    );
    println!();
    print!("Authorization code: ");
    io::stdout().flush()?;
    let mut code = String::new();
    io::stdin().read_line(&mut code)?;
    let code = code.trim();
    if code.is_empty() {
        bail!("no code entered");
    }

    let mut res = agent()
        .post(TOKEN_URL)
        .send_form([
            ("code", code),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
            ("client_id", app_key.as_str()),
        ])
        .context("request token")?;
    if !res.status().is_success() {
        let body = res.body_mut().read_to_string().unwrap_or_default();
        bail!("token request failed ({}): {}", res.status(), body);
    }
    let token: TokenResponse = read_json(&mut res)?;
    let Some(refresh) = token.refresh_token else {
        bail!("Dropbox returned no refresh token");
    };
    credentials::set(Secret::DropboxRefreshToken, &refresh)?;
    Ok(())
}

pub fn logout() -> Result<()> {
    credentials::delete(Secret::DropboxRefreshToken)
}

#[derive(Deserialize)]
struct ApiError {
    error_summary: String,
}

#[derive(Deserialize)]
struct FileMetadata {
    rev: String,
}

#[derive(Deserialize)]
struct SessionStart {
    session_id: String,
}

#[derive(Deserialize)]
struct Account {
    email: String,
    name: AccountName,
}

#[derive(Deserialize)]
struct AccountName {
    display_name: String,
}

#[derive(Deserialize)]
struct SpaceUsage {
    used: u64,
    allocation: Allocation,
}

#[derive(Deserialize)]
struct Allocation {
    #[serde(default)]
    allocated: u64,
}

enum Body<'a> {
    Empty,
    Bytes(&'a [u8]),
    File(&'a File),
}

impl Dropbox {
    pub fn new(refresh_token: String) -> Result<Self> {
        Ok(Dropbox {
            agent: agent(),
            app_key: app_key()?,
            refresh_token,
            access: Mutex::new(None),
        })
    }

    fn access_token(&self, force_refresh: bool) -> Result<String> {
        let mut cached = self.access.lock().unwrap();
        if !force_refresh
            && let Some((token, expires)) = cached.as_ref()
            && Instant::now() < *expires
        {
            return Ok(token.clone());
        }
        let mut res = self
            .agent
            .post(TOKEN_URL)
            .send_form([
                ("grant_type", "refresh_token"),
                ("refresh_token", self.refresh_token.as_str()),
                ("client_id", self.app_key.as_str()),
            ])
            .context("refresh access token")?;
        if !res.status().is_success() {
            let body = res.body_mut().read_to_string().unwrap_or_default();
            bail!(
                "could not refresh the Dropbox token ({}): {}. Run `grch remote login` again.",
                res.status(),
                body
            );
        }
        let token: TokenResponse = read_json(&mut res)?;
        // Refresh a minute early so a long upload does not start with a token about to expire.
        let expires = Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(60));
        *cached = Some((token.access_token.clone(), expires));
        Ok(token.access_token)
    }

    /// Send one request with auth, retrying on 401 (token refresh), 429 and 5xx.
    /// Returns the response for the caller to interpret; 4xx other than 401 / 429 are
    /// returned as-is because Dropbox encodes the specific error in the body.
    fn send(
        &self,
        url: &str,
        api_arg: Option<&str>,
        content_type: &str,
        body: Body<'_>,
    ) -> Result<Response<ureq::Body>> {
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            let token = self.access_token(refreshed)?;
            let mut req = self
                .agent
                .post(url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", content_type);
            if let Some(arg) = api_arg {
                req = req.header("Dropbox-API-Arg", arg);
            }
            let result = match body {
                Body::Empty => req.send_empty(),
                Body::Bytes(bytes) => req.send(bytes),
                Body::File(file) => {
                    // A retried upload must start from the beginning of the file.
                    let mut f = file;
                    f.seek_start()?;
                    req.send(file)
                }
            };
            match result {
                Ok(res) if res.status() == 401 && !refreshed => {
                    refreshed = true;
                    continue;
                }
                Ok(res) if res.status() == 429 || res.status().is_server_error() => {
                    if attempt >= RETRIES {
                        return Ok(res);
                    }
                    let retry_after = res
                        .headers()
                        .get("Retry-After")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1 << attempt);
                    std::thread::sleep(Duration::from_secs(retry_after));
                    attempt += 1;
                }
                Ok(res) => return Ok(res),
                Err(e) if attempt < RETRIES => {
                    eprintln!("retrying after network error: {e}");
                    std::thread::sleep(Duration::from_secs(1 << attempt));
                    attempt += 1;
                }
                Err(e) => return Err(e).context("dropbox request"),
            }
        }
    }

    fn rpc(&self, endpoint: &str, arg: &serde_json::Value) -> Result<Response<ureq::Body>> {
        let body = serde_json::to_vec(arg)?;
        self.send(
            &format!("{API}/{endpoint}"),
            None,
            "application/json",
            Body::Bytes(&body),
        )
    }

    fn content(
        &self,
        endpoint: &str,
        arg: &serde_json::Value,
        body: Body<'_>,
    ) -> Result<Response<ureq::Body>> {
        let arg = serde_json::to_string(arg)?;
        self.send(
            &format!("{CONTENT}/{endpoint}"),
            Some(&arg),
            "application/octet-stream",
            body,
        )
    }

    fn upload_session(&self, file: &mut File, size: u64, path: &str) -> Result<()> {
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut offset: u64 = 0;
        let mut session_id: Option<String> = None;
        loop {
            let n = read_full(file, &mut buf)?;
            let chunk = &buf[..n];
            let last = offset + n as u64 >= size;
            let endpoint;
            let arg;
            match (&session_id, last) {
                (None, _) => {
                    endpoint = "files/upload_session/start";
                    arg = serde_json::json!({ "close": last });
                }
                (Some(id), false) => {
                    endpoint = "files/upload_session/append_v2";
                    arg = serde_json::json!({ "cursor": { "session_id": id, "offset": offset } });
                }
                (Some(id), true) => {
                    endpoint = "files/upload_session/finish";
                    arg = serde_json::json!({
                        "cursor": { "session_id": id, "offset": offset },
                        "commit": { "path": path, "mode": "overwrite" },
                    });
                }
            }
            let mut res = self.content(endpoint, &arg, Body::Bytes(chunk))?;
            check_ok(&mut res, endpoint)?;
            if session_id.is_none() {
                let start: SessionStart = read_json(&mut res)?;
                if last {
                    // Whole file fit in the first chunk: finish with an empty append.
                    let arg = serde_json::json!({
                        "cursor": { "session_id": start.session_id, "offset": n },
                        "commit": { "path": path, "mode": "overwrite" },
                    });
                    let mut res =
                        self.content("files/upload_session/finish", &arg, Body::Bytes(&[]))?;
                    check_ok(&mut res, "files/upload_session/finish")?;
                    return Ok(());
                }
                session_id = Some(start.session_id);
            }
            offset += n as u64;
            if last {
                return Ok(());
            }
        }
    }
}

trait SeekStart {
    fn seek_start(&mut self) -> io::Result<()>;
}

impl SeekStart for &File {
    fn seek_start(&mut self) -> io::Result<()> {
        use std::io::Seek;
        (*self).seek(io::SeekFrom::Start(0)).map(|_| ())
    }
}

fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn read_json<T: serde::de::DeserializeOwned>(res: &mut Response<ureq::Body>) -> Result<T> {
    let text = res.body_mut().read_to_string()?;
    serde_json::from_str(&text).with_context(|| format!("parse response: {text}"))
}

fn error_summary(res: &mut Response<ureq::Body>) -> String {
    let text = res.body_mut().read_to_string().unwrap_or_default();
    serde_json::from_str::<ApiError>(&text)
        .map(|e| e.error_summary)
        .unwrap_or(text)
}

fn check_ok(res: &mut Response<ureq::Body>, what: &str) -> Result<()> {
    if res.status().is_success() {
        return Ok(());
    }
    let status = res.status();
    bail!("{what} failed ({status}): {}", error_summary(res));
}

fn object_path(object: &str) -> String {
    format!("{OBJECTS_DIR}/{object}.7z")
}

impl Remote for Dropbox {
    fn describe(&self) -> Result<String> {
        let mut res = self.rpc("users/get_current_account", &serde_json::Value::Null)?;
        check_ok(&mut res, "users/get_current_account")?;
        let account: Account = read_json(&mut res)?;
        let mut res = self.rpc("users/get_space_usage", &serde_json::Value::Null)?;
        check_ok(&mut res, "users/get_space_usage")?;
        let usage: SpaceUsage = read_json(&mut res)?;
        Ok(format!(
            "Dropbox app folder, account {} <{}>, {:.1} GB of {:.1} GB used",
            account.name.display_name,
            account.email,
            usage.used as f64 / 1e9,
            usage.allocation.allocated as f64 / 1e9
        ))
    }

    fn get_manifest(&self) -> Result<Option<(Vec<u8>, Rev)>> {
        let mut res = self.content(
            "files/download",
            &serde_json::json!({ "path": MANIFEST_PATH }),
            Body::Empty,
        )?;
        if res.status() == 409 {
            let summary = error_summary(&mut res);
            if summary.contains("not_found") {
                return Ok(None);
            }
            bail!("files/download failed: {summary}");
        }
        check_ok(&mut res, "files/download")?;
        let meta = res
            .headers()
            .get("Dropbox-API-Result")
            .and_then(|v| v.to_str().ok())
            .context("missing Dropbox-API-Result header")?;
        let meta: FileMetadata = serde_json::from_str(meta).context("parse file metadata")?;
        let bytes = res.body_mut().read_to_vec()?;
        Ok(Some((bytes, meta.rev)))
    }

    fn put_manifest(&self, bytes: &[u8], expect: Option<&Rev>) -> Result<Rev> {
        let mode = match expect {
            Some(rev) => serde_json::json!({ ".tag": "update", "update": rev }),
            None => serde_json::json!("add"),
        };
        let arg = serde_json::json!({ "path": MANIFEST_PATH, "mode": mode, "autorename": false });
        let mut res = self.content("files/upload", &arg, Body::Bytes(bytes))?;
        if res.status() == 409 {
            let summary = error_summary(&mut res);
            if summary.contains("conflict") {
                return Err(ManifestConflict.into());
            }
            bail!("files/upload failed: {summary}");
        }
        check_ok(&mut res, "files/upload")?;
        let meta: FileMetadata = read_json(&mut res)?;
        Ok(meta.rev)
    }

    fn upload(&self, object: &str, file: &Path) -> Result<()> {
        let mut f = File::open(file).with_context(|| format!("open {}", file.display()))?;
        let size = f.metadata()?.len();
        let path = object_path(object);
        if size > SINGLE_UPLOAD_LIMIT {
            return self.upload_session(&mut f, size, &path);
        }
        let arg = serde_json::json!({ "path": path, "mode": "overwrite" });
        let mut res = self.content("files/upload", &arg, Body::File(&f))?;
        check_ok(&mut res, "files/upload")
    }

    fn download(&self, object: &str, dest: &Path) -> Result<()> {
        let arg = serde_json::json!({ "path": object_path(object) });
        let mut res = self.content("files/download", &arg, Body::Empty)?;
        check_ok(&mut res, "files/download")?;
        let mut out = File::create(dest).with_context(|| format!("create {}", dest.display()))?;
        io::copy(&mut res.body_mut().as_reader(), &mut out)?;
        Ok(())
    }

    fn delete(&self, object: &str) -> Result<()> {
        let arg = serde_json::json!({ "path": object_path(object) });
        let mut res = self.rpc("files/delete_v2", &arg)?;
        if res.status() == 409 && error_summary(&mut res).contains("not_found") {
            return Ok(());
        }
        check_ok(&mut res, "files/delete_v2")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_matches_rfc4648_vectors() {
        // act & assert
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(&[0xfb, 0xff, 0xfe]), "-__-");
    }

    #[test]
    fn pkce_verifier_is_43_chars() {
        // arrange
        let bytes = [0u8; 32];

        // act & assert
        assert_eq!(base64url(&bytes).len(), 43);
    }

    #[test]
    fn read_full_fills_buffer_across_short_reads() {
        // arrange
        struct Short<'a>(&'a [u8]);
        impl Read for Short<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                let n = self.0.len().min(buf.len()).min(3);
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let mut reader = Short(b"0123456789");
        let mut buf = [0u8; 8];

        // act
        let first = read_full(&mut reader, &mut buf).unwrap();
        let first_bytes = buf[..first].to_vec();
        let second = read_full(&mut reader, &mut buf).unwrap();

        // assert
        assert_eq!(first, 8);
        assert_eq!(first_bytes, b"01234567");
        assert_eq!(second, 2);
        assert_eq!(&buf[..2], b"89");
    }

    #[test]
    fn object_path_is_under_objects_with_extension() {
        // act & assert
        assert_eq!(object_path("abc"), "/objects/abc.7z");
    }
}
