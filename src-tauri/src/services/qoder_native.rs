mod app;
pub(crate) mod desktop;

pub(crate) use app::active_user_id as active_app_user_id;

use super::qoder_accounts::{self, Account, Edition};
use std::path::Path;

fn tool_matches(edition: Edition, tool_id: &str) -> bool {
    matches!(
        (edition, tool_id),
        (Edition::Qoder, "qoder") | (Edition::QoderCn, "qodercn")
    )
}

pub(crate) fn desktop_edition(executable: &Path) -> Option<Edition> {
    desktop::edition(&std::fs::canonicalize(executable).ok()?)
}
pub(crate) async fn apply(
    edition: Edition,
    account_id: &str,
    tool_id: &str,
) -> Result<Account, String> {
    if !tool_matches(edition, tool_id) {
        return Err("accountError.invalidAccount".into());
    }
    let executable =
        super::tool_manager::get_tool_exe_path(tool_id).ok_or("accountError.unavailable")?;
    let session = qoder_accounts::credentials_for_native(edition, account_id).await?;
    let credentials = &session.credentials;
    let executable = std::fs::canonicalize(executable).map_err(|_| "accountError.unavailable")?;
    let identity = desktop::identity(&executable)
        .filter(|(e, _)| *e == edition)
        .ok_or("accountError.invalidAccount")?;
    super::process_manager::stop_desktop_for_account(tool_id).await?;
    let account = match identity.1 {
        desktop::ProductKind::Ide => {
            let transaction = desktop::apply(credentials, &executable).await?;
            let account = qoder_accounts::mark_active(edition, account_id).await?;
            transaction.commit();
            account
        }
        desktop::ProductKind::App => {
            let transaction = app::apply(credentials, &executable).await?;
            let account = qoder_accounts::mark_active(edition, account_id).await?;
            transaction.commit();
            account
        }
    };
    Ok(account)
}

pub(crate) fn discover(edition: Edition) -> Result<Option<qoder_accounts::Credentials>, String> {
    let executable =
        super::tool_manager::get_tool_exe_path(edition.tool()).ok_or("accountError.unavailable")?;
    match desktop::identity(Path::new(&executable)) {
        Some((e, desktop::ProductKind::App)) if e == edition => {
            app::discover(edition, Path::new(&executable))
        }
        Some((e, desktop::ProductKind::Ide)) if e == edition => desktop::discover(edition),
        _ => Err("accountError.invalidAccount".into()),
    }
}

fn device_credentials(
    edition: Edition,
    value: &serde_json::Value,
    machine: &str,
    app: bool,
) -> Result<qoder_accounts::Credentials, String> {
    let user = if app { &value["user"] } else { value };
    let text = |v: &serde_json::Value, key: &str| {
        v[key]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
    };
    let token = text(value, "token").ok_or("accountError.invalidAccount")?;
    let refresh = text(value, "refreshToken").ok_or("accountError.invalidAccount")?;
    let user_id = text(user, "id").ok_or("accountError.invalidAccount")?;
    let expires = if app {
        chrono::DateTime::parse_from_rfc3339(
            value["expiresAt"].as_str().ok_or("accountError.format")?,
        )
        .map_err(|_| "accountError.format")?
        .timestamp_millis()
    } else {
        value["expireTime"]
            .as_i64()
            .or_else(|| value["expireTime"].as_str()?.parse().ok())
            .ok_or("accountError.format")?
    };
    if expires <= 0 || machine.trim().is_empty() {
        return Err("accountError.invalidAccount".into());
    }
    Ok(qoder_accounts::Credentials {
        edition,
        access_token: String::new(),
        refresh_token: String::new(),
        expires_at: 0,
        user_id,
        name: text(user, "name").unwrap_or_default(),
        email: text(user, "email").unwrap_or_default(),
        machine_id: machine.trim().into(),
        device_token: token,
        device_refresh: refresh,
        device_expires_at: expires,
        device_chat: false,
        native_device_only: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_device_sessions_keep_their_token_family_and_region() {
        for edition in [Edition::Qoder, Edition::QoderCn] {
            let value = serde_json::json!({"token":"device-token","refreshToken":"device-refresh","expiresAt":"2099-01-01T00:00:00Z","user":{"id":"native-user","email":"fixture@example.test"}});
            let c = device_credentials(edition, &value, "existing-machine", true).unwrap();
            assert_eq!(c.edition, edition);
            assert_eq!(c.device_token, "device-token");
            assert!(c.native_device_only);
            assert!(c.access_token.is_empty());
            assert!(c.refresh_token.is_empty());
            assert_eq!(c.user_id, "native-user");
            let mut incomplete = value.clone();
            incomplete.as_object_mut().unwrap().remove("refreshToken");
            assert!(device_credentials(edition, &incomplete, "existing-machine", true).is_err());
            assert!(device_credentials(edition, &value, "", true).is_err());
            let ide = serde_json::json!({"token":"ide-device","refreshToken":"ide-refresh","expireTime":"4070908800000","id":"ide-user"});
            let c = device_credentials(edition, &ide, "existing-machine", false).unwrap();
            assert_eq!(c.device_token, "ide-device");
            assert_eq!(c.device_expires_at, 4070908800000);
        }
    }

    #[test]
    fn account_binding_accepts_only_its_desktop_edition() {
        assert!(tool_matches(Edition::Qoder, "qoder"));
        assert!(tool_matches(Edition::QoderCn, "qodercn"));
        assert!(!tool_matches(Edition::Qoder, "qodercn"));
        assert!(!tool_matches(Edition::QoderCn, "qoder"));
        assert!(!tool_matches(Edition::Qoder, "unknown"));
        assert!(!tool_matches(Edition::QoderCn, "unknown"));
    }

    #[tokio::test]
    async fn mismatched_or_unsupported_tools_fail_before_resolving_native_configuration() {
        for (edition, tool_id) in [
            (Edition::Qoder, "qodercn"),
            (Edition::QoderCn, "qoder"),
            (Edition::Qoder, "unknown"),
            (Edition::QoderCn, "unknown"),
        ] {
            assert!(matches!(
                apply(edition, "fixture-account", tool_id).await,
                Err(error) if error == "accountError.invalidAccount"
            ));
        }
    }
}
