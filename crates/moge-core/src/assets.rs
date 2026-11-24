//! Asset reference parsing (rbxassetid, rbxthumb, legacy URLs) and rbxthumb conversion.

#[derive(Debug, Clone, PartialEq)]
pub enum AssetRef {
    None,
    Id(u64),
    Thumb { kind: String, id: u64, w: u32, h: u32 },
    Url(String),
    Other(String),
}

pub const DEFAULT_THUMB_SIZE: u32 = 420;

pub fn parse(input: &str) -> AssetRef {
    let s = input.trim();
    if s.is_empty() {
        return AssetRef::None;
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse().map(AssetRef::Id).unwrap_or_else(|_| AssetRef::Other(s.to_string()));
    }
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("rbxassetid://") {
        return match rest.parse::<u64>() {
            Ok(n) => AssetRef::Id(n),
            Err(_) => AssetRef::Other(s.to_string()),
        };
    }
    if let Some(rest) = lower.strip_prefix("rbxthumb://") {
        let (mut kind, mut id, mut w, mut h) = ("Asset".to_string(), None, DEFAULT_THUMB_SIZE, DEFAULT_THUMB_SIZE);
        for kv in rest.split('&') {
            let mut it = kv.splitn(2, '=');
            match (it.next(), it.next()) {
                (Some("type"), Some(v)) => kind = canonical_kind(v),
                (Some("id"), Some(v)) => id = v.parse::<u64>().ok(),
                (Some("w"), Some(v)) => w = v.parse().unwrap_or(w),
                (Some("h"), Some(v)) => h = v.parse().unwrap_or(h),
                _ => {}
            }
        }
        return match id {
            Some(id) => AssetRef::Thumb { kind, id, w, h },
            None => AssetRef::Other(s.to_string()),
        };
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        if lower.contains("roblox.com/asset") {
            for key in ["?id=", "&id="] {
                if let Some(p) = lower.find(key) {
                    let digits: String = lower[p + key.len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = digits.parse::<u64>() {
                        return AssetRef::Id(n);
                    }
                }
            }
        }
        return AssetRef::Url(s.to_string());
    }
    AssetRef::Other(s.to_string())
}

fn canonical_kind(k: &str) -> String {
    const KINDS: [&str; 10] = [
        "Asset", "Avatar", "AvatarHeadShot", "AvatarBust", "Outfit", "GameIcon", "GamePass", "BadgeIcon", "BundleThumbnail", "GroupIcon",
    ];
    KINDS.iter().find(|n| n.eq_ignore_ascii_case(k)).map(|n| n.to_string()).unwrap_or_else(|| k.to_string())
}

/// Converts an asset id or legacy reference into an `rbxthumb://` URI.
/// Returns None for values that must stay untouched (empty, local, external URLs).
pub fn to_rbxthumb(input: &str) -> Option<String> {
    match parse(input) {
        AssetRef::Id(id) => Some(format!("rbxthumb://type=Asset&id={id}&w={0}&h={0}", DEFAULT_THUMB_SIZE)),
        AssetRef::Thumb { kind, id, w, h } => Some(format!("rbxthumb://type={kind}&id={id}&w={w}&h={h}")),
        _ => None,
    }
}

/// Image-bearing Content properties (excludes audio/mesh ids such as SoundId or MeshId).
pub fn is_image_property(prop: &str) -> bool {
    matches!(
        prop,
        "Image" | "HoverImage" | "PressedImage" | "Texture" | "TextureID" | "TextureId" | "ColorMap" | "NormalMap" | "MetalnessMap"
            | "RoughnessMap" | "SkyboxBk" | "SkyboxDn" | "SkyboxFt" | "SkyboxLf" | "SkyboxRt" | "SkyboxUp" | "SunTextureId"
            | "MoonTextureId" | "ShirtTemplate" | "PantsTemplate" | "Graphic" | "OverlayTextureId" | "ImageContent" | "TextureContent"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_all_id_forms() {
        let want = Some("rbxthumb://type=Asset&id=123&w=420&h=420".to_string());
        assert_eq!(to_rbxthumb("123"), want);
        assert_eq!(to_rbxthumb("rbxassetid://123"), want);
        assert_eq!(to_rbxthumb("http://www.roblox.com/asset/?id=123"), want);
        assert_eq!(to_rbxthumb("https://www.roblox.com/asset?id=123&x=1"), want);
    }

    #[test]
    fn keeps_thumbs_and_ignores_others() {
        assert_eq!(
            to_rbxthumb("rbxthumb://type=avatarheadshot&id=7&w=150&h=150").as_deref(),
            Some("rbxthumb://type=AvatarHeadShot&id=7&w=150&h=150")
        );
        assert_eq!(to_rbxthumb(""), None);
        assert_eq!(to_rbxthumb("rbxasset://textures/x.png"), None);
        assert_eq!(to_rbxthumb("https://example.com/a.png"), None);
        assert_eq!(to_rbxthumb("99999999999999999999999"), None);
    }
}
