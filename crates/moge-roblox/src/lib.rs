//! Roblox web API logic without any networking: requests are plain data and responses are parsed here,
//! so everything is testable offline. The app supplies an `Http` implementation.
use moge_core::json::{self, Json};

#[derive(Clone, Debug, Default)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

pub trait Http {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, String>;
}

fn get(url: String) -> HttpRequest {
    HttpRequest { method: "GET", url, ..Default::default() }
}

fn bearer(mut r: HttpRequest, token: &str) -> HttpRequest {
    r.headers.push(("Authorization".into(), format!("Bearer {token}")));
    r
}

pub fn percent_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn form(pairs: &[(&str, &str)]) -> Vec<u8> {
    pairs.iter().map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v))).collect::<Vec<_>>().join("&").into_bytes()
}

fn json_of(r: &HttpResponse) -> Result<Json, String> {
    json::parse(&String::from_utf8_lossy(&r.body)).map_err(|e| format!("Unexpected response from Roblox (HTTP {}): {e}", r.status))
}

/// First error message in Roblox's `{"errors":[{"message":...}]}` / `{"message":...}` envelopes.
fn api_error(r: &HttpResponse) -> String {
    let fallback = format!("Roblox returned HTTP {}", r.status);
    let Ok(j) = json::parse(&String::from_utf8_lossy(&r.body)) else { return fallback };
    let msg = j
        .get("errors")
        .and_then(|e| e.as_arr())
        .and_then(|e| e.first())
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .or_else(|| j.get("message").and_then(|m| m.as_str()))
        .or_else(|| j.get("error_description").and_then(|m| m.as_str()))
        .or_else(|| j.get("error").and_then(|m| m.as_str()));
    match msg {
        Some(m) => format!("{fallback}: {m}"),
        None => fallback,
    }
}

// ------------------------------------------------------------------ SHA-256 and PKCE

pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend(((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn base64url(data: &[u8]) -> String {
    moge_core::b64::encode(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// PKCE code verifier from caller-supplied randomness (at least 32 bytes).
pub fn pkce_verifier(random: &[u8]) -> Result<String, String> {
    if random.len() < 32 {
        return Err("PKCE needs at least 32 random bytes".into());
    }
    Ok(base64url(&random[..48.min(random.len())]))
}

pub fn pkce_challenge(verifier: &str) -> String {
    base64url(&sha256(verifier.as_bytes()))
}

// ------------------------------------------------------------------ OAuth 2.0 (Roblox)

pub const AUTHORIZE_URL: &str = "https://apis.roblox.com/oauth/v1/authorize";
pub const TOKEN_URL: &str = "https://apis.roblox.com/oauth/v1/token";
pub const USERINFO_URL: &str = "https://apis.roblox.com/oauth/v1/userinfo";
pub const SCOPES: &str = "openid profile universe-places:write";

pub fn authorize_url(client_id: &str, redirect_uri: &str, state: &str, challenge: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?client_id={}&redirect_uri={}&scope={}&response_type=code&state={}&code_challenge={}&code_challenge_method=S256",
        percent_encode(client_id),
        percent_encode(redirect_uri),
        percent_encode(SCOPES),
        percent_encode(state),
        percent_encode(challenge)
    )
}

/// Extracts `code` from a redirect URL, verifying `state`.
pub fn parse_redirect(url: &str, expected_state: &str) -> Result<String, String> {
    let query = url.split_once('?').map(|x| x.1).unwrap_or("").split('#').next().unwrap_or("");
    let (mut code, mut state, mut error, mut error_desc) = (None, None, None, None);
    for kv in query.split('&') {
        match kv.split_once('=') {
            Some(("code", v)) => code = Some(v.to_string()),
            Some(("state", v)) => state = Some(v.to_string()),
            Some(("error_description", v)) => error_desc = Some(v.replace('+', " ")),
            Some(("error", v)) => error = Some(v.replace('+', " ")),
            _ => {}
        }
    }
    if let Some(e) = error_desc.or(error) {
        return Err(format!("Login was not completed: {e}"));
    }
    if state.as_deref() != Some(&percent_encode(expected_state)) && state.as_deref() != Some(expected_state) {
        return Err("Login response did not match the request (invalid state)".into());
    }
    code.ok_or_else(|| "Login response did not contain an authorization code".into())
}

pub fn token_request(client_id: &str, client_secret: Option<&str>, code: &str, verifier: &str, redirect_uri: &str) -> HttpRequest {
    let mut pairs = vec![("grant_type", "authorization_code"), ("code", code), ("client_id", client_id), ("code_verifier", verifier), ("redirect_uri", redirect_uri)];
    if let Some(s) = client_secret {
        pairs.push(("client_secret", s));
    }
    HttpRequest { method: "POST", url: TOKEN_URL.into(), headers: vec![("Content-Type".into(), "application/x-www-form-urlencoded".into())], body: form(&pairs) }
}

pub fn refresh_request(client_id: &str, client_secret: Option<&str>, refresh_token: &str) -> HttpRequest {
    let mut pairs = vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("client_id", client_id)];
    if let Some(s) = client_secret {
        pairs.push(("client_secret", s));
    }
    HttpRequest { method: "POST", url: TOKEN_URL.into(), headers: vec![("Content-Type".into(), "application/x-www-form-urlencoded".into())], body: form(&pairs) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_in: u64,
}

pub fn parse_token(r: &HttpResponse) -> Result<Token, String> {
    if r.status != 200 {
        return Err(api_error(r));
    }
    let j = json_of(r)?;
    Ok(Token {
        access: j.get("access_token").and_then(|v| v.as_str()).ok_or("Token response has no access_token")?.to_string(),
        refresh: j.get("refresh_token").and_then(|v| v.as_str()).map(String::from),
        expires_in: j.get("expires_in").and_then(|v| v.as_f64()).unwrap_or(900.0) as u64,
    })
}

pub fn userinfo_request(token: &str) -> HttpRequest {
    bearer(get(USERINFO_URL.into()), token)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: u64,
    pub name: String,
}

pub fn parse_user(r: &HttpResponse) -> Result<User, String> {
    if r.status != 200 {
        return Err(api_error(r));
    }
    let j = json_of(r)?;
    let id = j.get("sub").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).ok_or("User info has no valid 'sub'")?;
    let name = ["preferred_username", "nickname", "name"].iter().find_map(|k| j.get(k).and_then(|v| v.as_str())).unwrap_or("Roblox user").to_string();
    Ok(User { id, name })
}

// ------------------------------------------------------------------ thumbnails (rbxthumb)

fn snap(v: u32, allowed: &[u32]) -> u32 {
    *allowed.iter().min_by_key(|a| (**a as i64 - v as i64).abs()).unwrap_or(&420)
}

/// Request for the thumbnail endpoint behind an `rbxthumb://type=...` URI. None for unsupported kinds.
pub fn thumb_request(kind: &str, id: u64, w: u32, h: u32) -> Option<HttpRequest> {
    let s = w.max(h);
    let (path, param, sizes): (&str, &str, &[u32]) = match kind {
        "Asset" => ("assets", "assetIds", &[30, 42, 50, 75, 110, 150, 250, 420, 512, 700]),
        "Avatar" => ("users/avatar", "userIds", &[30, 48, 60, 75, 100, 150, 180, 352, 420, 720]),
        "AvatarHeadShot" => ("users/avatar-headshot", "userIds", &[48, 50, 60, 75, 100, 110, 150, 180, 352, 420, 720]),
        "AvatarBust" => ("users/avatar-bust", "userIds", &[48, 50, 60, 75, 100, 150, 180, 352, 420]),
        "GameIcon" => ("games/icons", "universeIds", &[50, 128, 150, 256, 420, 512]),
        "GamePass" => ("game-passes", "gamePassIds", &[150, 420]),
        "BadgeIcon" => ("badges/icons", "badgeIds", &[150]),
        "GroupIcon" => ("groups/icons", "groupIds", &[150, 420]),
        "BundleThumbnail" => ("bundles/thumbnails", "bundleIds", &[150, 420]),
        _ => return None,
    };
    let n = snap(s, sizes);
    Some(get(format!("https://thumbnails.roblox.com/v1/{path}?{param}={id}&size={n}x{n}&format=Png")))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Thumb {
    Ready(String),
    Pending,
    Failed(String),
}

pub fn parse_thumb(r: &HttpResponse) -> Result<Thumb, String> {
    if r.status != 200 {
        return Err(api_error(r));
    }
    let j = json_of(r)?;
    let first = j.get("data").and_then(|d| d.as_arr()).and_then(|d| d.first()).ok_or("Thumbnail response is empty")?;
    Ok(match first.get("state").and_then(|s| s.as_str()) {
        Some("Completed") => match first.get("imageUrl").and_then(|u| u.as_str()) {
            Some(u) if !u.is_empty() => Thumb::Ready(u.to_string()),
            _ => Thumb::Failed("Thumbnail has no image".into()),
        },
        Some("Pending") | Some("TemporarilyUnavailable") => Thumb::Pending,
        Some(other) => Thumb::Failed(format!("Thumbnail is {other}")),
        None => Thumb::Failed("Thumbnail response has no state".into()),
    })
}

// ------------------------------------------------------------------ assets and places

pub fn asset_location_request(id: u64, cookie: Option<&str>) -> HttpRequest {
    let mut r = get(format!("https://assetdelivery.roblox.com/v2/assetId/{id}"));
    r.headers.push(("User-Agent".into(), "Roblox/WinInet".into()));
    if let Some(c) = cookie.filter(|c| !c.is_empty()) {
        r.headers.push(("Cookie".into(), format!(".ROBLOSECURITY={c}")));
    }
    r
}

pub fn parse_asset_location(r: &HttpResponse) -> Result<String, String> {
    if r.status != 200 {
        let base = api_error(r);
        return Err(match r.status {
            401 | 403 => format!("{base}. The asset is private or not copyable; a place must be uncopylocked or owned by the signed-in account"),
            404 => format!("{base}. The asset does not exist"),
            _ => base,
        });
    }
    let j = json_of(r)?;
    j.get("locations")
        .and_then(|l| l.as_arr())
        .and_then(|l| l.first())
        .and_then(|l| l.get("location"))
        .and_then(|l| l.as_str())
        .map(String::from)
        .ok_or_else(|| "Asset response has no download location".to_string())
}

pub const MAX_ASSET_BYTES: usize = 200 * 1024 * 1024;

/// Downloads an asset (model or place file) in two steps: resolve the CDN location, then fetch it.
pub fn fetch_asset(http: &dyn Http, id: u64, cookie: Option<&str>) -> Result<Vec<u8>, String> {
    let loc = parse_asset_location(&http.send(&asset_location_request(id, cookie))?)?;
    if !loc.starts_with("https://") {
        return Err("Roblox returned a non-HTTPS download location".into());
    }
    let r = http.send(&get(loc))?;
    if r.status != 200 {
        return Err(format!("Asset download failed with HTTP {}", r.status));
    }
    if r.body.len() > MAX_ASSET_BYTES {
        return Err("Asset is larger than the 200 MB import limit".into());
    }
    Ok(r.body)
}

// ------------------------------------------------------------------ publishing and games

pub fn publish_request(universe_id: u64, place_id: u64, file: Vec<u8>, binary: bool, token: &str) -> HttpRequest {
    let mut r = HttpRequest {
        method: "POST",
        url: format!("https://apis.roblox.com/universes/v1/{universe_id}/places/{place_id}/versions?versionType=Published"),
        headers: vec![("Content-Type".into(), if binary { "application/octet-stream" } else { "application/xml" }.into())],
        body: file,
    };
    r = bearer(r, token);
    r
}

pub fn parse_publish(r: &HttpResponse) -> Result<u64, String> {
    if r.status != 200 {
        let base = api_error(r);
        return Err(match r.status {
            401 => format!("{base}. Sign in again"),
            403 => format!("{base}. The account has no permission to write to this place"),
            _ => base,
        });
    }
    json_of(r)?.get("versionNumber").and_then(|v| v.as_f64()).map(|v| v as u64).ok_or_else(|| "Publish response has no versionNumber".into())
}

pub fn user_games_request(user_id: u64) -> HttpRequest {
    get(format!("https://games.roblox.com/v2/users/{user_id}/games?accessFilter=2&limit=50&sortOrder=Desc"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub universe_id: u64,
    pub place_id: u64,
    pub name: String,
}

pub fn parse_games(r: &HttpResponse) -> Result<Vec<Game>, String> {
    if r.status != 200 {
        return Err(api_error(r));
    }
    let j = json_of(r)?;
    Ok(j.get("data")
        .and_then(|d| d.as_arr())
        .unwrap_or(&[])
        .iter()
        .filter_map(|g| {
            Some(Game {
                universe_id: g.get("id")?.as_f64()? as u64,
                place_id: g.get("rootPlace")?.get("id")?.as_f64()? as u64,
                name: g.get("name")?.as_str()?.to_string(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn resp(status: u16, body: &str) -> HttpResponse {
        HttpResponse { status, body: body.as_bytes().to_vec() }
    }

    struct Mock(RefCell<Vec<HttpResponse>>, RefCell<Vec<String>>);
    impl Http for Mock {
        fn send(&self, r: &HttpRequest) -> Result<HttpResponse, String> {
            self.1.borrow_mut().push(r.url.clone());
            Ok(self.0.borrow_mut().remove(0))
        }
    }

    #[test]
    fn sha256_and_pkce_known_vectors() {
        let hex = |d: [u8; 32]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(
            hex(sha256(&vec![b'a'; 1000])),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert!(pkce_verifier(&[1; 31]).is_err());
        assert_eq!(pkce_verifier(&[7; 48]).unwrap().len(), 64);
    }

    #[test]
    fn oauth_urls_and_redirects() {
        let u = authorize_url("id 1", "moge://cb", "st&1", "chal");
        assert!(u.contains("client_id=id%201") && u.contains("redirect_uri=moge%3A%2F%2Fcb") && u.contains("code_challenge_method=S256"));
        assert_eq!(parse_redirect("moge://cb?code=abc&state=xyz", "xyz").unwrap(), "abc");
        assert!(parse_redirect("moge://cb?code=abc&state=bad", "xyz").unwrap_err().contains("invalid state"));
        assert!(parse_redirect("moge://cb?error=access_denied&error_description=User+denied", "x").unwrap_err().contains("User denied"));
        assert!(parse_redirect("moge://cb?state=xyz", "xyz").is_err());
        let t = token_request("cid", Some("sec"), "co de", "ver", "moge://cb");
        assert_eq!(String::from_utf8(t.body).unwrap(), "grant_type=authorization_code&code=co%20de&client_id=cid&code_verifier=ver&redirect_uri=moge%3A%2F%2Fcb&client_secret=sec");
    }

    #[test]
    fn parses_token_user_and_errors() {
        let t = parse_token(&resp(200, r#"{"access_token":"a","refresh_token":"r","expires_in":900}"#)).unwrap();
        assert_eq!(t, Token { access: "a".into(), refresh: Some("r".into()), expires_in: 900 });
        let e = parse_token(&resp(400, r#"{"error":"invalid_grant","error_description":"Code expired"}"#)).unwrap_err();
        assert_eq!(e, "Roblox returned HTTP 400: Code expired");
        assert!(parse_token(&resp(200, "{}")).is_err() && parse_token(&resp(200, "<html>")).is_err());
        assert_eq!(parse_user(&resp(200, r#"{"sub":"123","preferred_username":"bob"}"#)).unwrap(), User { id: 123, name: "bob".into() });
        assert!(parse_user(&resp(200, r#"{"sub":"abc"}"#)).is_err());
    }

    #[test]
    fn thumbnails() {
        assert_eq!(thumb_request("Asset", 5, 420, 420).unwrap().url, "https://thumbnails.roblox.com/v1/assets?assetIds=5&size=420x420&format=Png");
        assert!(thumb_request("AvatarHeadShot", 7, 150, 150).unwrap().url.contains("users/avatar-headshot?userIds=7&size=150x150"));
        assert!(thumb_request("Asset", 5, 400, 400).unwrap().url.contains("size=420x420"));
        assert!(thumb_request("Nope", 1, 1, 1).is_none());
        assert_eq!(parse_thumb(&resp(200, r#"{"data":[{"state":"Completed","imageUrl":"https://t/x.png"}]}"#)).unwrap(), Thumb::Ready("https://t/x.png".into()));
        assert_eq!(parse_thumb(&resp(200, r#"{"data":[{"state":"Pending"}]}"#)).unwrap(), Thumb::Pending);
        assert!(matches!(parse_thumb(&resp(200, r#"{"data":[{"state":"Blocked"}]}"#)).unwrap(), Thumb::Failed(_)));
        assert!(parse_thumb(&resp(200, r#"{"data":[]}"#)).is_err());
    }

    #[test]
    fn fetches_assets_in_two_steps_and_reports_errors() {
        let m = Mock(RefCell::new(vec![resp(200, r#"{"locations":[{"location":"https://cdn/x"}]}"#), resp(200, "FILE")]), RefCell::new(vec![]));
        assert_eq!(fetch_asset(&m, 9, None).unwrap(), b"FILE");
        assert_eq!(m.1.borrow().as_slice(), ["https://assetdelivery.roblox.com/v2/assetId/9", "https://cdn/x"]);
        let m = Mock(RefCell::new(vec![resp(403, r#"{"errors":[{"code":403,"message":"Forbidden"}]}"#)]), RefCell::new(vec![]));
        let e = fetch_asset(&m, 9, Some("c")).unwrap_err();
        assert!(e.contains("Forbidden") && e.contains("uncopylocked"), "{e}");
        let m = Mock(RefCell::new(vec![resp(200, r#"{"locations":[{"location":"http://insecure"}]}"#)]), RefCell::new(vec![]));
        assert!(fetch_asset(&m, 9, None).unwrap_err().contains("non-HTTPS"));
        assert!(asset_location_request(1, Some("tok")).headers.iter().any(|(k, v)| k == "Cookie" && v == ".ROBLOSECURITY=tok"));
        assert!(!asset_location_request(1, Some("")).headers.iter().any(|(k, _)| k == "Cookie"));
    }

    #[test]
    fn publish_and_games() {
        let r = publish_request(1, 2, vec![1], false, "T");
        assert!(r.url.ends_with("/universes/v1/1/places/2/versions?versionType=Published"));
        assert!(r.headers.contains(&("Content-Type".into(), "application/xml".into())) && r.headers.contains(&("Authorization".into(), "Bearer T".into())));
        assert_eq!(parse_publish(&resp(200, r#"{"versionNumber":42}"#)).unwrap(), 42);
        assert!(parse_publish(&resp(403, "{}")).unwrap_err().contains("no permission"));
        let g = parse_games(&resp(200, r#"{"data":[{"id":10,"name":"G","rootPlace":{"id":11}},{"id":1}]}"#)).unwrap();
        assert_eq!(g, vec![Game { universe_id: 10, place_id: 11, name: "G".into() }]);
    }
}
