#[cfg(target_os = "linux")]
use std::io::Write;

#[cfg(windows)]
const TARGET: &str = "gemini:antigravity";

#[cfg(windows)]
pub(super) fn check_size(value: &str) -> Result<(), String> {
    // CRED_MAX_CREDENTIAL_BLOB_SIZE from wincred.h.
    if value.len() > 5 * 512 {
        return Err("accountError.write".into());
    }
    Ok(())
}

#[cfg(windows)]
mod windows_credential {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct Credential {
        flags: u32,
        kind: u32,
        target: *const u16,
        comment: *const u16,
        written: FileTime,
        blob_size: u32,
        blob: *const u8,
        persist: u32,
        attribute_count: u32,
        attributes: *const c_void,
        alias: *const u16,
        username: *const u16,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn CredReadW(target: *const u16, kind: u32, flags: u32, value: *mut *mut Credential)
            -> i32;
        fn CredWriteW(value: *const Credential, flags: u32) -> i32;
        fn CredDeleteW(target: *const u16, kind: u32, flags: u32) -> i32;
        fn CredFree(value: *mut c_void);
    }

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub(super) fn read() -> Result<Option<String>, String> {
        let target = wide(super::TARGET);
        let mut ptr: *mut Credential = ptr::null_mut();
        if unsafe { CredReadW(target.as_ptr(), 1, 0, &mut ptr) } == 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(1168) {
                Ok(None)
            } else {
                Err("accountError.keychain".into())
            };
        }
        if ptr.is_null() {
            return Err("accountError.keychain".into());
        }
        let bytes = unsafe {
            let item = &*ptr;
            if item.blob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(item.blob, item.blob_size as usize).to_vec()
            }
        };
        unsafe { CredFree(ptr.cast()) };
        if bytes.is_empty() {
            return Ok(None);
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "accountError.format".into())
    }

    pub(super) fn write(value: &str) -> Result<(), String> {
        super::check_size(value)?;
        let target = wide(super::TARGET);
        let user = wide("antigravity");
        let blob = value.as_bytes();
        let credential = Credential {
            flags: 0,
            kind: 1,
            target: target.as_ptr(),
            comment: ptr::null(),
            written: FileTime { low: 0, high: 0 },
            blob_size: blob.len().try_into().map_err(|_| "accountError.write")?,
            blob: blob.as_ptr(),
            persist: 2,
            attribute_count: 0,
            attributes: ptr::null(),
            alias: ptr::null(),
            username: user.as_ptr(),
        };
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err("accountError.keychain".into());
        }
        Ok(())
    }

    pub(super) fn delete() -> Result<(), String> {
        let target = wide(super::TARGET);
        if unsafe { CredDeleteW(target.as_ptr(), 1, 0) } == 0 {
            return Err("accountError.keychain".into());
        }
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::check_size;

    #[test]
    fn windows_credential_limit_counts_utf8_bytes() {
        assert!(check_size(&"é".repeat(1280)).is_ok());
        assert_eq!(
            check_size(&"é".repeat(1281)),
            Err("accountError.write".into())
        );
    }
}

pub(super) fn read() -> Result<Option<String>, String> {
    #[cfg(windows)]
    {
        windows_credential::read()
    }
    #[cfg(target_os = "macos")]
    {
        let output = crate::utils::process::command("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                "gemini",
                "-a",
                "antigravity",
                "-w",
            ])
            .output()
            .map_err(|_| "accountError.keychain")?;
        if !output.status.success() {
            return Ok(None);
        }
        let value = String::from_utf8(output.stdout).map_err(|_| "accountError.format")?;
        let value = value.trim();
        let value = value.strip_prefix("go-keyring-base64:").unwrap_or(value);
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|_| "accountError.format")?;
        String::from_utf8(decoded)
            .map(Some)
            .map_err(|_| "accountError.format".into())
    }
    #[cfg(target_os = "linux")]
    {
        let output = crate::utils::process::command("secret-tool")
            .args(["lookup", "service", "gemini", "username", "antigravity"])
            .output()
            .map_err(|_| "accountError.keychain")?;
        if !output.status.success() {
            return Ok(None);
        }
        let value = String::from_utf8(output.stdout).map_err(|_| "accountError.format")?;
        Ok(Some(value.trim().to_string()))
    }
}

pub(super) fn write(value: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows_credential::write(value)
    }
    #[cfg(target_os = "macos")]
    {
        use base64::Engine;
        let encoded = format!(
            "go-keyring-base64:{}",
            base64::engine::general_purpose::STANDARD.encode(value)
        );
        let status = crate::utils::process::command("/usr/bin/security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                "gemini",
                "-a",
                "antigravity",
                "-w",
                &encoded,
            ])
            .status()
            .map_err(|_| "accountError.keychain")?;
        if !status.success() {
            return Err("accountError.keychain".into());
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        use std::process::Stdio;
        let mut child = crate::utils::process::command("secret-tool")
            .args([
                "store",
                "--label=Antigravity",
                "service",
                "gemini",
                "username",
                "antigravity",
            ])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|_| "accountError.keychain")?;
        child
            .stdin
            .take()
            .ok_or("accountError.keychain")?
            .write_all(value.as_bytes())
            .map_err(|_| "accountError.keychain")?;
        if !child.wait().map_err(|_| "accountError.keychain")?.success() {
            return Err("accountError.keychain".into());
        }
        Ok(())
    }
}

pub(super) fn delete() -> Result<(), String> {
    #[cfg(windows)]
    {
        windows_credential::delete()
    }
    #[cfg(target_os = "macos")]
    {
        let status = crate::utils::process::command("/usr/bin/security")
            .args([
                "delete-generic-password",
                "-s",
                "gemini",
                "-a",
                "antigravity",
            ])
            .status()
            .map_err(|_| "accountError.keychain")?;
        if !status.success() {
            return Err("accountError.keychain".into());
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let status = crate::utils::process::command("secret-tool")
            .args(["clear", "service", "gemini", "username", "antigravity"])
            .status()
            .map_err(|_| "accountError.keychain")?;
        if !status.success() {
            return Err("accountError.keychain".into());
        }
        Ok(())
    }
}
