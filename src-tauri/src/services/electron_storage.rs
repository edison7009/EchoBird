//! The native Electron credential formats used by Cursor and Grok Bot.
#[cfg(windows)]
use aes_gcm::KeyInit;
#[cfg(any(windows, test))]
use aes_gcm::{aead::Aead, Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine};
#[cfg(any(unix, test))]
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
#[cfg(any(windows, test))]
use rand::RngCore;
use std::path::Path;

pub(super) enum Cipher {
    #[cfg(any(windows, test))]
    Gcm(Box<Aes256Gcm>),
    #[cfg(any(unix, test))]
    Cbc {
        key: [u8; 16],
        prefix: [u8; 3],
        linux: bool,
    },
}

#[cfg(any(unix, test))]
fn cbc_key(password: &[u8], iterations: u32) -> [u8; 16] {
    let mut key = [0; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", iterations, &mut key);
    key
}

#[cfg(any(target_os = "macos", test))]
fn macos_password(
    name: &str,
    mut query: impl FnMut(&str, &str) -> Result<Option<String>, String>,
) -> Result<String, String> {
    let service = format!("{name} Safe Storage");
    for account in [name.to_owned(), format!("{name} Key")] {
        if let Some(password) = query(&service, &account)? {
            let password = password.trim_end_matches(['\r', '\n']);
            return if password.is_empty() {
                Err("accountError.keychain".into())
            } else {
                Ok(password.to_owned())
            };
        }
    }
    Err("accountError.keychain".into())
}

pub(super) fn cipher(dir: &Path) -> Result<Cipher, String> {
    #[cfg(unix)]
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("accountError.home")?;
    #[cfg(not(unix))]
    let name = "";
    cipher_with_name(dir, name)
}

pub(super) fn cipher_with_name(dir: &Path, name: &str) -> Result<Cipher, String> {
    #[cfg(windows)]
    {
        let _ = name;
        use windows::Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{
                CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
            },
        };
        let state: serde_json::Value = super::cursor_auth::read(&dir.join("Local State"))?;
        let wrapped = STANDARD
            .decode(
                state["os_crypt"]["encrypted_key"]
                    .as_str()
                    .ok_or("accountError.keychain")?,
            )
            .map_err(|_| "accountError.keychain")?;
        let key = wrapped
            .strip_prefix(b"DPAPI")
            .ok_or("accountError.keychain")?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: key.len().try_into().map_err(|_| "accountError.keychain")?,
            pbData: key.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // DPAPI owns output; copy into the cipher before releasing it.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|_| "accountError.keychain")?;
            let result = if output.cbData == 32 && !output.pbData.is_null() {
                Aes256Gcm::new_from_slice(std::slice::from_raw_parts(output.pbData, 32))
                    .map(|key| Cipher::Gcm(Box::new(key)))
                    .map_err(|_| "accountError.keychain".to_string())
            } else {
                Err("accountError.keychain".into())
            };
            let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
            result
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = dir;
        let password = macos_password(name, |service, account| {
            let output = crate::utils::process::command("/usr/bin/security")
                .args(["find-generic-password", "-s", service, "-a", account, "-w"])
                .output()
                .map_err(|_| "accountError.keychain")?;
            // security returns 44 for errSecItemNotFound. Denied or locked
            // keychains must stop here instead of triggering another prompt.
            match output.status.code() {
                Some(0) => String::from_utf8(output.stdout)
                    .map(Some)
                    .map_err(|_| "accountError.keychain".into()),
                Some(44) => Ok(None),
                _ => Err("accountError.keychain".into()),
            }
        })?;
        Ok(Cipher::Cbc {
            key: cbc_key(password.as_bytes(), 1003),
            prefix: *b"v10",
            linux: false,
        })
    }
    #[cfg(target_os = "linux")]
    {
        // Keep the format the client explicitly selected. A locked keyring must
        // never cause v11 credentials to be rewritten using the basic-text key.
        if let Ok(document) = super::cursor_auth::read(&dir.join("sand-secrets.json")) {
            if uses_linux_basic_storage(&document) {
                return Ok(Cipher::Cbc {
                    key: cbc_key(b"peanuts", 1),
                    prefix: *b"v10",
                    linux: true,
                });
            }
        }
        let password = linux::password(name)?;
        Ok(Cipher::Cbc {
            key: cbc_key(&password, 1),
            prefix: *b"v11",
            linux: true,
        })
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = (dir, name);
        Err("accountError.unavailable".into())
    }
}

#[cfg(any(target_os = "linux", test))]
fn uses_linux_basic_storage(document: &serde_json::Value) -> bool {
    let Some(raw) = document["cursor-accounts"].as_str() else {
        return false;
    };
    let Ok(catalog) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    let Some(accounts) = catalog["accounts"].as_object() else {
        return false;
    };
    let tokens: Vec<_> = accounts
        .values()
        .flat_map(|row| {
            [
                row["cursor-access-token"].as_str(),
                row["cursor-refresh-token"].as_str(),
                row["cursor-account-profile"].as_str(),
                row["cursor-selected-team-id"].as_str(),
            ]
        })
        .flatten()
        .collect();
    !tokens.is_empty()
        && tokens.iter().all(|s| {
            STANDARD
                .decode(s)
                .is_ok_and(|bytes| bytes.starts_with(b"v10") && bytes.len() > 3)
        })
}

pub(super) fn decrypt(cipher: &Cipher, encoded: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "accountError.format")?;
    String::from_utf8(decrypt_bytes(cipher, &bytes)?).map_err(|_| "accountError.format".into())
}

pub(super) fn decrypt_bytes(cipher: &Cipher, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let plain = match cipher {
        #[cfg(any(windows, test))]
        Cipher::Gcm(key) => {
            if !bytes.starts_with(b"v10") || bytes.len() < 31 {
                return Err("accountError.format".into());
            }
            key.decrypt(Nonce::from_slice(&bytes[3..15]), &bytes[15..])
                .map_err(|_| "accountError.keychain")?
        }
        #[cfg(any(unix, test))]
        Cipher::Cbc { key, prefix, linux } => {
            let legacy_key;
            let key = if bytes.starts_with(prefix) {
                key
            } else if *linux && bytes.starts_with(b"v10") {
                legacy_key = cbc_key(b"peanuts", 1);
                &legacy_key
            } else {
                return Err("accountError.format".into());
            };
            if bytes.len() <= 3 {
                return Err("accountError.format".into());
            }
            cbc::Decryptor::<aes::Aes128>::new(key.into(), (&[b' '; 16]).into())
                .decrypt_padded_vec_mut::<Pkcs7>(&bytes[3..])
                .map_err(|_| "accountError.keychain")?
        }
    };
    Ok(plain)
}

pub(super) fn encrypt(cipher: &Cipher, plain: &str) -> Result<String, String> {
    Ok(STANDARD.encode(encrypt_bytes(cipher, plain.as_bytes())?))
}

pub(super) fn encrypt_bytes(cipher: &Cipher, plain: &[u8]) -> Result<Vec<u8>, String> {
    let bytes = match cipher {
        #[cfg(any(windows, test))]
        Cipher::Gcm(key) => {
            let mut nonce = [0u8; 12];
            rand::rngs::OsRng.fill_bytes(&mut nonce);
            let mut bytes = b"v10".to_vec();
            bytes.extend_from_slice(&nonce);
            bytes.extend(
                key.encrypt(Nonce::from_slice(&nonce), plain)
                    .map_err(|_| "accountError.keychain")?,
            );
            bytes
        }
        #[cfg(any(unix, test))]
        Cipher::Cbc { key, prefix, .. } => {
            let mut bytes = prefix.to_vec();
            bytes.extend(
                cbc::Encryptor::<aes::Aes128>::new(key.into(), (&[b' '; 16]).into())
                    .encrypt_padded_vec_mut::<Pkcs7>(plain),
            );
            bytes
        }
    };
    Ok(bytes)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{collections::HashMap, time::Duration};
    use zbus::{
        blocking::{connection::Builder, Connection, Proxy},
        zvariant::{OwnedObjectPath, OwnedValue, Value},
    };

    pub(super) fn password(name: &str) -> Result<Vec<u8>, String> {
        let connection = Builder::session()
            .and_then(|b| b.method_timeout(Duration::from_secs(5)).build())
            .map_err(|_| "accountError.keychain")?;
        secret_service(&connection, name)
            .or_else(|_| kwallet(&connection, name))
            .map_err(|_| "accountError.keychain".into())
    }

    fn secret_service(connection: &Connection, name: &str) -> zbus::Result<Vec<u8>> {
        let proxy = Proxy::new(
            connection,
            "org.freedesktop.secrets",
            "/org/freedesktop/secrets",
            "org.freedesktop.Secret.Service",
        )?;
        let (unlocked, _locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            proxy.call("SearchItems", &(HashMap::from([("application", name)]),))?;
        // Passive account reads must not unlock a wallet or open a system prompt.
        let item = unlocked.first().ok_or_else(|| {
            zbus::Error::Failure("Credential store locked or uninitialized".into())
        })?;
        let (_, session): (OwnedValue, OwnedObjectPath) =
            proxy.call("OpenSession", &("plain", Value::from("")))?;
        let result = (|| {
            let item = Proxy::new(
                connection,
                "org.freedesktop.secrets",
                item.as_str(),
                "org.freedesktop.Secret.Item",
            )?;
            let (_, _, secret, _): (OwnedObjectPath, Vec<u8>, Vec<u8>, String) =
                item.call("GetSecret", &(&session,))?;
            if secret.is_empty() {
                return Err(zbus::Error::Failure("Empty key".into()));
            }
            Ok(secret)
        })();
        if let Ok(session) = Proxy::new(
            connection,
            "org.freedesktop.secrets",
            session.as_str(),
            "org.freedesktop.Secret.Session",
        ) {
            let _: zbus::Result<()> = session.call("Close", &());
        }
        result
    }

    fn kwallet(connection: &Connection, name: &str) -> zbus::Result<Vec<u8>> {
        for suffix in ["6", "5", ""] {
            let destination = format!("org.kde.kwalletd{suffix}");
            let path = format!("/modules/kwalletd{suffix}");
            let Ok(proxy) = Proxy::new(
                connection,
                destination.as_str(),
                path.as_str(),
                "org.kde.KWallet",
            ) else {
                continue;
            };
            let result = (|| -> zbus::Result<Vec<u8>> {
                let wallet: String = proxy.call("networkWallet", &())?;
                let open: bool = proxy.call("isOpen", &(&wallet,))?;
                if !open {
                    return Err(zbus::Error::Failure("Wallet locked".into()));
                }
                let handle: i32 = proxy.call("open", &(&wallet, 0i64, name))?;
                if handle < 0 {
                    return Err(zbus::Error::Failure("Wallet unavailable".into()));
                }
                let result: zbus::Result<String> = proxy.call(
                    "readPassword",
                    &(handle, "Chromium Keys", "Chromium Safe Storage", name),
                );
                let _: zbus::Result<i32> = proxy.call("close", &(handle, false, name));
                let password = result?;
                if password.is_empty() {
                    return Err(zbus::Error::Failure("Empty key".into()));
                }
                Ok(password.into_bytes())
            })();
            if result.is_ok() {
                return result;
            }
        }
        Err(zbus::Error::Failure("Credential store unavailable".into()))
    }

    #[test]
    fn secret_service_reply_matches_the_dbus_struct_signature() {
        let secret = (
            OwnedObjectPath::try_from("/session").unwrap(),
            Vec::<u8>::new(),
            b"test-key".to_vec(),
            "text/plain".to_string(),
        );
        let message = zbus::Message::method_call("/", "GetSecret")
            .unwrap()
            .build(&(secret,))
            .unwrap();
        let (_, _, secret, _): (OwnedObjectPath, Vec<u8>, Vec<u8>, String) =
            message.body().deserialize().unwrap();
        assert_eq!(secret, b"test-key");
    }
}

#[cfg(test)]
pub(super) fn test_ciphers() -> Vec<Cipher> {
    use aes_gcm::KeyInit;
    vec![
        Cipher::Gcm(Box::new(Aes256Gcm::new_from_slice(&[7; 32]).unwrap())),
        Cipher::Cbc {
            key: cbc_key(b"test-safe-storage-password", 1003),
            prefix: *b"v10",
            linux: false,
        },
        Cipher::Cbc {
            key: cbc_key(b"test-safe-storage-password", 1),
            prefix: *b"v11",
            linux: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_keychain_lookup_keeps_service_and_supports_both_account_names() {
        for expected in ["Claude", "Claude Key"] {
            let mut accounts = Vec::new();
            let password = macos_password("Claude", |service, account| {
                assert_eq!(service, "Claude Safe Storage");
                accounts.push(account.to_owned());
                Ok((account == expected).then(|| "fixture-password\n".into()))
            })
            .unwrap();
            assert_eq!(password, "fixture-password");
            assert_eq!(accounts.last().unwrap(), expected);
            assert_eq!(accounts.len(), if expected == "Claude" { 1 } else { 2 });
        }
    }

    #[test]
    fn macos_keychain_failure_never_falls_back_after_denial_or_empty_password() {
        for result in [Err("accountError.keychain".into()), Ok(Some(String::new()))] {
            let mut calls = 0;
            assert!(macos_password("Claude", |_, _| {
                calls += 1;
                result.clone()
            })
            .is_err());
            assert_eq!(calls, 1);
        }
        assert!(macos_password("Claude", |_, _| Ok(None)).is_err());
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "Requires the isolated native credential fixture configured by CI"]
    fn reads_native_os_credential_fixture() {
        assert_eq!(
            std::env::var("ECHOBIRD_NATIVE_CREDENTIAL_TEST").as_deref(),
            Ok("1")
        );
        let expected = if cfg!(target_os = "macos") {
            "djEwCTpDbc8erpLeOOa4QeXX3hTJLw1RJaPhtqn8lNRG/ug="
        } else {
            "djExfP9LPCqfuh6gHB6yEZQaljVJQLFbyL1I9lBu1VrKmpA="
        };
        let names: &[&str] = if cfg!(target_os = "macos") {
            &[
                "EchoBird Native Storage Test",
                "EchoBird Native Storage Alternate Test",
            ]
        } else {
            &["EchoBird Native Storage Test"]
        };
        for name in names {
            let key = cipher(Path::new(name)).unwrap();
            assert_eq!(decrypt(&key, expected).unwrap(), "native-token-中文");
            assert_eq!(encrypt(&key, "native-token-中文").unwrap(), expected);
        }
    }

    #[test]
    fn native_macos_and_linux_vectors_are_compatible_in_both_directions() {
        // Independently generated with Python's cryptography AES-CBC and PBKDF2.
        let vectors = [
            "djEwCTpDbc8erpLeOOa4QeXX3hTJLw1RJaPhtqn8lNRG/ug=",
            "djExfP9LPCqfuh6gHB6yEZQaljVJQLFbyL1I9lBu1VrKmpA=",
        ];
        for (key, encoded) in test_ciphers().iter().skip(1).zip(vectors) {
            assert_eq!(decrypt(key, encoded).unwrap(), "native-token-中文");
            assert_eq!(encrypt(key, "native-token-中文").unwrap(), encoded);
            for invalid in ["!", "djEy", "djEw", "djExAAAA"] {
                assert!(decrypt(key, invalid).is_err());
            }
        }
        let keys = test_ciphers();
        let legacy = "djEwiDSMDp5ppG7dczOrzNnvnZo1KG+vH2lW1/ygLC96EPg=";
        assert_eq!(decrypt(&keys[2], legacy).unwrap(), "native-token-中文");
        assert!(decrypt(&keys[1], legacy).is_err());
        assert!(decrypt(&keys[0], vectors[0]).is_err());
    }

    #[test]
    fn basic_storage_requires_existing_native_v10_tokens() {
        use serde_json::json;
        let document = |token: &str| json!({"cursor-accounts":json!({"accounts":{"one":{"cursor-access-token":token}}}).to_string()});
        assert!(uses_linux_basic_storage(&document(
            "djEwiDSMDp5ppG7dczOrzNnvnZo1KG+vH2lW1/ygLC96EPg="
        )));
        assert!(!uses_linux_basic_storage(&document(
            "djExfP9LPCqfuh6gHB6yEZQaljVJQLFbyL1I9lBu1VrKmpA="
        )));
        assert!(!uses_linux_basic_storage(
            &json!({"cursor-accounts":json!({"accounts":{"one":{
            "cursor-access-token":"djEwiDSMDp5ppG7dczOrzNnvnZo1KG+vH2lW1/ygLC96EPg=",
            "cursor-account-profile":"djExfP9LPCqfuh6gHB6yEZQaljVJQLFbyL1I9lBu1VrKmpA="
        }}}).to_string()})
        ));
        assert!(!uses_linux_basic_storage(&document("bad")));
        assert!(!uses_linux_basic_storage(&json!({})));
    }
}
