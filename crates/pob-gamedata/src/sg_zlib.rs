//! SimpleGraphic's zlib host functions `Inflate(data)` and `Deflate(data)` (`ui_api.cpp`
//! `l_Inflate` / `l_Deflate`), the one port every caller shares (FUP7): the timeless jewel LUT
//! loader (DataJewelFileLoader.lua:84, [`crate::timeless_lut`]), the build codes of ImportTab,
//! PartyTab and CompareTab (`pob-app`), and the LuaJIT oracle's natives (`pob-oracle` `jit.rs`
//! `install_natives`), so both sides of a parity run inflate and deflate alike.
//!
//! `l_Inflate`: `inflateInit(&z)` (a zlib stream, 15-bit window), an output buffer of
//! `inLen * 4` bytes doubled whenever `inflate(&z, Z_NO_FLUSH)` fills it, until `inflate` stops
//! returning `Z_OK`; the bytes when it returned `Z_STREAM_END` (trailing input is ignored),
//! otherwise `nil, zError(err)`. `l_Deflate`: `deflateInit(&z, 9)`, one `deflate(&z, Z_FINISH)`
//! into a `deflateBound` buffer.
//!
//! DIVERGENCE(deflate-bytes): [`Deflate`] is miniz_oxide at level 9, whose compressed bytes
//! differ from zlib's. Both inflaters accept both deflaters' output, so a code still decodes to
//! the same XML; only the code text differs from the one PoB would show.
//!
//! Pure Rust (miniz_oxide), wasm-safe.

use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

/// zlib's `zError(err)` for the codes `inflate` can return to `l_Inflate`.
fn zError(err: MZError) -> &'static str {
    match err {
        // Z_ERRNO
        MZError::ErrNo => "file error",
        // Z_STREAM_ERROR
        MZError::Stream => "stream error",
        // Z_DATA_ERROR
        MZError::Data => "data error",
        // Z_MEM_ERROR
        MZError::Mem => "insufficient memory",
        // Z_BUF_ERROR
        MZError::Buf => "buffer error",
        // Z_VERSION_ERROR
        MZError::Version => "incompatible version",
        // Z_PARAM_ERROR is not a zlib return code of inflate; zError(-10000) is ""
        MZError::Param => "",
    }
}

/// `Inflate(data)`: `Ok(bytes)` when the zlib stream reaches `Z_STREAM_END`, `Err(zError(err))`
/// (Lua's `nil, msg`) otherwise: a truncated stream (`Z_BUF_ERROR`), a corrupt one or a bad
/// checksum (`Z_DATA_ERROR`), a preset dictionary (`Z_NEED_DICT`).
pub fn Inflate(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut z = InflateState::new_boxed(DataFormat::Zlib);
    // int outSz = inLen * 4;
    let mut out_sz = data.len() * 4;
    let mut out = vec![0u8; out_sz];
    let (mut total_in, mut total_out) = (0usize, 0usize);
    // while ((err = inflate(&z, Z_NO_FLUSH)) == Z_OK)
    loop {
        let r = inflate(
            &mut z,
            &data[total_in..],
            &mut out[total_out..],
            MZFlush::None,
        );
        total_in += r.bytes_consumed;
        total_out += r.bytes_written;
        match r.status {
            // zlib never returns Z_OK without progress (that is Z_BUF_ERROR)
            Ok(MZStatus::Ok) if r.bytes_consumed != 0 || r.bytes_written != 0 => {
                if total_out == out_sz {
                    // Output buffer filled, embiggen it
                    let new_sz = out_sz << 1;
                    out.resize(new_sz, 0);
                    out_sz = new_sz;
                }
            }
            Ok(MZStatus::Ok) => return Err(zError(MZError::Buf)),
            Ok(MZStatus::StreamEnd) => {
                out.truncate(total_out);
                return Ok(out);
            }
            // Z_NEED_DICT
            Ok(MZStatus::NeedDict) => return Err("need dictionary"),
            Err(e) => return Err(zError(e)),
        }
    }
}

/// `Deflate(data)`: a zlib stream at level 9. DIVERGENCE(deflate-bytes) (module notes).
pub fn Deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflate_follows_simplegraphic() {
        let z = Deflate(b"<PathOfBuilding/>");
        assert_eq!(&z[..2], b"\x78\xda", "level 9 header");
        assert_eq!(Inflate(&z).as_deref(), Ok(&b"<PathOfBuilding/>"[..]));
        // a truncated stream never reaches Z_STREAM_END: nil
        assert_eq!(Inflate(&z[..z.len() - 3]), Err("buffer error"));
        // not a zlib stream / a bad checksum: nil
        assert_eq!(Inflate(b"hello"), Err("data error"));
        let mut bad = z.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(Inflate(&bad), Err("data error"));
        // no input: outSz 0, inflate makes no progress
        assert_eq!(Inflate(b""), Err("buffer error"));
        // trailing bytes after the stream end are ignored
        let mut t = z.clone();
        t.extend_from_slice(b"junk");
        assert_eq!(Inflate(&t).as_deref(), Ok(&b"<PathOfBuilding/>"[..]));
        // the output buffer is doubled until the stream ends (here 4x the input is far too small)
        let big = vec![b'a'; 1 << 20];
        assert_eq!(Inflate(&Deflate(&big)).as_deref(), Ok(&big[..]));
        assert_eq!(Inflate(&Deflate(b"")).as_deref(), Ok(&b""[..]));
    }
}
