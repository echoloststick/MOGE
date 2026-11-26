//! Networking: a blocking `Http` implementation (ureq) and a tiny background-job channel.
use moge_roblox::{Game, Http, HttpRequest, HttpResponse, Token, User};
use std::io::Read;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

const MAX_BODY: u64 = 256 * 1024 * 1024;

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> UreqHttp {
        UreqHttp { agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(90)).user_agent("MOGE/0.1").build() }
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Http for UreqHttp {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, String> {
        let mut r = self.agent.request(req.method, &req.url);
        for (k, v) in &req.headers {
            r = r.set(k, v);
        }
        let res = if req.method == "GET" && req.body.is_empty() { r.call() } else { r.send_bytes(&req.body) };
        let resp = match res {
            Ok(x) => x,
            Err(ureq::Error::Status(_, x)) => x,
            Err(e) => return Err(format!("Network request failed: {e}")),
        };
        let status = resp.status();
        let mut body = Vec::new();
        resp.into_reader().take(MAX_BODY).read_to_end(&mut body).map_err(|e| format!("Failed to read the response body: {e}"))?;
        Ok(HttpResponse { status, body })
    }
}

pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub enum Msg {
    Dump(Result<String, String>),
    Asset { label: String, as_model: bool, data: Result<Vec<u8>, String> },
    Login(Result<(Token, User), String>),
    Games(Result<Vec<Game>, String>),
    Published(Result<u64, String>),
    Image { key: String, image: Result<Image, String> },
}

pub struct Net {
    tx: Sender<Msg>,
    pub rx: Receiver<Msg>,
}

impl Net {
    pub fn new() -> Net {
        let (tx, rx) = channel();
        Net { tx, rx }
    }

    pub fn spawn<F: FnOnce() -> Msg + Send + 'static>(&self, f: F) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
    }
}

impl Default for Net {
    fn default() -> Self {
        Self::new()
    }
}

pub const DUMP_URL: &str = "https://raw.githubusercontent.com/MaximumADHD/Roblox-Client-Tracker/roblox/Full-API-Dump.json";

pub fn download_text(http: &dyn Http, url: &str) -> Result<String, String> {
    let r = http.send(&HttpRequest { method: "GET", url: url.to_string(), ..Default::default() })?;
    if r.status != 200 {
        return Err(format!("Server returned HTTP {} for {url}", r.status));
    }
    String::from_utf8(r.body).map_err(|_| "The downloaded file is not valid UTF-8".to_string())
}

/// Resolves an image property value (rbxassetid / rbxthumb / numeric id) to RGBA pixels.
pub fn fetch_image(http: &dyn Http, value: &str) -> Result<Image, String> {
    use moge_core::assets::{parse, AssetRef};
    let (kind, id, w, h) = match parse(value) {
        AssetRef::Id(id) => ("Asset".to_string(), id, 420, 420),
        AssetRef::Thumb { kind, id, w, h } => (kind, id, w, h),
        AssetRef::Url(u) => return decode(&download_bytes(http, &u)?),
        _ => return Err(format!("Unsupported image reference: {value}")),
    };
    let req = moge_roblox::thumb_request(&kind, id, w, h).ok_or_else(|| format!("Unsupported thumbnail type: {kind}"))?;
    match moge_roblox::parse_thumb(&http.send(&req)?)? {
        moge_roblox::Thumb::Ready(url) => decode(&download_bytes(http, &url)?),
        moge_roblox::Thumb::Pending => Err("The thumbnail is still being generated".into()),
        moge_roblox::Thumb::Failed(e) => Err(e),
    }
}

fn download_bytes(http: &dyn Http, url: &str) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err("Only https image URLs are allowed".into());
    }
    let r = http.send(&HttpRequest { method: "GET", url: url.to_string(), ..Default::default() })?;
    if r.status != 200 {
        return Err(format!("Image download failed with HTTP {}", r.status));
    }
    Ok(r.body)
}

fn decode(bytes: &[u8]) -> Result<Image, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Failed to decode the image: {e}"))?.to_rgba8();
    Ok(Image { width: img.width() as usize, height: img.height() as usize, rgba: img.into_raw() })
}
