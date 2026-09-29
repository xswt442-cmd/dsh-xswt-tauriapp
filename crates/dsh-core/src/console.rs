//! Reading what a child process wrote to its console.
//!
//! The captured stream is not one encoding. npm writes UTF-8 and says so.
//! Windows itself writes its own messages — and `cmd.exe` writes the ones an
//! install can fail with — in the console's OEM code page. Decoding those as
//! UTF-8 produces replacement characters and a few byte pairs that happen to be
//! valid, so `cmd.exe` refusing a path arrived in the update dialog as
//! `"ϵͳ�Ҳ���…"` instead of "系统找不到指定的路径。".
//!
//! So: try UTF-8, and when that fails ask the machine which code page it writes
//! in. Lossy is the last resort, not the first — and the try is per line once a
//! capture turns out to mix them, because one writer's OEM bytes are not that
//! writer's answer about everybody else's.

/// Decode a captured stream, or make the best of it.
///
/// Whole when it can be whole: one encoding for the buffer is the normal case,
/// and splitting anything is a cost paid for a mixed stream that is usually not
/// there.
pub fn decode(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    // Line by line from here, because a capture can mix encodings: pushing the
    // *whole* buffer through one code page turns npm's UTF-8 Chinese into
    // valid-but-wrong GBK — output that reads as decoded and is not. The
    // two writers interleave by line at worst, so a line is the largest unit with
    // one encoding.
    let mut text = String::new();
    for line in lines(bytes) {
        match std::str::from_utf8(line) {
            Ok(ok) => text.push_str(ok),
            Err(_) => text.push_str(&decode_line(line)),
        }
    }
    text
}

/// The best reading of one line that is not valid UTF-8.
fn decode_line(line: &[u8]) -> String {
    oem::decode(line).unwrap_or_else(|| String::from_utf8_lossy(line).into_owned())
}

/// `bytes` in line-sized pieces, each keeping its own terminator.
fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        match rest.iter().position(|byte| *byte == b'\n') {
            Some(through) => {
                let (line, tail) = rest.split_at(through + 1);
                rest = tail;
                Some(line)
            }
            None => {
                let line = rest;
                rest = &[];
                Some(line)
            }
        }
    })
}

#[cfg(windows)]
mod oem {
    //! `MultiByteToWideChar` with `CP_OEMCP`, declared rather than depended on:
    //! this is one Win32 call used in one place, and a crate for it would be a
    //! dependency with no other purpose.

    /// `CP_OEMCP` — whatever code page this machine's console messages are in.
    const CP_OEMCP: u32 = 1;

    #[link(name = "kernel32")]
    extern "system" {
        fn MultiByteToWideChar(
            code_page: u32,
            flags: u32,
            bytes: *const u8,
            byte_count: i32,
            wide: *mut u16,
            wide_count: i32,
        ) -> i32;
    }

    /// The code page `CP_OEMCP` resolves to.
    ///
    /// Only the test needs it, so it is declared here rather than beside the
    /// call above: the test has to know which machine's bytes it can expect a
    /// word from, since the same bytes say something else on another page.
    #[cfg(test)]
    pub(super) fn code_page() -> u32 {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetOEMCP() -> u32;
        }
        unsafe { GetOEMCP() }
    }

    pub(super) fn decode(bytes: &[u8]) -> Option<String> {
        if bytes.is_empty() {
            return Some(String::new());
        }
        // A capture longer than `i32::MAX` is not a console message.
        let len = bytes.len().min(i32::MAX as usize) as i32;
        // Measure, then convert.
        let needed = unsafe {
            MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0)
        };
        if needed <= 0 {
            return None;
        }
        let mut wide = vec![0u16; needed as usize];
        let written = unsafe {
            MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), needed)
        };
        if written <= 0 {
            return None;
        }
        wide.truncate(written as usize);
        Some(String::from_utf16_lossy(&wide))
    }
}

#[cfg(not(windows))]
mod oem {
    /// Nothing to fall back to. A Unix process writes bytes, and a program whose
    /// own messages are not UTF-8 has no second encoding this side can name.
    pub(super) fn decode(_bytes: &[u8]) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_output_is_taken_as_it_comes() {
        // What npm writes, and the only kind of output that was ever readable.
        let npm = "npm error code E404\nnpm error 404 No match found for version 999.0.0\n";
        assert_eq!(decode(npm.as_bytes()), npm);
        // A path with a non-ASCII name survives too, which is the reason this
        // is not simply lossy everywhere.
        assert_eq!(
            decode("安装到 C:\\用户\\下载".as_bytes()),
            "安装到 C:\\用户\\下载"
        );
    }

    #[test]
    fn nothing_an_install_can_emit_panics_on() {
        assert_eq!(decode(b""), "");
        assert_eq!(decode(&[0xff]), decode(&[0xff]));
        // Truncated multi-byte sequences are the realistic case: a message cut
        // off by a read boundary.
        assert!(!decode(&[0xe5, 0xae]).is_empty());
    }

    #[test]
    fn a_mixed_capture_keeps_both_encodings_line_by_line() {
        // npm's own line, then the line `cmd.exe` wrote under it: the shape of a
        // failed install's captured output. Read as one buffer, those OEM bytes
        // took the first line through the code page with them, which returns
        // *valid* text in the wrong language rather than anything that looks
        // broken — the reason the whole-buffer path above is only taken once the
        // buffer is known to be UTF-8.
        const CMD_SAID_NO_SUCH_PATH: &[u8] = &[
            0xcf, 0xb5, 0xcd, 0xb3, 0xd5, 0xd2, 0xb2, 0xbb, 0xb5, 0xbd, 0xd6, 0xb8, 0xb6, 0xa8,
            0xb5, 0xc4, 0xc2, 0xb7, 0xbe, 0xb6, 0xa1, 0xa3,
        ];
        let mut capture = "npm error 安装到 C:\\用户\\下载\n".as_bytes().to_vec();
        capture.extend_from_slice(CMD_SAID_NO_SUCH_PATH);

        let decoded = decode(&capture);
        assert_eq!(
            decoded.lines().next(),
            Some("npm error 安装到 C:\\用户\\下载"),
            "the UTF-8 line is not collateral damage"
        );
        #[cfg(windows)]
        {
            assert!(!decoded.contains('\u{fffd}'), "still mojibake: {decoded:?}");
            if oem::code_page() == 936 {
                assert_eq!(decoded.lines().nth(1), Some("系统找不到指定的路径。"));
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_message_comes_back_as_words() {
        // The bytes cmd.exe wrote when an install reported this failure, and the
        // sentence they are. Windows' messages are in the console's OEM code
        // page, so what they say depends on the machine: on the page they were
        // written in they have to come back word for word, and anywhere else the
        // same call still has to produce text rather than replacement
        // characters.
        const CMD_SAID_NO_SUCH_PATH: &[u8] = &[
            0xcf, 0xb5, 0xcd, 0xb3, 0xd5, 0xd2, 0xb2, 0xbb, 0xb5, 0xbd, 0xd6, 0xb8, 0xb6, 0xa8,
            0xb5, 0xc4, 0xc2, 0xb7, 0xbe, 0xb6, 0xa1, 0xa3,
        ];
        let decoded = decode(CMD_SAID_NO_SUCH_PATH);
        assert!(!decoded.contains('\u{fffd}'), "still mojibake: {decoded:?}");
        if oem::code_page() == 936 {
            assert_eq!(decoded, "系统找不到指定的路径。");
        }
    }
}
