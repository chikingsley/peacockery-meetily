//! Windows DPAPI protects the Outlook session for the current Windows user.
//! JWTs exceed Credential Manager's single-entry size limit.
#[cfg(windows)]
pub fn protect(data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).map_err(|_| "Session is too large.")?,
        pbData: data.as_ptr().cast_mut(),
    };
    let entropy_bytes = b"studio.peacockery.meetily/outlook-calendar/v1";
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy_bytes.len() as u32,
        pbData: entropy_bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI allocates output with LocalAlloc. Copy it, then always free it.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err("Windows session storage failed. Reconnect Outlook.".into());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Ok(bytes)
}
#[cfg(not(windows))]
pub fn protect(_: &[u8], _: bool) -> Result<Vec<u8>, String> {
    Err("Outlook session storage is available in the Windows app.".into())
}
#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn protects_large_sessions_and_rejects_corruption() {
        let data = vec![b'x'; 16000];
        let sealed = super::protect(&data, true).unwrap();
        assert_ne!(sealed, data);
        assert_eq!(super::protect(&sealed, false).unwrap(), data);
        assert!(super::protect(b"invalid session", false).is_err());
    }
}
