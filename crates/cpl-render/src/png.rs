//! A minimal PNG writer for headless page renders: 8-bit RGBA, unfiltered
//! rows, stored (uncompressed) deflate blocks. Files are large but exact, and
//! it needs no dependency; `sips` or Preview reads them.

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encodes `rgba` (`width × height × 4` bytes) as a PNG file.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    assert_eq!(rgba.len(), width as usize * height as usize * 4);
    // Each row is prefixed with filter type 0 (none).
    let row = width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks_exact(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    // zlib stream of stored blocks, at most 65535 bytes each.
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    if blocks.is_empty() {
        zlib.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlace
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A decoded image, always expanded to 8-bit RGBA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes an 8-bit, non-interlaced PNG of colour type 0, 2, 4 or 6 — what
/// `encode` and `sips` write. Enough to read goldens back; not a general
/// decoder.
pub fn decode(file: &[u8]) -> Result<Image, String> {
    if file.len() < 8 || &file[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("not a PNG".into());
    }
    let (mut width, mut height, mut color) = (0u32, 0u32, 0u8);
    let mut idat = Vec::new();
    let mut at = 8;
    while at + 12 <= file.len() {
        let len = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        let kind = &file[at + 4..at + 8];
        let data = file.get(at + 8..at + 8 + len).ok_or("truncated chunk")?;
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(data[0..4].try_into().unwrap());
                height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                let (depth, interlace) = (data[8], data[12]);
                color = data[9];
                if depth != 8 || interlace != 0 || ![0, 2, 4, 6].contains(&color) {
                    return Err(format!(
                        "unsupported PNG: depth {depth} colour {color} interlace {interlace}"
                    ));
                }
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    let channels = match color {
        0 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let raw = inflate(idat.get(2..).ok_or("empty zlib stream")?)?;
    let stride = width as usize * channels;
    if raw.len() < (stride + 1) * height as usize {
        return Err("image data too short".into());
    }
    let mut pixels = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let a = if x >= channels {
                pixels[y * stride + x - channels]
            } else {
                0
            } as i32;
            let b = if y > 0 {
                pixels[(y - 1) * stride + x]
            } else {
                0
            } as i32;
            let c = if x >= channels && y > 0 {
                pixels[(y - 1) * stride + x - channels]
            } else {
                0
            } as i32;
            let predictor = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                other => return Err(format!("bad row filter {other}")),
            };
            pixels[y * stride + x] = (line[x] as i32 + predictor) as u8;
        }
    }
    let rgba = match channels {
        4 => pixels,
        _ => pixels
            .chunks_exact(channels)
            .flat_map(|px| match channels {
                1 => [px[0], px[0], px[0], 255],
                2 => [px[0], px[0], px[0], px[1]],
                _ => [px[0], px[1], px[2], 255],
            })
            .collect(),
    };
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// Bits read least significant first, as deflate packs them.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    bit: u32,
}

impl Bits<'_> {
    fn take(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0;
        for i in 0..count {
            let byte = *self.data.get(self.at).ok_or("deflate stream ended early")?;
            value |= (((byte >> self.bit) & 1) as u32) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.at += 1;
            }
        }
        Ok(value)
    }
}

/// Canonical Huffman code as (counts per length, symbols in code order).
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Self {
        let mut counts = [0u16; 16];
        for &len in lengths {
            counts[len as usize] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for len in 1..16 {
            offsets[len] = offsets[len - 1] + counts[len - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &len) in lengths.iter().enumerate() {
            if len != 0 {
                symbols[offsets[len as usize] as usize] = symbol as u16;
                offsets[len as usize] += 1;
            }
        }
        Self { counts, symbols }
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.take(1)? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return Ok(self.symbols[(index + code - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("bad Huffman code".into())
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// RFC 1951 inflate (after puff.c): stored, fixed and dynamic blocks.
fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut bits = Bits {
        data,
        at: 0,
        bit: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = bits.take(1)?;
        match bits.take(2)? {
            0 => {
                if bits.bit != 0 {
                    bits.bit = 0;
                    bits.at += 1;
                }
                let header = data.get(bits.at..bits.at + 4).ok_or("stored header")?;
                let len = u16::from_le_bytes([header[0], header[1]]) as usize;
                bits.at += 4;
                out.extend_from_slice(data.get(bits.at..bits.at + len).ok_or("stored data")?);
                bits.at += len;
            }
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                codes(
                    &mut bits,
                    &mut out,
                    &Huffman::new(&lengths),
                    &Huffman::new(&[5; 30]),
                )?;
            }
            2 => {
                let literals = bits.take(5)? as usize + 257;
                let distances = bits.take(5)? as usize + 1;
                let code_lengths = bits.take(4)? as usize + 4;
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut lens = [0u8; 19];
                for &slot in ORDER.iter().take(code_lengths) {
                    lens[slot] = bits.take(3)? as u8;
                }
                let lencode = Huffman::new(&lens);
                let mut lengths = vec![0u8; literals + distances];
                let mut index = 0;
                while index < lengths.len() {
                    let symbol = lencode.decode(&mut bits)?;
                    let (value, repeat) = match symbol {
                        0..=15 => (symbol as u8, 1),
                        16 => {
                            let previous =
                                *lengths[..index].last().ok_or("repeat with no length")?;
                            (previous, 3 + bits.take(2)? as usize)
                        }
                        17 => (0, 3 + bits.take(3)? as usize),
                        _ => (0, 11 + bits.take(7)? as usize),
                    };
                    for _ in 0..repeat {
                        *lengths.get_mut(index).ok_or("too many lengths")? = value;
                        index += 1;
                    }
                }
                let literal = Huffman::new(&lengths[..literals]);
                let distance = Huffman::new(&lengths[literals..]);
                codes(&mut bits, &mut out, &literal, &distance)?;
            }
            _ => return Err("bad deflate block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

fn codes(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    literal: &Huffman,
    distance: &Huffman,
) -> Result<(), String> {
    loop {
        let symbol = literal.decode(bits)? as usize;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            _ => {
                let index = symbol - 257;
                if index >= 29 {
                    return Err("bad length symbol".into());
                }
                let len =
                    LENGTH_BASE[index] as usize + bits.take(LENGTH_EXTRA[index] as u32)? as usize;
                let dsym = distance.decode(bits)? as usize;
                if dsym >= 30 {
                    return Err("bad distance symbol".into());
                }
                let dist = DIST_BASE[dsym] as usize + bits.take(DIST_EXTRA[dsym] as u32)? as usize;
                if dist > out.len() {
                    return Err("distance too far back".into());
                }
                let start = out.len() - dist;
                for i in 0..len {
                    out.push(out[start + i]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_encoding_decodes_back_exactly() {
        let rgba: Vec<u8> = (0..70_000u32)
            .flat_map(|i| [(i % 251) as u8, (i % 7) as u8, 9, 255])
            .collect();
        let png = encode(350, 200, &rgba);
        let image = decode(&png).unwrap();
        assert_eq!((image.width, image.height), (350, 200));
        assert_eq!(image.rgba, rgba);
    }

    /// `sips` writes dynamic-Huffman deflate with row filters, which is what
    /// committed goldens are. Re-encode one of ours through it and read it back.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_sips_compressed_png_decodes_to_the_same_pixels() {
        let rgba: Vec<u8> = (0..200 * 120u32)
            .flat_map(|i| {
                let v = if (i / 7) % 5 == 0 {
                    0
                } else {
                    255 - (i % 200) as u8
                };
                [v, v, v, 255]
            })
            .collect();
        let dir = std::env::temp_dir().join(format!("cpl-png-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (raw, packed) = (dir.join("raw.png"), dir.join("packed.png"));
        std::fs::write(&raw, encode(200, 120, &rgba)).unwrap();
        let status = std::process::Command::new("sips")
            .args(["-s", "format", "png"])
            .arg(&raw)
            .arg("--out")
            .arg(&packed)
            .output()
            .unwrap();
        assert!(status.status.success());
        let bytes = std::fs::read(&packed).unwrap();
        assert!(bytes.len() < 200 * 120 * 4 / 4, "sips compressed it");
        assert_eq!(decode(&bytes).unwrap().rgba, rgba);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn a_tiny_image_has_the_png_shape() {
        let png = encode(2, 1, &[0, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }
}
