//! TGA decoder for SimpleGraphic font atlases (ui doc §3 `font/tga.rs`: RLE/raw 32-bpp TGA). Owner: U2.
//! Moved from the pre-port `pob-sg/src/tga.rs` by UI-U0.
//!
//! Port of `engine/core/core_image.cpp` `targa_c::Load` (SimpleGraphic
//! master @ 8694a07):
//! - colour-mapped images are rejected;
//! - supported (type & 7, depth): (3, 8) grey, (2, 24) RGB, (2, 32) RGBA;
//!   bit 3 of the image type selects RLE;
//! - `flipV = !(descriptor & 0x20)`: rows are stored bottom-up unless the
//!   top-left-origin bit is set (the shipped font atlases use both: e.g.
//!   `Liberation Sans.16.tga` has descriptor 0x00, `Liberation Sans.32.tga`
//!   has 0x28);
//! - BGR(A) is swapped to RGB(A).
//!
//! Output is always top-down RGBA8 (grey expands to `v,v,v,255`, RGB gets
//! alpha 255), which is what the GL upload ends up sampling.

/// Decoded image, top-down RGBA8.
#[derive(Clone, Debug)]
pub struct Rgba8Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Header fields needed for metrics without decoding pixel data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TgaHeader {
    pub id_len: u8,
    pub color_map_type: u8,
    pub image_type: u8,
    pub width: u16,
    pub height: u16,
    pub depth: u8,
    pub descriptor: u8,
}

pub fn read_header(data: &[u8]) -> Result<TgaHeader, String> {
    if data.len() < 18 {
        return Err("TGA: couldn't read header".into());
    }
    let u16le = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
    Ok(TgaHeader {
        id_len: data[0],
        color_map_type: data[1],
        image_type: data[2],
        width: u16le(12),
        height: u16le(14),
        depth: data[16],
        descriptor: data[17],
    })
}

pub fn decode(data: &[u8]) -> Result<Rgba8Image, String> {
    let hdr = read_header(data)?;
    if hdr.color_map_type != 0 {
        return Err("TGA: color mapped images not supported".into());
    }
    let comp: usize = match (hdr.image_type & 7, hdr.depth) {
        (3, 8) => 1,
        (2, 24) => 3,
        (2, 32) => 4,
        (it, d) => return Err(format!("TGA: unsupported image type (it: {it} pd: {d})")),
    };
    let width = hdr.width as usize;
    let height = hdr.height as usize;
    let row_size = width * comp;
    let mut dat = vec![0u8; row_size * height];
    let flip_v = hdr.descriptor & 0x20 == 0;
    let mut pos = 18 + hdr.id_len as usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = data
            .get(pos..pos + n)
            .ok_or_else(|| "TGA: unexpected end of data".to_owned())?;
        pos += n;
        Ok(s)
    };
    if hdr.image_type & 8 != 0 {
        // RLE: packets never straddle rows (SG rejects "overlong row").
        for row in 0..height {
            let base = (if flip_v { height - row - 1 } else { row }) * row_size;
            let mut x = 0;
            while x < row_size {
                let rle_hdr = take(1)?[0];
                let rlen = ((rle_hdr & 0x7f) as usize + 1) * comp;
                if x + rlen > row_size {
                    return Err("TGA: invalid RLE coding (overlong row)".into());
                }
                if rle_hdr & 0x80 != 0 {
                    let px = take(comp)?;
                    for c in 0..rlen {
                        dat[base + x + c] = px[c % comp];
                    }
                } else {
                    dat[base + x..base + x + rlen].copy_from_slice(take(rlen)?);
                }
                x += rlen;
            }
        }
    } else {
        for row in 0..height {
            let dst = if flip_v { height - row - 1 } else { row };
            dat[dst * row_size..(dst + 1) * row_size].copy_from_slice(take(row_size)?);
        }
    }
    let mut pixels = Vec::with_capacity(width * height * 4);
    for px in dat.chunks_exact(comp) {
        match comp {
            1 => pixels.extend_from_slice(&[px[0], px[0], px[0], 255]),
            3 => pixels.extend_from_slice(&[px[2], px[1], px[0], 255]),
            _ => pixels.extend_from_slice(&[px[2], px[1], px[0], px[3]]),
        }
    }
    Ok(Rgba8Image {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(it: u8, w: u16, h: u16, depth: u8, desc: u8) -> Vec<u8> {
        let mut v = vec![0u8; 18];
        v[2] = it;
        v[12..14].copy_from_slice(&w.to_le_bytes());
        v[14..16].copy_from_slice(&h.to_le_bytes());
        v[16] = depth;
        v[17] = desc;
        v
    }

    #[test]
    fn rle_bottom_up() {
        // 2x2 RLE 32bpp, bottom-up: first row in file is the bottom row.
        let mut d = header(10, 2, 2, 32, 0);
        // bottom row: run of 2 BGRA (1,2,3,4)
        d.extend_from_slice(&[0x81, 1, 2, 3, 4]);
        // top row: raw packet of 2 pixels
        d.extend_from_slice(&[0x01, 10, 20, 30, 40, 50, 60, 70, 80]);
        let img = decode(&d).unwrap();
        assert_eq!(img.pixels[0..8], [30, 20, 10, 40, 70, 60, 50, 80]);
        assert_eq!(img.pixels[8..16], [3, 2, 1, 4, 3, 2, 1, 4]);
    }

    #[test]
    fn raw_top_down_grey() {
        let mut d = header(3, 2, 1, 8, 0x20);
        d.extend_from_slice(&[7, 9]);
        let img = decode(&d).unwrap();
        assert_eq!(img.pixels, vec![7, 7, 7, 255, 9, 9, 9, 255]);
    }

    #[test]
    fn overlong_rle_rejected() {
        let mut d = header(10, 1, 1, 32, 0);
        d.extend_from_slice(&[0x81, 1, 2, 3, 4]);
        assert!(decode(&d).is_err());
    }
}
