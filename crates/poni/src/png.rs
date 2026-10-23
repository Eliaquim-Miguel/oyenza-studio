//! Dependency-free PNG encoder (self-test screenshots, generated sample files).
//!
//! Writes 8-bit, non-interlaced RGBA or greyscale images. Each row gets the
//! PNG filter with the smallest sum of absolute differences (the classic
//! libpng heuristic), and the filtered bytes are zlib-compressed with a single
//! fixed-Huffman deflate block driven by a greedy LZ77 matcher. When that
//! would come out larger than the input (noise), stored blocks are used
//! instead. CRC-32 and Adler-32 are computed here too.

use std::path::Path;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
/// IDAT payloads are split at this size (any size is valid; this is conventional).
const IDAT_MAX: usize = 1 << 20;

// ---------------------------------------------------------------- checksums

const fn make_crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

static CRC_TABLE: [u32; 256] = make_crc_table();

fn crc32_update(mut c: u32, data: &[u8]) -> u32 {
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c
}

/// CRC-32 (ISO 3309 / PNG chunk checksum).
pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0xFFFF_FFFF, data) ^ 0xFFFF_FFFF
}

/// Adler-32 (zlib trailer).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    // 5552 is the longest run that cannot overflow u32 before the modulo.
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------- pixels

/// Premultiplied BGRA (Direct2D / GDI layout) → straight RGBA.
pub fn pbgra_to_rgba(pbgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pbgra.len() / 4 * 4);
    for px in pbgra.as_chunks::<4>().0 {
        let a = px[3] as u32;
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
        out.extend_from_slice(&[un(px[2]), un(px[1]), un(px[0]), a as u8]);
    }
    out
}

// ---------------------------------------------------------------- encoder

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorType {
    Gray = 0,
    Rgba = 6,
}

impl ColorType {
    fn channels(self) -> usize {
        match self {
            ColorType::Gray => 1,
            ColorType::Rgba => 4,
        }
    }
}

/// Encode straight (non-premultiplied) RGBA, top-down rows, `w * 4` stride.
///
/// A short buffer is padded with transparent pixels; extra bytes are ignored.
/// PNG needs at least one pixel, so a zero size yields a 1×1 transparent image.
pub fn encode_rgba(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    encode(w, h, ColorType::Rgba, rgba)
}

/// Encode 8-bit greyscale, top-down rows, `w` stride (same padding rules).
pub fn encode_gray(w: u32, h: u32, gray: &[u8]) -> Vec<u8> {
    encode(w, h, ColorType::Gray, gray)
}

/// Save premultiplied BGRA pixels (as produced by the renderer) as an RGBA PNG.
pub fn save_pbgra(path: impl AsRef<Path>, w: u32, h: u32, pbgra: &[u8]) -> Result<(), String> {
    check_len(path.as_ref(), w, h, 4, pbgra.len())?;
    write_file(path.as_ref(), &encode_rgba(w, h, &pbgra_to_rgba(pbgra)))
}

/// Save straight RGBA pixels as a PNG.
pub fn save_rgba(path: impl AsRef<Path>, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    check_len(path.as_ref(), w, h, 4, rgba.len())?;
    write_file(path.as_ref(), &encode_rgba(w, h, rgba))
}

/// Save 8-bit greyscale pixels as a PNG.
pub fn save_gray(path: impl AsRef<Path>, w: u32, h: u32, gray: &[u8]) -> Result<(), String> {
    check_len(path.as_ref(), w, h, 1, gray.len())?;
    write_file(path.as_ref(), &encode_gray(w, h, gray))
}

fn check_len(path: &Path, w: u32, h: u32, channels: usize, len: usize) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err(format!("{}: empty image ({w}x{h})", path.display()));
    }
    let want = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(channels));
    if want != Some(len) {
        return Err(format!("{}: {w}x{h}x{channels} image needs {want:?} bytes, got {len}", path.display()));
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn encode(w: u32, h: u32, ct: ColorType, data: &[u8]) -> Vec<u8> {
    let (w, h, data) = if w == 0 || h == 0 { (1, 1, &[0u8; 4][..ct.channels()]) } else { (w, h, data) };
    let ch = ct.channels();
    let raw = filter_rows(data, w as usize * ch, h as usize, ch);
    let z = zlib_compress(&raw);

    let mut out = Vec::with_capacity(z.len() + 64);
    out.extend_from_slice(&SIGNATURE);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    // Bit depth 8, colour type, deflate, adaptive filtering, no interlace.
    ihdr.extend_from_slice(&[8, ct as u8, 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &ihdr);
    // Pixels are sRGB (perceptual intent), so viewers don't colour-manage them.
    write_chunk(&mut out, b"sRGB", &[0]);
    for part in z.chunks(IDAT_MAX) {
        write_chunk(&mut out, b"IDAT", part);
    }
    write_chunk(&mut out, b"IEND", &[]);
    out
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Prefix every row with its best filter type and filter it.
fn filter_rows(data: &[u8], row_len: usize, rows: usize, bpp: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity((row_len + 1) * rows);
    let mut prev = vec![0u8; row_len];
    let mut cur = vec![0u8; row_len];
    let mut cand: [Vec<u8>; 5] = std::array::from_fn(|_| vec![0u8; row_len]);
    for y in 0..rows {
        cur.fill(0);
        let start = y * row_len;
        if start < data.len() {
            let end = (start + row_len).min(data.len());
            cur[..end - start].copy_from_slice(&data[start..end]);
        }
        let mut cost = [0u64; 5];
        for i in 0..row_len {
            let x = cur[i];
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let v = [
                x,
                x.wrapping_sub(a),
                x.wrapping_sub(b),
                x.wrapping_sub(((a as u16 + b as u16) / 2) as u8),
                x.wrapping_sub(paeth(a, b, c)),
            ];
            for f in 0..5 {
                cand[f][i] = v[f];
                cost[f] += (v[f] as i8).unsigned_abs() as u64;
            }
        }
        let mut best = 0;
        for f in 1..5 {
            if cost[f] < cost[best] {
                best = f;
            }
        }
        out.push(best as u8);
        out.extend_from_slice(&cand[best]);
        std::mem::swap(&mut prev, &mut cur);
    }
    out
}

// ---------------------------------------------------------------- zlib / deflate

/// zlib stream (RFC 1950) of `data`: fixed-Huffman deflate, or stored blocks
/// when those are smaller.
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let fixed = deflate_fixed(data);
    let stored_len = stored_size(data.len());
    let mut out = Vec::with_capacity(fixed.len().min(stored_len) + 6);
    // CMF: deflate, 32 KiB window. FLG: "fastest" level, FCHECK so the pair is a multiple of 31.
    out.extend_from_slice(&[0x78, 0x01]);
    if fixed.len() <= stored_len {
        out.extend_from_slice(&fixed);
    } else {
        deflate_stored_into(&mut out, data);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn stored_size(n: usize) -> usize {
    n + n.div_ceil(65_535).max(1) * 5
}

fn deflate_stored_into(out: &mut Vec<u8>, data: &[u8]) {
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
        return;
    }
    let mut blocks = data.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        // Header bits: BFINAL, BTYPE = 00; the rest of the byte is padding.
        out.push(blocks.peek().is_none() as u8);
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] =
    [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

const fn reverse_bits(mut v: u32, len: u32) -> u32 {
    let mut r = 0;
    let mut i = 0;
    while i < len {
        r = (r << 1) | (v & 1);
        v >>= 1;
        i += 1;
    }
    r
}

/// Fixed literal/length codes (RFC 1951 §3.2.6), bit-reversed for an LSB-first stream.
const fn build_litlen() -> [(u16, u8); 288] {
    let mut t = [(0u16, 0u8); 288];
    let mut s = 0u32;
    while s < 288 {
        let (code, len) = if s < 144 {
            (0x30 + s, 8)
        } else if s < 256 {
            (0x190 + (s - 144), 9)
        } else if s < 280 {
            (s - 256, 7)
        } else {
            (0xC0 + (s - 280), 8)
        };
        t[s as usize] = (reverse_bits(code, len) as u16, len as u8);
        s += 1;
    }
    t
}

/// Match length (3..=258) → index into LEN_BASE. 258 must use code 285.
const fn build_len_index() -> [u8; 259] {
    let mut t = [0u8; 259];
    let mut code = 0usize;
    let mut len = 3usize;
    while len <= 258 {
        while code + 1 < 29 && LEN_BASE[code + 1] as usize <= len {
            code += 1;
        }
        t[len] = code as u8;
        len += 1;
    }
    t
}

static LITLEN: [(u16, u8); 288] = build_litlen();
static LEN_INDEX: [u8; 259] = build_len_index();

/// Distance (1..=32768) → distance code 0..=29.
fn dist_code(d: usize) -> usize {
    if d <= 4 {
        return d - 1;
    }
    let d1 = (d - 1) as u32;
    let nb = 31 - d1.leading_zeros(); // floor(log2(d - 1)), >= 2 here
    (2 * nb + ((d1 >> (nb - 1)) & 1)) as usize
}

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn with_capacity(cap: usize) -> Self {
        Self { out: Vec::with_capacity(cap), acc: 0, n: 0 }
    }

    /// Append the low `len` bits of `bits`, least significant first.
    fn put(&mut self, bits: u32, len: u32) {
        self.acc |= ((bits as u64) & ((1u64 << len) - 1)) << self.n;
        self.n += len;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn sym(&mut self, s: usize) {
        let (code, len) = LITLEN[s];
        self.put(code as u32, len as u32);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

const WINDOW: usize = 32 * 1024;
const HASH_BITS: u32 = 15;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MAX_CHAIN: usize = 32;
const NIL: u32 = u32::MAX;

fn hash3(d: &[u8], i: usize) -> usize {
    let v = d[i] as u32 | (d[i + 1] as u32) << 8 | (d[i + 2] as u32) << 16;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// One final fixed-Huffman deflate block (RFC 1951) with greedy LZ77 matching.
fn deflate_fixed(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    let mut w = BitWriter::with_capacity(n / 3 + 16);
    w.put(1, 1); // BFINAL
    w.put(1, 2); // BTYPE = 01 (fixed Huffman)
    let mut head = vec![NIL; 1 << HASH_BITS];
    let mut prev = vec![NIL; WINDOW];
    let insert = |head: &mut [u32], prev: &mut [u32], pos: usize| {
        if pos + MIN_MATCH <= n {
            let h = hash3(data, pos);
            prev[pos % WINDOW] = head[h];
            head[h] = pos as u32;
        }
    };
    let mut i = 0;
    while i < n {
        let mut best_len = 0;
        let mut best_dist = 0;
        if i + MIN_MATCH <= n {
            let max_len = (n - i).min(MAX_MATCH);
            let mut cand = head[hash3(data, i)];
            let mut chain = MAX_CHAIN;
            while cand != NIL && chain > 0 {
                let c = cand as usize;
                if c >= i || i - c > WINDOW {
                    break;
                }
                // Cheap reject: a longer match must also match at `best_len`.
                if best_len < max_len && data[c + best_len] == data[i + best_len] {
                    let mut l = 0;
                    while l < max_len && data[c + l] == data[i + l] {
                        l += 1;
                    }
                    if l > best_len {
                        best_len = l;
                        best_dist = i - c;
                        if l == max_len {
                            break;
                        }
                    }
                }
                let next = prev[c % WINDOW];
                if next != NIL && next as usize >= c {
                    break;
                }
                cand = next;
                chain -= 1;
            }
        }
        if best_len >= MIN_MATCH {
            let li = LEN_INDEX[best_len] as usize;
            w.sym(257 + li);
            if LEN_EXTRA[li] > 0 {
                w.put((best_len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
            }
            let di = dist_code(best_dist);
            w.put(reverse_bits(di as u32, 5), 5);
            if DIST_EXTRA[di] > 0 {
                w.put((best_dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
            }
            for p in i..i + best_len {
                insert(&mut head, &mut prev, p);
            }
            i += best_len;
        } else {
            w.sym(data[i] as usize);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    w.sym(256); // end of block
    w.finish()
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal inflater for what this encoder emits (stored + fixed blocks).
    struct BitReader<'a> {
        d: &'a [u8],
        pos: usize,
        bit: u32,
    }

    impl BitReader<'_> {
        fn bit(&mut self) -> Result<u32, String> {
            let byte = *self.d.get(self.pos).ok_or("unexpected end of deflate data")?;
            let b = (byte >> self.bit) & 1;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
            Ok(b as u32)
        }
        fn bits(&mut self, n: u32) -> Result<u32, String> {
            let mut v = 0;
            for i in 0..n {
                v |= self.bit()? << i;
            }
            Ok(v)
        }
        fn align(&mut self) {
            if self.bit != 0 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        fn fixed_symbol(&mut self) -> Result<u32, String> {
            let mut code = 0;
            for len in 1..=9 {
                code = (code << 1) | self.bit()?;
                match len {
                    7 if code <= 0b001_0111 => return Ok(256 + code),
                    8 if (0x30..=0xBF).contains(&code) => return Ok(code - 0x30),
                    8 if (0xC0..=0xC7).contains(&code) => return Ok(280 + code - 0xC0),
                    9 if (0x190..=0x1FF).contains(&code) => return Ok(144 + code - 0x190),
                    _ => {}
                }
            }
            Err(format!("bad fixed code {code:#x}"))
        }
    }

    fn inflate(d: &[u8]) -> Result<Vec<u8>, String> {
        let mut r = BitReader { d, pos: 0, bit: 0 };
        let mut out: Vec<u8> = Vec::new();
        loop {
            let last = r.bits(1)?;
            match r.bits(2)? {
                0 => {
                    r.align();
                    let b = r.d.get(r.pos..r.pos + 4).ok_or("short stored header")?;
                    let len = u16::from_le_bytes([b[0], b[1]]);
                    let nlen = u16::from_le_bytes([b[2], b[3]]);
                    if len != !nlen {
                        return Err("LEN/NLEN mismatch".into());
                    }
                    r.pos += 4;
                    let body = r.d.get(r.pos..r.pos + len as usize).ok_or("short stored block")?;
                    out.extend_from_slice(body);
                    r.pos += len as usize;
                }
                1 => loop {
                    let s = r.fixed_symbol()?;
                    if s < 256 {
                        out.push(s as u8);
                    } else if s == 256 {
                        break;
                    } else {
                        let li = (s - 257) as usize;
                        let len = LEN_BASE[li] as usize + r.bits(LEN_EXTRA[li] as u32)? as usize;
                        let mut dc = 0;
                        for _ in 0..5 {
                            dc = (dc << 1) | r.bit()?;
                        }
                        let di = dc as usize;
                        let dist = DIST_BASE[di] as usize + r.bits(DIST_EXTRA[di] as u32)? as usize;
                        if dist > out.len() {
                            return Err(format!("distance {dist} beyond output {}", out.len()));
                        }
                        for _ in 0..len {
                            let b = out[out.len() - dist];
                            out.push(b);
                        }
                    }
                },
                t => return Err(format!("unsupported block type {t}")),
            }
            if last == 1 {
                return Ok(out);
            }
        }
    }

    fn unzlib(z: &[u8]) -> Result<Vec<u8>, String> {
        if z.len() < 6 || z[0] & 0x0f != 8 || ((z[0] as u32) << 8 | z[1] as u32) % 31 != 0 {
            return Err("bad zlib header".into());
        }
        let out = inflate(&z[2..z.len() - 4])?;
        let want = u32::from_be_bytes([z[z.len() - 4], z[z.len() - 3], z[z.len() - 2], z[z.len() - 1]]);
        if adler32(&out) != want {
            return Err("adler mismatch".into());
        }
        Ok(out)
    }

    struct Decoded {
        w: u32,
        h: u32,
        depth: u8,
        color: u8,
        interlace: u8,
        chunks: Vec<[u8; 4]>,
        pixels: Vec<u8>,
    }

    fn decode(png: &[u8]) -> Result<Decoded, String> {
        if png.get(..8) != Some(&SIGNATURE[..]) {
            return Err("bad signature".into());
        }
        let mut pos = 8;
        let (mut chunks, mut idat, mut ihdr) = (Vec::new(), Vec::new(), Vec::new());
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().map_err(|_| "short chunk")?) as usize;
            let kind: [u8; 4] = png[pos + 4..pos + 8].try_into().map_err(|_| "short chunk")?;
            let body = &png[pos + 8..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().map_err(|_| "short crc")?);
            if crc32(&png[pos + 4..pos + 8 + len]) != crc {
                return Err(format!("bad CRC on {}", String::from_utf8_lossy(&kind)));
            }
            match &kind {
                b"IHDR" => ihdr = body.to_vec(),
                b"IDAT" => idat.extend_from_slice(body),
                _ => {}
            }
            chunks.push(kind);
            pos += 12 + len;
        }
        if ihdr.len() != 13 {
            return Err("IHDR must be 13 bytes".into());
        }
        let w = u32::from_be_bytes(ihdr[0..4].try_into().map_err(|_| "ihdr")?);
        let h = u32::from_be_bytes(ihdr[4..8].try_into().map_err(|_| "ihdr")?);
        let bpp = if ihdr[9] == 6 { 4 } else { 1 };
        let raw = unzlib(&idat)?;
        let row = w as usize * bpp;
        if raw.len() != (row + 1) * h as usize {
            return Err(format!("raw size {} != {}", raw.len(), (row + 1) * h as usize));
        }
        let mut pixels = vec![0u8; row * h as usize];
        for y in 0..h as usize {
            let f = raw[y * (row + 1)];
            for i in 0..row {
                let x = raw[y * (row + 1) + 1 + i];
                let a = if i >= bpp { pixels[y * row + i - bpp] } else { 0 };
                let b = if y > 0 { pixels[(y - 1) * row + i] } else { 0 };
                let c = if y > 0 && i >= bpp { pixels[(y - 1) * row + i - bpp] } else { 0 };
                pixels[y * row + i] = match f {
                    0 => x,
                    1 => x.wrapping_add(a),
                    2 => x.wrapping_add(b),
                    3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                    4 => x.wrapping_add(paeth(a, b, c)),
                    _ => return Err(format!("bad filter {f}")),
                };
            }
        }
        Ok(Decoded { w, h, depth: ihdr[8], color: ihdr[9], interlace: ihdr[12], chunks, pixels })
    }

    fn noise(n: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn crc_and_adler_known_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(crc32(b""), 0);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
        // Long input exercises the deferred modulo.
        let big = vec![0xffu8; 100_000];
        let (mut a, mut b) = (1u64, 0u64);
        for &x in &big {
            a = (a + x as u64) % 65_521;
            b = (b + a) % 65_521;
        }
        assert_eq!(adler32(&big), ((b << 16) | a) as u32);
    }

    #[test]
    fn signature_and_chunk_layout() {
        let png = encode_rgba(2, 2, &[255; 16]);
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let d = decode(&png).expect("decodes");
        assert_eq!(d.chunks.first(), Some(b"IHDR"));
        assert_eq!(d.chunks.last(), Some(b"IEND"));
        assert!(d.chunks.contains(b"IDAT"));
        // IEND is the fixed 12-byte trailer every PNG ends with.
        assert_eq!(&png[png.len() - 12..], &[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]);
    }

    #[test]
    fn tiny_image_roundtrip() {
        let (w, h) = (3u32, 2u32);
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 37 % 251) as u8).collect();
        let d = decode(&encode_rgba(w, h, &rgba)).expect("decodes");
        assert_eq!((d.w, d.h, d.depth, d.color, d.interlace), (3, 2, 8, 6, 0));
        assert_eq!(d.pixels, rgba);

        let gray: Vec<u8> = vec![0, 64, 128, 255, 7, 9];
        let d = decode(&encode_gray(3, 2, &gray)).expect("decodes");
        assert_eq!((d.w, d.h, d.depth, d.color), (3, 2, 8, 0));
        assert_eq!(d.pixels, gray);
    }

    #[test]
    fn larger_image_roundtrip_uses_matches() {
        // A screenshot-like image: flat areas, a gradient and a noisy patch.
        let (w, h) = (300u32, 120u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let n = noise(40 * 40 * 4, 7);
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let px = if x < 100 {
                    [0, 0, 0, 255]
                } else if x < 200 {
                    [x as u8, y as u8 * 2, 128, 200]
                } else if x < 240 && y < 40 {
                    let k = (((y * 40) + (x - 200)) * 4) as usize;
                    [n[k], n[k + 1], n[k + 2], n[k + 3]]
                } else {
                    [250, 250, 250, 0]
                };
                rgba[o..o + 4].copy_from_slice(&px);
            }
        }
        let png = encode_rgba(w, h, &rgba);
        assert!(png.len() < rgba.len() / 3, "compressed to {} of {}", png.len(), rgba.len());
        assert_eq!(decode(&png).expect("decodes").pixels, rgba);
    }

    #[test]
    fn deflate_paths_roundtrip() {
        for data in [
            Vec::new(),
            b"a".to_vec(),
            b"abcabcabcabcabcabcabcabc hello hello hello".to_vec(),
            vec![0u8; 100_000],              // long distance-1 runs (length 258)
            noise(200_000, 1),                // incompressible → stored blocks
            [noise(40_000, 2), noise(40_000, 2)].concat(), // far matches (distance 40000 > window: literals)
            [noise(20_000, 3), noise(20_000, 3)].concat(), // distance 20000 matches
        ] {
            let z = zlib_compress(&data);
            assert_eq!(unzlib(&z).expect("inflates"), data, "len {}", data.len());
            assert!(z.len() <= stored_size(data.len()) + 6);
        }
        assert_eq!(unzlib(&zlib_compress(&[0u8; 100_000])).map(|v| v.len()), Ok(100_000));
        assert!(zlib_compress(&[0u8; 100_000]).len() < 1_000);
    }

    #[test]
    fn distance_and_length_codes() {
        for d in 1..=32_768usize {
            let c = dist_code(d);
            assert!(DIST_BASE[c] as usize <= d && d < DIST_BASE[c] as usize + (1 << DIST_EXTRA[c]), "d={d}");
        }
        for len in 3..=258usize {
            let i = LEN_INDEX[len] as usize;
            assert!(LEN_BASE[i] as usize <= len && len < LEN_BASE[i] as usize + (1 << LEN_EXTRA[i]).max(1), "len={len}");
        }
        assert_eq!(LEN_INDEX[258], 28);
        assert_eq!(LEN_INDEX[257], 27);
    }

    #[test]
    fn unpremultiply() {
        // Half-transparent pure red, premultiplied BGRA.
        assert_eq!(pbgra_to_rgba(&[0, 0, 128, 128]), vec![255, 0, 0, 128]);
        assert_eq!(pbgra_to_rgba(&[10, 20, 30, 0]), vec![0, 0, 0, 0]);
        assert_eq!(pbgra_to_rgba(&[40, 80, 120, 255]), vec![120, 80, 40, 255]);
        // Over-bright premultiplied values clamp instead of wrapping.
        assert_eq!(pbgra_to_rgba(&[200, 0, 0, 100]), vec![0, 0, 255, 100]);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert!(decode(&encode_rgba(0, 5, &[])).is_ok());
        let d = decode(&encode_rgba(2, 2, &[1, 2, 3])).expect("short buffer pads");
        assert_eq!(&d.pixels[..4], &[1, 2, 3, 0]);
        assert!(save_rgba(std::env::temp_dir().join("poni-png-test-bad.png"), 2, 2, &[0; 3]).is_err());
        assert!(save_gray(std::env::temp_dir().join("poni-png-test-empty.png"), 0, 2, &[]).is_err());
    }
}
