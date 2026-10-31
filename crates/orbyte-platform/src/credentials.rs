//! SSH passwords stored outside the application database.
//!
//! Author: jaswtao
//! Date: 2026-10-31

use zeroize::Zeroizing;

pub trait PasswordStore: Send + Sync {
    fn read(&self, connection_id: &str) -> Result<Option<Zeroizing<String>>, String>;
    fn write(&self, connection_id: &str, password: &str) -> Result<(), String>;
    fn delete(&self, connection_id: &str) -> Result<(), String>;
}

pub struct SystemPasswordStore;

#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::*;
    use zeroize::Zeroize;

    fn target(id: &str) -> Vec<u16> {
        format!("Orbyte/ssh/{id}").encode_utf16().chain(Some(0)).collect()
    }

    fn failure(action: &str) -> String {
        format!("Windows credential {action} failed ({}).", unsafe { GetLastError() })
    }

    impl PasswordStore for SystemPasswordStore {
        fn read(&self, id: &str) -> Result<Option<Zeroizing<String>>, String> {
            let name = target(id);
            let mut pointer = std::ptr::null_mut();
            // CredRead owns the returned allocation until CredFree.
            unsafe {
                if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) == 0 {
                    return if GetLastError() == ERROR_NOT_FOUND { Ok(None) } else { Err(failure("read")) };
                }
                let credential = &mut *pointer;
                let result = if credential.CredentialBlobSize == 0 {
                    Ok(Some(Zeroizing::new(String::new())))
                } else {
                    let bytes = std::slice::from_raw_parts_mut(credential.CredentialBlob, credential.CredentialBlobSize as usize);
                    let result = std::str::from_utf8(bytes)
                        .map(|value| Some(Zeroizing::new(value.to_owned())))
                        .map_err(|_| "Saved SSH password has an invalid encoding.".into());
                    bytes.zeroize();
                    result
                };
                CredFree(pointer.cast());
                result
            }
        }

        fn write(&self, id: &str, password: &str) -> Result<(), String> {
            if password.is_empty() || password.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
                return Err("Saved SSH password must contain 1–2560 UTF-8 bytes.".into());
            }
            let mut name = target(id);
            let mut username: Vec<u16> = "Orbyte".encode_utf16().chain(Some(0)).collect();
            let mut bytes = Zeroizing::new(password.as_bytes().to_vec());
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: name.as_mut_ptr(),
                CredentialBlobSize: bytes.len() as u32,
                CredentialBlob: bytes.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                UserName: username.as_mut_ptr(),
                ..Default::default()
            };
            if unsafe { CredWriteW(&credential, 0) } == 0 { return Err(failure("write")); }
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<(), String> {
            let name = target(id);
            if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0
                && unsafe { GetLastError() } != ERROR_NOT_FOUND {
                return Err(failure("delete"));
            }
            Ok(())
        }
    }
}

#[cfg(not(windows))]
impl PasswordStore for SystemPasswordStore {
    fn read(&self, _: &str) -> Result<Option<Zeroizing<String>>, String> { Ok(None) }
    fn write(&self, _: &str, _: &str) -> Result<(), String> {
        Err("Saving SSH passwords is currently supported only on Windows.".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> { Ok(()) }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes an isolated entry in Windows Credential Manager"]
    fn system_password_roundtrip() {
        let id = format!("test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) { let _ = SystemPasswordStore.delete(&self.0); }
        }
        let _cleanup = Cleanup(id.clone());
        let store = SystemPasswordStore;
        assert!(store.read(&id).unwrap().is_none());
        store.write(&id, "  密码-test  ").unwrap();
        assert_eq!(store.read(&id).unwrap().as_deref().map(|s| s.as_str()), Some("  密码-test  "));
        store.write(&id, "replacement").unwrap();
        assert_eq!(store.read(&id).unwrap().as_deref().map(|s| s.as_str()), Some("replacement"));
        assert!(store.write(&id, &"a".repeat(2561)).is_err());
        store.delete(&id).unwrap();
        store.delete(&id).unwrap();
        assert!(store.read(&id).unwrap().is_none());
    }
}
