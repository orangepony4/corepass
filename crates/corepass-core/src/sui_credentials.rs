//! S-UI API tokens in the current Windows user's Credential Manager.
//!
//! Only the fixed Corepass target namespace is accessible. Tokens are never
//! serialized to application files, exposed in errors, or passed to a process.

use zeroize::Zeroizing;

const TARGET_PREFIX: &str = "CorepassNext/SUI/";
const MAX_TOKEN_BYTES: usize = 2048;

fn target_name(panel_id: &str) -> Result<String, String> {
    if panel_id.is_empty()
        || panel_id.len() > 64
        || !panel_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("CP-SUI-VAULT-ID: Некорректный идентификатор панели".into());
    }
    Ok(format!("{TARGET_PREFIX}{panel_id}"))
}

fn validate_token(token: &[u8]) -> Result<(), String> {
    if token.is_empty()
        || token.len() > MAX_TOKEN_BYTES
        || !token.iter().all(|byte| (0x21..=0x7e).contains(byte))
    {
        return Err(
            "CP-SUI-VAULT-TOKEN: Нужен API-токен S-UI без пробелов и управляющих знаков".into(),
        );
    }
    Ok(())
}

/// Whether a valid token is stored for this exact local panel ID.
/// No token value is retained by this check.
pub fn has(panel_id: &str) -> bool {
    load(panel_id).is_ok()
}

/// Save a token under the current user's Windows Credential Manager identity.
/// PERSIST_LOCAL_MACHINE persists across logons for that user; it does not make
/// the credential available to all users of the machine.
pub fn save(panel_id: &str, token: &str) -> Result<(), String> {
    let target = target_name(panel_id)?;
    validate_token(token.as_bytes())?;
    #[cfg(windows)]
    {
        windows::save(&target, token)
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("CP-SUI-VAULT-UNSUPPORTED: Сохранение API-токена доступно только через Диспетчер учётных данных Windows".into())
    }
}

/// Load the token into an explicitly zeroizing allocation.
pub fn load(panel_id: &str) -> Result<Zeroizing<String>, String> {
    let target = target_name(panel_id)?;
    #[cfg(windows)]
    {
        windows::load(&target)
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("CP-SUI-VAULT-UNSUPPORTED: Диспетчер учётных данных Windows недоступен на этой платформе".into())
    }
}

/// Delete only this panel's credential. Forgetting an absent token succeeds.
pub fn forget(panel_id: &str) -> Result<(), String> {
    let target = target_name(panel_id)?;
    #[cfg(windows)]
    {
        windows::forget(&target)
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err(
            "CP-SUI-VAULT-UNSUPPORTED: Удаление сохранённого API-токена доступно только в Windows"
                .into(),
        )
    }
}

#[cfg(windows)]
mod windows {
    use super::{validate_token, MAX_TOKEN_BYTES};
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::{
            CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
            CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        },
    };
    use zeroize::{Zeroize, Zeroizing};

    fn wide(value: &str) -> Zeroizing<Vec<u16>> {
        Zeroizing::new(value.encode_utf16().chain(Some(0)).collect())
    }

    /// Owns exactly the allocation returned by CredReadW.
    /// The secret blob is wiped before releasing the allocation to Windows.
    struct ReadCredential(*mut CREDENTIALW);

    impl Drop for ReadCredential {
        fn drop(&mut self) {
            if self.0.is_null() {
                return;
            }
            // SAFETY: this pointer is created only by successful CredReadW,
            // and CredFree is called exactly once by this owner. The Win32 API
            // bounds credential blobs by CRED_MAX_CREDENTIAL_BLOB_SIZE.
            unsafe {
                let credential = &mut *self.0;
                let length = credential.CredentialBlobSize as usize;
                if !credential.CredentialBlob.is_null()
                    && length <= CRED_MAX_CREDENTIAL_BLOB_SIZE as usize
                {
                    std::slice::from_raw_parts_mut(credential.CredentialBlob, length).zeroize();
                }
                CredFree(self.0.cast());
                self.0 = ptr::null_mut();
            }
        }
    }

    pub(super) fn save(target: &str, token: &str) -> Result<(), String> {
        let mut target_wide = wide(target);
        let mut username = wide("Corepass S-UI API token");
        // Win32 borrows this allocation during CredWriteW and copies its data.
        // Zeroizing wipes it on both success and failure.
        let mut blob = Zeroizing::new(token.as_bytes().to_vec());
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target_wide.as_mut_ptr(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: username.as_mut_ptr(),
            ..CREDENTIALW::default()
        };
        // SAFETY: all pointers reference live, NUL-terminated wide strings or
        // the live token blob. CredWriteW does not take ownership of them.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err("CP-SUI-VAULT-WRITE: Windows не сохранила API-токен. Он не записан в файлы приложения".into());
        }
        Ok(())
    }

    pub(super) fn load(target: &str) -> Result<Zeroizing<String>, String> {
        let target_wide = wide(target);
        let mut pointer = ptr::null_mut();
        // SAFETY: target_wide remains live and pointer is an output parameter
        // initialized to null, as required by CredReadW.
        if unsafe { CredReadW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) } == 0 {
            return Err(
                "CP-SUI-VAULT-READ: Сохранённый API-токен недоступен. Введите его снова".into(),
            );
        }
        let allocation = ReadCredential(pointer);
        if allocation.0.is_null() {
            return Err("CP-SUI-VAULT-READ: Windows не вернула сохранённый API-токен".into());
        }
        // SAFETY: a non-null successful CredReadW allocation is valid until
        // the RAII owner drops. Do not expose the pointer outside this scope.
        let credential = unsafe { &*allocation.0 };
        let length = credential.CredentialBlobSize as usize;
        if credential.Type != CRED_TYPE_GENERIC
            || credential.CredentialBlob.is_null()
            || length == 0
            || length > MAX_TOKEN_BYTES
            || length > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize
        {
            return Err(
                "CP-SUI-VAULT-DATA: Сохранённый API-токен имеет неподдерживаемый формат".into(),
            );
        }
        // SAFETY: length and null have just been checked against the Win32
        // credential blob bounds; allocation remains live during the copy.
        let bytes = unsafe { std::slice::from_raw_parts(credential.CredentialBlob, length) };
        validate_token(bytes)?;
        let mut token = Zeroizing::new(String::with_capacity(length));
        // All bytes are ASCII after validation. This avoids an unprotected
        // intermediate UTF-8 Vec/String allocation containing the secret.
        for byte in bytes {
            token.push(*byte as char);
        }
        Ok(token)
        // allocation's Drop wipes Windows' source buffer before CredFree.
    }

    pub(super) fn forget(target: &str) -> Result<(), String> {
        let target_wide = wide(target);
        // SAFETY: the validated namespace and NUL-terminated target remain
        // live for the call; only a generic credential of this name is deleted.
        if unsafe { CredDeleteW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            // GetLastError is read immediately, before any other Win32 call.
            if unsafe { GetLastError() } != ERROR_NOT_FOUND {
                return Err("CP-SUI-VAULT-DELETE: Windows не удалила сохранённый API-токен".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{target_name, validate_token, TARGET_PREFIX};

    #[test]
    fn targets_are_bounded_to_one_owned_namespace() {
        assert_eq!(
            target_name("abc123_-XYZ").unwrap(),
            format!("{TARGET_PREFIX}abc123_-XYZ")
        );
        assert!(target_name(&"a".repeat(64)).is_ok());
        for value in [
            "",
            "../another",
            "a/b",
            "a\\b",
            "a\0b",
            "a:b",
            "a b",
            "панель",
            "a\nb",
        ] {
            assert!(target_name(value).is_err());
        }
        assert!(target_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn token_validation_does_not_call_the_windows_vault() {
        assert!(validate_token(b"example-token_0123456789").is_ok());
        assert!(validate_token(&vec![b'x'; 2048]).is_ok());
        for token in [
            b"".as_slice(),
            b"token value",
            b"token\nvalue",
            b"token\0value",
            b"\x7f",
            b"\xff",
        ] {
            assert!(validate_token(token).is_err());
        }
        assert!(validate_token(&vec![b'x'; 2049]).is_err());
    }
}
