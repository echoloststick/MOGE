//! LZ4 block decompressor (bounds-checked, no allocation beyond the declared size).
use crate::error::FormatError;

fn bad(m: &str) -> FormatError {
    FormatError::Malformed(format!("Invalid LZ4 data: {m}"))
}

pub fn decompress(src: &[u8], expected: usize) -> Result<Vec<u8>, FormatError> {
    let mut out: Vec<u8> = Vec::with_capacity(expected.min(1 << 24));
    let mut i = 0usize;
    while i < src.len() {
        let token = src[i];
        i += 1;
        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            loop {
                let b = *src.get(i).ok_or_else(|| bad("truncated literal length"))?;
                i += 1;
                lit += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        let end = i.checked_add(lit).filter(|e| *e <= src.len()).ok_or_else(|| bad("literal run exceeds input"))?;
        if out.len() + lit > expected {
            return Err(bad("output exceeds declared size"));
        }
        out.extend_from_slice(&src[i..end]);
        i = end;
        if i >= src.len() {
            break; // the last sequence carries literals only
        }
        let off = src.get(i..i + 2).ok_or_else(|| bad("truncated match offset"))?;
        let offset = u16::from_le_bytes([off[0], off[1]]) as usize;
        i += 2;
        if offset == 0 || offset > out.len() {
            return Err(bad("match offset out of range"));
        }
        let mut mlen = (token & 15) as usize;
        if mlen == 15 {
            loop {
                let b = *src.get(i).ok_or_else(|| bad("truncated match length"))?;
                i += 1;
                mlen += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        mlen += 4;
        if out.len() + mlen > expected {
            return Err(bad("output exceeds declared size"));
        }
        let start = out.len() - offset;
        for k in 0..mlen {
            let b = out[start + k];
            out.push(b);
        }
    }
    if out.len() != expected {
        return Err(bad("output shorter than declared size"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_match() {
        let block = [0x33, b'a', b'b', b'c', 3, 0, 0x10, b'Z'];
        assert_eq!(decompress(&block, 11).unwrap(), b"abcabcabcaZ");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(decompress(&[0x33, b'a', b'b', b'c', 9, 0, 0x10, b'Z'], 11).is_err());
        assert!(decompress(&[0x33, b'a'], 11).is_err());
        assert!(decompress(&[0x33, b'a', b'b', b'c', 3, 0, 0x10, b'Z'], 5).is_err());
        assert!(decompress(&[0x33, b'a', b'b', b'c', 3, 0, 0x10, b'Z'], 99).is_err());
    }
}
