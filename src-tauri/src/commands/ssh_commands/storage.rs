use super::SSHServer;
use crate::services::json_store;
use std::path::Path;

pub(super) fn upsert(path: &Path, mut server: SSHServer) -> Result<SSHServer, String> {
    json_store::mutate(path, |servers: &mut Vec<SSHServer>| {
        // Omitted aliases preserve saved names; an explicit empty alias clears one.
        let clear_alias = server.alias.as_deref() == Some("");
        if clear_alias {
            server.alias = None;
        }
        if let Some(saved) = servers.iter_mut().find(|s| s.id == server.id) {
            if server.alias.is_none() && !clear_alias {
                server.alias = saved.alias.clone();
            }
            *saved = server.clone();
        } else {
            servers.push(server.clone());
        }
        Ok((server, true))
    })
}

pub(super) fn remove(path: &Path, id: &str) -> Result<bool, String> {
    json_store::mutate(path, |servers: &mut Vec<SSHServer>| {
        let before = servers.len();
        servers.retain(|s| s.id != id);
        let changed = before != servers.len();
        Ok((changed, changed))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn server(id: &str) -> SSHServer {
        SSHServer {
            id: id.into(),
            host: "fixture.invalid".into(),
            port: 22,
            username: "fixture".into(),
            password: "enc:v1:fixture".into(),
            alias: None,
        }
    }

    #[test]
    fn ssh_mutations_preserve_alias_and_fail_without_overwriting_corrupt_data() {
        let root = std::env::temp_dir().join(format!("echobird-ssh-{}", uuid::Uuid::new_v4()));
        let path = root.join("ssh.json");
        let mut saved = server("first");
        saved.alias = Some("name".into());
        upsert(&path, saved).unwrap();
        assert_eq!(
            upsert(&path, server("first")).unwrap().alias.as_deref(),
            Some("name")
        );
        let mut cleared = server("first");
        cleared.alias = Some(String::new());
        assert!(upsert(&path, cleared).unwrap().alias.is_none());
        assert!(json_store::read::<Vec<SSHServer>>(&path).unwrap()[0]
            .alias
            .is_none());
        upsert(&path, server("second")).unwrap();
        assert!(remove(&path, "first").unwrap());
        assert!(!remove(&path, "missing").unwrap());
        assert_eq!(json_store::read::<Vec<SSHServer>>(&path).unwrap().len(), 1);
        fs::write(&path, b"broken {").unwrap();
        assert!(upsert(&path, server("third")).is_err());
        assert!(remove(&path, "second").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken {");
        assert!(upsert(&path.join("blocked"), server("fourth")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
