//! rbxl / rbxm (binary) importer. Every read is bounds-checked; malformed input yields an error, never a panic.
use crate::error::FormatError;
use crate::{lz4, Import};
use moge_core::{Id, Tree, Value};
use std::collections::HashMap;

pub const MAGIC_PREFIX: &[u8] = b"<roblox!";
const MAGIC: &[u8] = b"<roblox!\x89\xff\r\n\x1a\n";

fn mal(m: impl Into<String>) -> FormatError {
    FormatError::Malformed(m.into())
}

/// (id, row-major 3x3) for the 24 axis-aligned CFrame rotations the format encodes as one byte.
#[rustfmt::skip]
const ROTS: [(u8, [i8; 9]); 24] = [
    (0x02, [1,0,0, 0,1,0, 0,0,1]),    (0x03, [1,0,0, 0,0,-1, 0,1,0]),
    (0x05, [1,0,0, 0,-1,0, 0,0,-1]),  (0x06, [1,0,0, 0,0,1, 0,-1,0]),
    (0x07, [0,1,0, 1,0,0, 0,0,-1]),   (0x09, [0,0,1, 1,0,0, 0,1,0]),
    (0x0A, [0,-1,0, 1,0,0, 0,0,1]),   (0x0C, [0,0,-1, 1,0,0, 0,-1,0]),
    (0x0D, [0,1,0, 0,0,1, 1,0,0]),    (0x0E, [0,0,-1, 0,1,0, 1,0,0]),
    (0x10, [0,-1,0, 0,0,-1, 1,0,0]),  (0x11, [0,0,1, 0,-1,0, 1,0,0]),
    (0x14, [-1,0,0, 0,1,0, 0,0,-1]),  (0x15, [-1,0,0, 0,0,1, 0,1,0]),
    (0x17, [-1,0,0, 0,-1,0, 0,0,1]),  (0x18, [-1,0,0, 0,0,-1, 0,-1,0]),
    (0x19, [0,1,0, -1,0,0, 0,0,1]),   (0x1B, [0,0,-1, -1,0,0, 0,1,0]),
    (0x1C, [0,-1,0, -1,0,0, 0,0,-1]), (0x1E, [0,0,1, -1,0,0, 0,-1,0]),
    (0x1F, [0,1,0, 0,0,-1, -1,0,0]),  (0x20, [0,0,1, 0,1,0, -1,0,0]),
    (0x22, [0,-1,0, 0,0,1, -1,0,0]),  (0x23, [0,0,-1, 0,-1,0, -1,0,0]),
];

fn rot_from_id(id: u8) -> Option<[f32; 9]> {
    ROTS.iter().find(|(i, _)| *i == id).map(|(_, m)| m.map(|v| v as f32))
}

struct R<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn new(d: &'a [u8]) -> Self {
        R { d, p: 0 }
    }
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], FormatError> {
        let end = self.p.checked_add(n).filter(|e| *e <= self.d.len()).ok_or(FormatError::Truncated(what))?;
        let s = &self.d[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1, "byte")?[0])
    }
    fn u16(&mut self) -> Result<u16, FormatError> {
        let b = self.take(2, "u16")?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self.take(4, "u32")?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, FormatError> {
        Ok(self.u32()? as i32)
    }
    fn f32le(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn f64le(&mut self) -> Result<f64, FormatError> {
        let b = self.take(8, "f64")?;
        Ok(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }
    fn bytes(&mut self) -> Result<&'a [u8], FormatError> {
        let n = self.u32()? as usize;
        self.take(n, "string")
    }
    fn size(&self, n: usize, w: usize) -> Result<usize, FormatError> {
        n.checked_mul(w).ok_or_else(|| mal("Array size overflow"))
    }
    /// Byte-plane interleaved u32 array (most significant plane first).
    fn u32s(&mut self, n: usize) -> Result<Vec<u32>, FormatError> {
        let len = self.size(n, 4)?;
        let b = self.take(len, "u32 array")?;
        Ok((0..n).map(|i| ((b[i] as u32) << 24) | ((b[n + i] as u32) << 16) | ((b[2 * n + i] as u32) << 8) | (b[3 * n + i] as u32)).collect())
    }
    fn i32s(&mut self, n: usize) -> Result<Vec<i32>, FormatError> {
        Ok(self.u32s(n)?.into_iter().map(|v| ((v >> 1) as i32) ^ -((v & 1) as i32)).collect())
    }
    fn floats(&mut self, n: usize) -> Result<Vec<f32>, FormatError> {
        Ok(self.u32s(n)?.into_iter().map(|v| f32::from_bits((v >> 1) | (v << 31))).collect())
    }
    fn refs(&mut self, n: usize) -> Result<Vec<i32>, FormatError> {
        let mut acc = 0i32;
        Ok(self.i32s(n)?.into_iter().map(|d| {
            acc = acc.wrapping_add(d);
            acc
        }).collect())
    }
    fn i64s(&mut self, n: usize) -> Result<Vec<i64>, FormatError> {
        let len = self.size(n, 8)?;
        let b = self.take(len, "i64 array")?;
        Ok((0..n)
            .map(|i| {
                let mut v = 0u64;
                for k in 0..8 {
                    v = (v << 8) | b[k * n + i] as u64;
                }
                ((v >> 1) as i64) ^ -((v & 1) as i64)
            })
            .collect())
    }
}

pub fn load(data: &[u8], tree: &mut Tree) -> Result<Import, FormatError> {
    if !data.starts_with(MAGIC) {
        return Err(FormatError::BadMagic);
    }
    let mut r = R::new(&data[MAGIC.len()..]);
    let version = r.u16()?;
    if version != 0 {
        return Err(FormatError::Unsupported(format!("binary format version {version}")));
    }
    let (class_count, inst_count) = (r.i32()?, r.i32()?);
    if class_count < 0 || inst_count < 0 || inst_count as usize > data.len() {
        return Err(mal("Invalid class or instance count in header"));
    }
    r.take(8, "header")?;

    let mut classes: HashMap<u32, (String, Vec<Id>)> = HashMap::new();
    let mut by_ref: HashMap<i32, Id> = HashMap::new();
    let mut sstr: Vec<Vec<u8>> = Vec::new();
    let mut roots = Vec::new();
    let mut warnings = Vec::new();

    loop {
        let name = r.take(4, "chunk name")?;
        let (clen, ulen) = (r.u32()? as usize, r.u32()? as usize);
        r.u32()?;
        let payload = if clen == 0 {
            r.take(ulen, "chunk payload")?.to_vec()
        } else {
            let raw = r.take(clen, "chunk payload")?;
            if raw.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
                return Err(FormatError::Unsupported("Zstandard-compressed chunks".into()));
            }
            if ulen > (1 << 30) {
                return Err(mal("Chunk too large"));
            }
            lz4::decompress(raw, ulen)?
        };
        let mut c = R::new(&payload);
        match name {
            b"META" => {}
            b"SSTR" => {
                c.u32()?;
                let n = c.u32()? as usize;
                for _ in 0..n {
                    c.take(16, "shared string hash")?;
                    sstr.push(c.bytes()?.to_vec());
                }
            }
            b"INST" => {
                let cid = c.u32()?;
                let cname = String::from_utf8_lossy(c.bytes()?).into_owned();
                let has_service = c.u8()?;
                let n = c.u32()? as usize;
                let refs = c.refs(n)?;
                if has_service != 0 {
                    c.take(n, "service flags")?;
                }
                let mut ids = Vec::with_capacity(n);
                for rf in refs {
                    let id = tree.create(&cname);
                    if by_ref.insert(rf, id).is_some() {
                        return Err(mal(format!("Duplicate instance referent {rf}")));
                    }
                    ids.push(id);
                }
                classes.insert(cid, (cname, ids));
            }
            b"PROP" => {
                let cid = c.u32()?;
                let pname = String::from_utf8_lossy(c.bytes()?).into_owned();
                let ty = c.u8()?;
                let (cname, ids) = classes.get(&cid).ok_or_else(|| mal(format!("Property '{pname}' references unknown class id {cid}")))?;
                match read_values(ty, &mut c, ids.len(), &pname, &by_ref, &sstr)? {
                    Some(vals) => {
                        for (id, v) in ids.iter().zip(vals) {
                            apply(tree, *id, &pname, v);
                        }
                    }
                    None => warnings.push(format!("Skipped {cname}.{pname}: unsupported property type 0x{ty:02X}")),
                }
            }
            b"PRNT" => {
                c.u8()?;
                let n = c.u32()? as usize;
                let kids = c.refs(n)?;
                let parents = c.refs(n)?;
                for (k, p) in kids.iter().zip(parents) {
                    let child = *by_ref.get(k).ok_or_else(|| mal(format!("PRNT references unknown instance {k}")))?;
                    if p == -1 {
                        roots.push(child);
                    } else {
                        let parent = *by_ref.get(&p).ok_or_else(|| mal(format!("PRNT references unknown parent {p}")))?;
                        tree.set_parent(child, Some(parent)).map_err(|e| mal(e.to_string()))?;
                    }
                }
            }
            b"END\0" => break,
            other => warnings.push(format!("Ignored unknown chunk '{}'", String::from_utf8_lossy(other).trim_end_matches('\0'))),
        }
    }
    Ok(Import { roots, warnings })
}

fn apply(tree: &mut Tree, id: Id, prop: &str, v: Value) {
    if prop == "Tags" {
        let raw = match &v {
            Value::String(s) => s.as_bytes().to_vec(),
            Value::Bytes(b) => b.clone(),
            _ => return,
        };
        if let Some(n) = tree.node_mut(id) {
            n.tags = raw.split(|c| *c == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
        }
        return;
    }
    let _ = tree.set_prop(id, prop, v);
}

fn string_value(prop: &str, raw: &[u8]) -> Value {
    if prop == "AttributesSerialize" {
        return Value::Bytes(raw.to_vec());
    }
    match std::str::from_utf8(raw) {
        Ok(s) => Value::String(s.to_string()),
        Err(_) => Value::Bytes(raw.to_vec()),
    }
}

fn read_values(
    ty: u8,
    c: &mut R,
    n: usize,
    prop: &str,
    by_ref: &HashMap<i32, Id>,
    sstr: &[Vec<u8>],
) -> Result<Option<Vec<Value>>, FormatError> {
    let f64s = |v: Vec<f32>| v.into_iter().map(f64::from);
    Ok(Some(match ty {
        0x01 | 0x1D => {
            let mut out = Vec::new();
            for _ in 0..n {
                let raw = c.bytes()?;
                out.push(if ty == 0x1D { Value::Bytes(raw.to_vec()) } else { string_value(prop, raw) });
            }
            out
        }
        0x02 => c.take(n, "bool array")?.iter().map(|b| Value::Bool(*b != 0)).collect(),
        0x03 => c.i32s(n)?.into_iter().map(|v| Value::Int(v as i64)).collect(),
        0x04 => f64s(c.floats(n)?).map(Value::Float).collect(),
        0x05 => {
            c.take(c.size(n, 8)?, "double array")?;
            c.p -= n * 8;
            (0..n).map(|_| c.f64le().map(Value::Float)).collect::<Result<_, _>>()?
        }
        0x06 => {
            let (s, o) = (c.floats(n)?, c.i32s(n)?);
            (0..n).map(|i| Value::UDim(s[i], o[i])).collect()
        }
        0x07 => {
            let (sx, sy, ox, oy) = (c.floats(n)?, c.floats(n)?, c.i32s(n)?, c.i32s(n)?);
            (0..n).map(|i| Value::UDim2 { sx: sx[i], ox: ox[i], sy: sy[i], oy: oy[i] }).collect()
        }
        0x09 => c.take(n, "faces")?.iter().map(|b| Value::Faces(*b)).collect(),
        0x0A => c.take(n, "axes")?.iter().map(|b| Value::Axes(*b)).collect(),
        0x0B => c.u32s(n)?.into_iter().map(Value::BrickColor).collect(),
        0x0C => {
            let (r, g, b) = (c.floats(n)?, c.floats(n)?, c.floats(n)?);
            (0..n).map(|i| Value::Color3([r[i], g[i], b[i]])).collect()
        }
        0x0D => {
            let (x, y) = (c.floats(n)?, c.floats(n)?);
            (0..n).map(|i| Value::Vector2([x[i], y[i]])).collect()
        }
        0x0E => {
            let (x, y, z) = (c.floats(n)?, c.floats(n)?, c.floats(n)?);
            (0..n).map(|i| Value::Vector3([x[i], y[i], z[i]])).collect()
        }
        0x10 => {
            let mut rots = Vec::with_capacity(n);
            for _ in 0..n {
                let id = c.u8()?;
                if id == 0 {
                    let mut m = [0f32; 9];
                    for k in m.iter_mut() {
                        *k = c.f32le()?;
                    }
                    rots.push(m);
                } else {
                    rots.push(rot_from_id(id).ok_or_else(|| mal(format!("Unknown CFrame rotation id 0x{id:02X}")))?);
                }
            }
            let (x, y, z) = (c.floats(n)?, c.floats(n)?, c.floats(n)?);
            (0..n).map(|i| Value::CFrame { pos: [x[i], y[i], z[i]], rot: rots[i] }).collect()
        }
        0x12 => c.u32s(n)?.into_iter().map(Value::Enum).collect(),
        0x13 => c.refs(n)?.into_iter().map(|r| Value::Ref(if r == -1 { None } else { by_ref.get(&r).copied() })).collect(),
        0x15 => {
            let mut out = Vec::new();
            for _ in 0..n {
                let k = c.u32()? as usize;
                let mut keys = Vec::new();
                for _ in 0..k {
                    keys.push([c.f32le()?, c.f32le()?, c.f32le()?]);
                }
                out.push(Value::NumberSequence(keys));
            }
            out
        }
        0x16 => {
            let mut out = Vec::new();
            for _ in 0..n {
                let k = c.u32()? as usize;
                let mut keys = Vec::new();
                for _ in 0..k {
                    let t = c.f32le()?;
                    let col = [c.f32le()?, c.f32le()?, c.f32le()?];
                    c.f32le()?;
                    keys.push((t, col));
                }
                out.push(Value::ColorSequence(keys));
            }
            out
        }
        0x17 => {
            let mut out = Vec::new();
            for _ in 0..n {
                out.push(Value::NumberRange(c.f32le()?, c.f32le()?));
            }
            out
        }
        0x18 => {
            let (a, b, x, y) = (c.floats(n)?, c.floats(n)?, c.floats(n)?, c.floats(n)?);
            (0..n).map(|i| Value::Rect { min: [a[i], b[i]], max: [x[i], y[i]] }).collect()
        }
        0x1A => {
            let len = c.size(n, 1)?;
            let (r, g, b) = (c.take(len, "color")?, c.take(len, "color")?, c.take(len, "color")?);
            (0..n).map(|i| Value::Color3([r[i] as f32 / 255.0, g[i] as f32 / 255.0, b[i] as f32 / 255.0])).collect()
        }
        0x1B => c.i64s(n)?.into_iter().map(Value::Int).collect(),
        0x1C => {
            let mut out = Vec::new();
            for i in c.u32s(n)? {
                out.push(Value::Bytes(sstr.get(i as usize).ok_or_else(|| mal(format!("Shared string index {i} out of range")))?.clone()));
            }
            out
        }
        0x20 => {
            let mut out = Vec::new();
            for _ in 0..n {
                let family = String::from_utf8_lossy(c.bytes()?).into_owned();
                let weight = c.u16()?;
                let style = c.u8()?;
                c.bytes()?;
                out.push(Value::Font { family, weight, style });
            }
            out
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use moge_core::value::IDENTITY_ROT;

    fn zz(i: i32) -> u32 {
        ((i << 1) ^ (i >> 31)) as u32
    }
    fn fl(f: f32) -> u32 {
        let b = f.to_bits();
        (b << 1) | (b >> 31)
    }
    fn il(vals: &[u32]) -> Vec<u8> {
        let n = vals.len();
        let mut out = vec![0u8; n * 4];
        for (i, v) in vals.iter().enumerate() {
            for k in 0..4 {
                out[k * n + i] = (v >> (24 - 8 * k)) as u8;
            }
        }
        out
    }
    fn refs_enc(v: &[i32]) -> Vec<u8> {
        let mut prev = 0;
        let z: Vec<u32> = v.iter().map(|x| {
            let d = x - prev;
            prev = *x;
            zz(d)
        }).collect();
        il(&z)
    }
    fn s(v: &mut Vec<u8>, t: &str) {
        v.extend((t.len() as u32).to_le_bytes());
        v.extend(t.as_bytes());
    }
    fn chunk(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = name.to_vec();
        v.extend(0u32.to_le_bytes());
        v.extend((payload.len() as u32).to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(payload);
        v
    }
    fn prop(cid: u32, name: &str, ty: u8, body: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend(cid.to_le_bytes());
        s(&mut p, name);
        p.push(ty);
        p.extend(body);
        chunk(b"PROP", &p)
    }

    fn sample() -> Vec<u8> {
        let mut f = MAGIC.to_vec();
        f.extend(0u16.to_le_bytes());
        f.extend(2i32.to_le_bytes());
        f.extend(2i32.to_le_bytes());
        f.extend([0u8; 8]);
        for (cid, name, rf) in [(0u32, "Model", 0), (1, "Part", 1)] {
            let mut p = Vec::new();
            p.extend(cid.to_le_bytes());
            s(&mut p, name);
            p.push(0);
            p.extend(1u32.to_le_bytes());
            p.extend(refs_enc(&[rf]));
            f.extend(chunk(b"INST", &p));
        }
        let mut name = Vec::new();
        s(&mut name, "P");
        f.extend(prop(1, "Name", 0x01, &name));
        f.extend(prop(1, "Anchored", 0x02, &[1]));
        let mut cf = vec![0x02];
        for v in [1.0f32, 2.0, 3.0] {
            cf.extend(il(&[fl(v)]));
        }
        f.extend(prop(1, "CFrame", 0x10, &cf));
        let mut sz = Vec::new();
        for v in [4.0f32, 1.0, 2.0] {
            sz.extend(il(&[fl(v)]));
        }
        f.extend(prop(1, "Size", 0x0E, &sz));
        let mut col = Vec::new();
        for v in [1.0f32, 0.0, 0.5] {
            col.extend(il(&[fl(v)]));
        }
        f.extend(prop(1, "Color", 0x0C, &col));
        f.extend(prop(1, "Material", 0x12, &il(&[288])));
        f.extend(prop(1, "Offset", 0x03, &il(&[zz(-7)])));
        f.extend(prop(0, "PrimaryPart", 0x13, &refs_enc(&[1])));
        let mut tags = Vec::new();
        s(&mut tags, "a\0b");
        f.extend(prop(1, "Tags", 0x01, &tags));
        f.extend(prop(1, "Mystery", 0x7F, &[1, 2, 3]));
        let mut prnt = vec![0u8];
        prnt.extend(2u32.to_le_bytes());
        prnt.extend(refs_enc(&[0, 1]));
        prnt.extend(refs_enc(&[-1, 0]));
        f.extend(chunk(b"PRNT", &prnt));
        f.extend(chunk(b"END\0", b"</roblox>"));
        f
    }

    #[test]
    fn imports_sample() {
        let mut t = Tree::new();
        let imp = load(&sample(), &mut t).unwrap();
        assert_eq!(imp.roots.len(), 1);
        let m = imp.roots[0];
        let p = t.node(m).unwrap().children[0];
        assert_eq!(t.node(m).unwrap().class, "Model");
        assert_eq!(t.name(p), Some("P"));
        assert_eq!(t.get_prop(p, "Anchored"), Some(&Value::Bool(true)));
        assert_eq!(t.get_prop(p, "CFrame"), Some(&Value::CFrame { pos: [1.0, 2.0, 3.0], rot: IDENTITY_ROT }));
        assert_eq!(t.get_prop(p, "Size"), Some(&Value::Vector3([4.0, 1.0, 2.0])));
        assert_eq!(t.get_prop(p, "Color"), Some(&Value::Color3([1.0, 0.0, 0.5])));
        assert_eq!(t.get_prop(p, "Material"), Some(&Value::Enum(288)));
        assert_eq!(t.get_prop(p, "Offset"), Some(&Value::Int(-7)));
        assert_eq!(t.get_prop(m, "PrimaryPart"), Some(&Value::Ref(Some(p))));
        assert_eq!(t.node(p).unwrap().tags, ["a", "b"]);
        assert_eq!(imp.warnings.len(), 1, "{:?}", imp.warnings);
    }

    #[test]
    fn rotation_table_is_a_set_of_distinct_proper_rotations() {
        let mut seen = Vec::new();
        for (id, m) in ROTS {
            let g = |i: usize| m[i] as i32;
            let det = g(0) * (g(4) * g(8) - g(5) * g(7)) - g(1) * (g(3) * g(8) - g(5) * g(6)) + g(2) * (g(3) * g(7) - g(4) * g(6));
            assert_eq!(det, 1, "rotation 0x{id:02X} is not a proper rotation");
            assert!(!seen.contains(&m), "duplicate rotation 0x{id:02X}");
            seen.push(m);
        }
    }

    #[test]
    fn never_panics_on_corrupt_input() {
        let file = sample();
        for cut in 0..file.len() {
            let _ = load(&file[..cut], &mut Tree::new());
        }
        for i in 0..file.len() {
            let mut c = file.clone();
            c[i] ^= 0xFF;
            let _ = load(&c, &mut Tree::new());
            c[i] = 0;
            let _ = load(&c, &mut Tree::new());
        }
    }

    #[test]
    fn rejects_foreign_and_zstd() {
        assert_eq!(load(b"hello", &mut Tree::new()).unwrap_err(), FormatError::BadMagic);
        let mut f = MAGIC.to_vec();
        f.extend(0u16.to_le_bytes());
        f.extend(0i32.to_le_bytes());
        f.extend(0i32.to_le_bytes());
        f.extend([0u8; 8]);
        f.extend(b"INST");
        f.extend(8u32.to_le_bytes());
        f.extend(8u32.to_le_bytes());
        f.extend(0u32.to_le_bytes());
        f.extend([0x28, 0xB5, 0x2F, 0xFD, 0, 0, 0, 0]);
        assert!(matches!(load(&f, &mut Tree::new()), Err(FormatError::Unsupported(_))));
    }
}
