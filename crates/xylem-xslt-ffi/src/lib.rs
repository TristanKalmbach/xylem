//! A C ABI for Xylem's XSLT engine, so other applications can run
//! stylesheets without Saxon, .NET or Java: compile a stylesheet once, then
//! transform any number of inputs with it.
//!
//! All strings are UTF-8. No panic crosses the C boundary: a failure inside
//! the engine becomes an error return with a message. Every function is
//! safe to call from any thread, and one compiled stylesheet can run any
//! number of transformations at once on different threads. See
//! `docs/embedding.md` for a C# example.
//!
//! ```c
//! XylemStylesheet *ss = xylem_xslt_compile("C:/styles/book-to-fo.xsl");
//! if (!ss) { puts(xylem_xslt_last_error()); return; }
//! uint8_t *out; size_t out_len;
//! int rc = xylem_xslt_transform(ss, input, input_len, "{\"mode\":\"final\"}",
//!                               XYLEM_OUTPUT_PUBLISH, &out, &out_len);
//! if (rc == 0) { /* use out[0..out_len] */ xylem_xslt_free_bytes(out, out_len); }
//! xylem_xslt_free(ss);
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use xylem_xslt_engine::{Stylesheet, transform_bytes, with_file_access};

/// Write the result the way a publishing service saves it: the
/// `XmlDocument.OuterXml` form (no XML declaration, `xsl:output` ignored).
/// A result that is not one well-formed document (top-level text, or more
/// than one top-level element) fails, as it does there.
pub const XYLEM_OUTPUT_PUBLISH: u32 = 0;
/// Write the result as the stylesheet's `xsl:output` asks (for HTML steps).
pub const XYLEM_OUTPUT_SERIALIZE: u32 = 1;
/// Do not let the transformation read files: `document()` and external
/// entities in the input fail instead. Stylesheet modules are still read.
pub const XYLEM_NO_FILE_ACCESS: u32 = 2;

/// A compiled stylesheet. Opaque to callers. It holds no per-run state,
/// so transformations on one handle run concurrently.
pub struct XylemStylesheet {
    inner: Stylesheet,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
    static LAST_LOG: RefCell<CString> = RefCell::new(CString::default());
}

fn set_log(messages: &[String], warnings: &[String]) {
    let s = serde_json::json!({"messages": messages, "warnings": warnings}).to_string();
    LAST_LOG.with(|l| *l.borrow_mut() = CString::new(s.replace('\0', " ")).unwrap_or_default());
}

/// Runs `f`, turning a panic into `on_panic` with the panic's message as the
/// last error, so no unwinding ever reaches the caller's frames.
fn guarded<T>(what: &str, on_panic: T, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            if detail.is_empty() {
                set_error(format!("the engine failed unexpectedly while {what}"));
            } else {
                set_error(format!(
                    "the engine failed unexpectedly while {what}: {detail}"
                ));
            }
            on_panic
        }
    }
}

fn set_error(e: impl Into<String>) {
    let s = e.into().replace('\0', " ");
    LAST_ERROR.with(|l| *l.borrow_mut() = CString::new(s).unwrap_or_default());
}

/// The last error on this thread, valid until the next call on it.
#[unsafe(no_mangle)]
pub extern "C" fn xylem_xslt_last_error() -> *const c_char {
    guarded("reading the last error", std::ptr::null(), || {
        LAST_ERROR.with(|l| l.borrow().as_ptr())
    })
}

/// What the last successful transformation on this thread reported besides
/// its result, as JSON: `{"messages":[...],"warnings":[...]}` with the
/// `xsl:message` texts and warnings about errors it recovered from (such as
/// a `document()` that could not be read). Valid until the next call on
/// this thread.
#[unsafe(no_mangle)]
pub extern "C" fn xylem_xslt_last_log() -> *const c_char {
    guarded("reading the last log", std::ptr::null(), || {
        LAST_LOG.with(|l| l.borrow().as_ptr())
    })
}

/// The library version, such as `0.5.0`.
#[unsafe(no_mangle)]
pub extern "C" fn xylem_xslt_version() -> *const c_char {
    static VERSION: &CStr =
        match CStr::from_bytes_with_nul(concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes()) {
            Ok(v) => v,
            Err(_) => c"",
        };
    VERSION.as_ptr()
}

/// Compiles the stylesheet at `path` (and everything it includes or
/// imports). Returns null on failure; see [`xylem_xslt_last_error`].
///
/// # Safety
/// `path` must be a valid, NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xylem_xslt_compile(path: *const c_char) -> *mut XylemStylesheet {
    guarded("compiling", std::ptr::null_mut(), || {
        if path.is_null() {
            set_error("path is null");
            return std::ptr::null_mut();
        }
        // SAFETY: the caller passes a NUL-terminated string.
        let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
            set_error("path is not UTF-8");
            return std::ptr::null_mut();
        };
        match Stylesheet::compile(Path::new(path)) {
            Ok(ss) => Box::into_raw(Box::new(XylemStylesheet { inner: ss })),
            Err(e) => {
                set_error(e);
                std::ptr::null_mut()
            }
        }
    })
}

/// The XSLT version the stylesheet runs as: 1 or 2 (0 for a null handle).
///
/// # Safety
/// `ss` must be null or a handle from [`xylem_xslt_compile`] not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xylem_xslt_version_of(ss: *const XylemStylesheet) -> u32 {
    guarded("reading the version", 0, || {
        // SAFETY: see above.
        let Some(ss) = (unsafe { ss.as_ref() }) else {
            return 0;
        };
        if ss.inner.version.starts_with('1') {
            1
        } else {
            2
        }
    })
}

/// Transforms `input` (an XML document, `input_len` bytes) with `ss`.
///
/// `params_json` is null or a JSON object of stylesheet parameters, such as
/// `{"mode":"final","pageCount":"0"}`; each sets the global `xsl:param` of
/// exactly that name (values are converted to a declared boolean or numeric
/// type, and are strings otherwise). `flags` combines the `XYLEM_*`
/// constants. On success returns 0 and sets `*out`/`*out_len` to the UTF-8
/// result, which the caller frees with [`xylem_xslt_free_bytes`]; on
/// failure returns non-zero (see [`xylem_xslt_last_error`]): 1 for bad
/// arguments, 2 when the stylesheet or input fails, 3 when the engine
/// itself fails unexpectedly (an engine bug; the handle stays usable).
///
/// # Safety
/// `ss` must be a live handle; `input` must point to `input_len` readable
/// bytes; `params_json` must be null or NUL-terminated; `out` and `out_len`
/// must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xylem_xslt_transform(
    ss: *const XylemStylesheet,
    input: *const u8,
    input_len: usize,
    params_json: *const c_char,
    flags: u32,
    out: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    guarded("transforming", 3, || {
        // SAFETY: the caller upholds the pointer contracts above.
        let (Some(ss), false, false) = (unsafe { ss.as_ref() }, out.is_null(), out_len.is_null())
        else {
            set_error("null handle or output pointer");
            return 1;
        };
        if input.is_null() && input_len > 0 {
            set_error("input is null");
            return 1;
        }
        let bytes: &[u8] = if input_len == 0 {
            &[]
        } else {
            // SAFETY: `input` points to `input_len` readable bytes.
            unsafe { std::slice::from_raw_parts(input, input_len) }
        };
        let params: HashMap<String, String> = if params_json.is_null() {
            HashMap::new()
        } else {
            // SAFETY: NUL-terminated per the contract.
            let text = unsafe { CStr::from_ptr(params_json) }.to_string_lossy();
            match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text) {
                Ok(m) => m
                    .into_iter()
                    .map(|(k, v)| {
                        let v = match v {
                            serde_json::Value::String(s) => s,
                            other => other.to_string(),
                        };
                        (k, v)
                    })
                    .collect(),
                Err(e) => {
                    set_error(format!("parameters are not a JSON object: {e}"));
                    return 1;
                }
            }
        };
        // A panic here unwinds to `guarded`, which returns 3; the handle
        // keeps no state from the run, so it stays usable.
        let result = with_file_access(flags & XYLEM_NO_FILE_ACCESS == 0, || {
            transform_bytes(&ss.inner, bytes, &params, false)
        });
        if let Ok(o) = &result {
            set_log(&o.messages, &o.warnings);
        }
        let text = match result {
            Ok(o) if !o.results.is_empty() => {
                set_error(
                    "the stylesheet writes secondary results (xsl:result-document), which the C API does not return",
                );
                return 2;
            }
            Ok(o) if flags & XYLEM_OUTPUT_SERIALIZE != 0 => o.serialized,
            Ok(o) => match o.published() {
                Ok(_) => o.text,
                Err(e) => {
                    set_error(e);
                    return 2;
                }
            },
            Err(e) => {
                set_error(e);
                return 2;
            }
        };
        let mut v = text.into_bytes().into_boxed_slice();
        // SAFETY: both pointers were checked for null above.
        unsafe {
            *out_len = v.len();
            *out = v.as_mut_ptr();
        }
        std::mem::forget(v);
        0
    })
}

/// Frees a result from [`xylem_xslt_transform`].
///
/// # Safety
/// `ptr`/`len` must be exactly what a transform returned, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xylem_xslt_free_bytes(ptr: *mut u8, len: usize) {
    guarded("freeing a result", (), || {
        if !ptr.is_null() {
            // SAFETY: rebuilt from the boxed slice handed out by transform.
            drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
        }
    })
}

/// Frees a compiled stylesheet.
///
/// # Safety
/// `ss` must be null or a live handle, freed once, with no call using it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xylem_xslt_free(ss: *mut XylemStylesheet) {
    guarded("freeing a stylesheet", (), || {
        if !ss.is_null() {
            // SAFETY: created by Box::into_raw in compile.
            drop(unsafe { Box::from_raw(ss) });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_become_errors() {
        let r = guarded("testing", 7, || -> i32 { panic!("boom {}", 1) });
        assert_eq!(r, 7);
        let err = unsafe { CStr::from_ptr(xylem_xslt_last_error()) }
            .to_string_lossy()
            .to_string();
        assert_eq!(err, "the engine failed unexpectedly while testing: boom 1");
        let r = guarded("testing", 0, || -> i32 { std::panic::panic_any(5u8) });
        assert_eq!(r, 0);
    }

    #[test]
    fn one_handle_serves_many_threads_at_once() {
        let dir = std::env::temp_dir().join(format!("xylem-ffi-threads-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xsl = dir.join("s.xsl");
        std::fs::write(
            &xsl,
            r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:param name="n"/><xsl:template match="/"><r n="{$n}"><xsl:value-of select="sum(for $i in 1 to 20000 return $i) + count(//a)"/></r></xsl:template></xsl:stylesheet>"#,
        )
        .unwrap();
        let path = CString::new(xsl.to_str().unwrap()).unwrap();
        let ss = unsafe { xylem_xslt_compile(path.as_ptr()) };
        assert!(!ss.is_null());
        // Raw pointers are not Send; the handle is shared by address.
        let addr = ss as usize;
        let started = std::time::Instant::now();
        std::thread::scope(|scope| {
            for t in 0..8 {
                scope.spawn(move || {
                    let ss = addr as *const XylemStylesheet;
                    for k in 0..20 {
                        let input = format!("<d>{}</d>", "<a/>".repeat(t + k));
                        let params = CString::new(format!(r#"{{"n":"{t}-{k}"}}"#)).unwrap();
                        let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
                        let rc = unsafe {
                            xylem_xslt_transform(
                                ss,
                                input.as_ptr(),
                                input.len(),
                                params.as_ptr(),
                                0,
                                &mut out,
                                &mut len,
                            )
                        };
                        assert_eq!(rc, 0);
                        let text = unsafe { std::slice::from_raw_parts(out, len) }.to_vec();
                        unsafe { xylem_xslt_free_bytes(out, len) };
                        let want = format!(r#"<r n="{t}-{k}">{}</r>"#, 200010000 + t + k);
                        assert_eq!(String::from_utf8(text).unwrap(), want);
                    }
                });
            }
        });
        eprintln!("8 threads x 20 transforms: {:?}", started.elapsed());
        unsafe { xylem_xslt_free(ss) };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_needs_one_well_formed_document() {
        let dir = std::env::temp_dir().join(format!("xylem-ffi-doc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xsl = dir.join("s.xsl");
        std::fs::write(
            &xsl,
            r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/">text<r/></xsl:template></xsl:stylesheet>"#,
        )
        .unwrap();
        let path = CString::new(xsl.to_str().unwrap()).unwrap();
        unsafe {
            let ss = xylem_xslt_compile(path.as_ptr());
            let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
            let rc = xylem_xslt_transform(
                ss,
                b"<a/>".as_ptr(),
                4,
                std::ptr::null(),
                0,
                &mut out,
                &mut len,
            );
            assert_eq!(rc, 2);
            let err = CStr::from_ptr(xylem_xslt_last_error())
                .to_string_lossy()
                .to_string();
            assert!(err.starts_with("Token Text in state Start"), "{err}");
            // Serialized output is a fragment and is allowed.
            let rc = xylem_xslt_transform(
                ss,
                b"<a/>".as_ptr(),
                4,
                std::ptr::null(),
                XYLEM_OUTPUT_SERIALIZE,
                &mut out,
                &mut len,
            );
            assert_eq!(rc, 0);
            xylem_xslt_free_bytes(out, len);
            xylem_xslt_free(ss);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn last_log_reports_messages_and_warnings() {
        let dir = std::env::temp_dir().join(format!("xylem-ffi-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xsl = dir.join("s.xsl");
        std::fs::write(
            &xsl,
            r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/"><xsl:message>hi</xsl:message><r n="{count(document('missing.xml'))}"/></xsl:template></xsl:stylesheet>"#,
        )
        .unwrap();
        let path = CString::new(xsl.to_str().unwrap()).unwrap();
        unsafe {
            let ss = xylem_xslt_compile(path.as_ptr());
            let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
            let rc = xylem_xslt_transform(
                ss,
                b"<a/>".as_ptr(),
                4,
                std::ptr::null(),
                0,
                &mut out,
                &mut len,
            );
            assert_eq!(rc, 0);
            xylem_xslt_free_bytes(out, len);
            let log = CStr::from_ptr(xylem_xslt_last_log())
                .to_string_lossy()
                .to_string();
            let v: serde_json::Value = serde_json::from_str(&log).unwrap();
            assert_eq!(v["messages"][0], "hi");
            assert!(
                v["warnings"][0].as_str().unwrap().starts_with("FODC0002"),
                "{log}"
            );
            xylem_xslt_free(ss);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_transform_free() {
        let dir = std::env::temp_dir().join(format!("xylem-ffi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xsl = dir.join("s.xsl");
        std::fs::write(
            &xsl,
            r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:param name="who"/><xsl:template match="/"><r who="{$who}"><xsl:value-of select="count(//a)"/><xsl:copy-of select="document('missing.xml')"/></r></xsl:template></xsl:stylesheet>"#,
        )
        .unwrap();
        let path = CString::new(xsl.to_str().unwrap()).unwrap();
        unsafe {
            let ss = xylem_xslt_compile(path.as_ptr());
            assert!(!ss.is_null());
            assert_eq!(xylem_xslt_version_of(ss), 2);
            let input = b"<d><a/><a/></d>";
            let params = CString::new(r#"{"who":"me"}"#).unwrap();
            let (mut out, mut len) = (std::ptr::null_mut(), 0usize);
            let rc = xylem_xslt_transform(
                ss,
                input.as_ptr(),
                input.len(),
                params.as_ptr(),
                0,
                &mut out,
                &mut len,
            );
            assert_eq!(rc, 0);
            let text = std::str::from_utf8(std::slice::from_raw_parts(out, len))
                .unwrap()
                .to_string();
            xylem_xslt_free_bytes(out, len);
            assert_eq!(text, r#"<r who="me">2</r>"#);
            // With file access off, document() fails.
            let rc = xylem_xslt_transform(
                ss,
                input.as_ptr(),
                input.len(),
                std::ptr::null(),
                XYLEM_NO_FILE_ACCESS,
                &mut out,
                &mut len,
            );
            assert_eq!(rc, 2);
            let err = CStr::from_ptr(xylem_xslt_last_error())
                .to_string_lossy()
                .to_string();
            assert!(err.contains("file access is turned off"), "{err}");
            xylem_xslt_free(ss);
            let bad = CString::new(dir.join("missing.xsl").to_str().unwrap()).unwrap();
            assert!(xylem_xslt_compile(bad.as_ptr()).is_null());
            assert!(
                !CStr::from_ptr(xylem_xslt_last_error())
                    .to_bytes()
                    .is_empty()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
