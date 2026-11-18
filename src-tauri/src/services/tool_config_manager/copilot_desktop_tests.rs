//! Isolated native-channel fixtures: no installed client, credentials, or user files.
use super::*;
use crate::services::{copilot_accounts::desktop, smart_router::catalog};
use futures_util::{SinkExt, StreamExt};
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message;

struct Fixture {
    dir: super::super::switching::TestDir,
    connection: Connection,
    bridge: desktop::Desktop,
    requests: Arc<Mutex<Vec<Value>>>,
    fail_delete: Arc<Mutex<Option<usize>>>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new() -> Self {
        let dir = super::super::switching::TestDir::new();
        let database = dir.path().join("data.db");
        let connection = Connection::open(&database).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE app_state(key TEXT PRIMARY KEY,value TEXT,updated_at TEXT);
            CREATE TABLE provider_models(id TEXT PRIMARY KEY,provider_id TEXT,model_id TEXT,wire_model TEXT,display_name TEXT,UNIQUE(provider_id,model_id));
            INSERT INTO provider_models VALUES('foreign-row','foreign','manual','manual','Keep');").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let fail_delete = Arc::new(Mutex::new(None));
        let recorded = requests.clone();
        let failure = fail_delete.clone();
        let server = tokio::spawn(async move {
            let mut connection = Connection::open(database).unwrap();
            let mut providers = std::collections::HashMap::<String, Value>::new();
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let command: Value = serde_json::from_str(&text).unwrap();
                recorded.lock().unwrap().push(command.clone());
                let mut response = match command["type"].as_str().unwrap() {
                    "upsert_model_provider" => {
                        let mut provider = command["provider"].clone();
                        provider["hasSecret"] = json!(!command["secret"].is_null());
                        providers.insert(provider["id"].as_str().unwrap().into(), provider.clone());
                        json!({"type":"model_provider_upserted","provider":provider})
                    }
                    "upsert_provider_model" => {
                        let model = &command["model"];
                        connection.execute("INSERT INTO provider_models VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET wire_model=excluded.wire_model,display_name=excluded.display_name",
                            params![model["id"].as_str(),model["providerId"].as_str(),model["modelId"].as_str(),model["wireModel"].as_str(),model["displayName"].as_str()]).unwrap();
                        json!({"type":"provider_model_upserted","model":model})
                    }
                    "delete_provider_model" => {
                        let fail = {
                            let mut remaining = failure.lock().unwrap();
                            match remaining.as_mut() {
                                Some(0) => {
                                    *remaining = None;
                                    true
                                }
                                Some(value) => {
                                    *value -= 1;
                                    false
                                }
                                None => false,
                            }
                        };
                        if fail {
                            json!({"type":"provider_model_error"})
                        } else {
                            connection
                                .execute(
                                    "DELETE FROM provider_models WHERE id=?1",
                                    [command["model_id"].as_str().unwrap()],
                                )
                                .unwrap();
                            json!({"type":"provider_model_deleted","model_id":command["model_id"]})
                        }
                    }
                    "list_model_providers" => {
                        json!({"type":"model_providers_list","providers":providers.values().collect::<Vec<_>>()})
                    }
                    "delete_model_provider" => {
                        let id = command["provider_id"].as_str().unwrap();
                        providers.remove(id);
                        connection
                            .execute("DELETE FROM provider_models WHERE provider_id=?1", [id])
                            .unwrap();
                        // A native delete may also clear its selected model.
                        if desktop::selected_model(&connection)
                            .unwrap()
                            .is_some_and(|selected| selected.starts_with(&format!("{id}/")))
                        {
                            desktop::set_selected_model(&mut connection, None).unwrap();
                        }
                        json!({"type":"model_provider_deleted","provider_id":id})
                    }
                    unexpected => panic!("Unexpected native command: {unexpected}"),
                };
                response["request_id"] = command["request_id"].clone();
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let bridge = desktop::Desktop::fixture(&format!("ws://{address}")).await;
        Self {
            dir,
            connection,
            bridge,
            requests,
            fail_delete,
            server,
        }
    }

    fn snapshot(&self) -> PathBuf {
        self.dir.path().join("snapshot.json")
    }
    fn database(&self) -> PathBuf {
        self.dir.path().join("data.db")
    }
    fn owned_id(&self) -> String {
        read_object(&self.snapshot()).unwrap()["providerId"]
            .as_str()
            .unwrap()
            .into()
    }
    fn entries(&self) -> Vec<Value> {
        desktop_entries(&self.connection, &self.owned_id()).unwrap()
    }
    async fn apply(&mut self, model: &ModelInfo) -> Result<(), String> {
        apply_desktop_with_connection(
            model,
            &self.snapshot(),
            &mut self.bridge,
            &mut self.connection,
        )
        .await
    }
    async fn restore(&mut self) {
        restore_desktop_with_connection(&self.snapshot(), &mut self.bridge, &mut self.connection)
            .await
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn router() -> ModelInfo {
    let mut model = super::super::switching::test_info();
    model.api_key = Some("local".into());
    model
}

#[test]
fn smart_menu_uses_stable_wire_selectors_and_direct_api_stays_single() {
    catalog::with_test_models(|| {
        let entries = desktop_model_entries("owned", &router()).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0]["modelId"], MODEL);
        assert_eq!(entries[0]["wireModel"], "auto");
        assert_eq!(entries[1]["modelId"], catalog::selector("first"));
        assert_eq!(entries[1]["wireModel"], catalog::selector("first"));
        assert_eq!(entries[1]["displayName"], "First model");
        let mut direct = router();
        direct.base_url = Some("https://direct.invalid/v1".into());
        assert_eq!(desktop_model_entries("owned", &direct).unwrap().len(), 1);
    });
}

#[tokio::test]
async fn native_menu_apply_repeat_selection_and_direct_cleanup() {
    catalog::with_test_models_async(async {
        let mut fixture = Fixture::new().await;
        fixture.apply(&router()).await.unwrap();
        let before = fixture.entries();
        assert_eq!(before.len(), 3);
        fixture.apply(&router()).await.unwrap();
        assert_eq!(fixture.entries(), before);
        let selected = format!("{}/{}", fixture.owned_id(), catalog::selector("first"));
        desktop::set_selected_model(&mut fixture.connection, Some(&selected)).unwrap();
        let request_count = fixture.requests.lock().unwrap().len();
        let recovered = read_desktop_from(&fixture.snapshot(), &fixture.database()).unwrap();
        assert_eq!(recovered.base_url, router().base_url);
        assert_eq!(fixture.requests.lock().unwrap().len(), request_count);
        let mut direct = router();
        direct.base_url = Some("https://direct.invalid/v1".into());
        direct.model = Some("direct-model".into());
        fixture.apply(&direct).await.unwrap();
        assert_eq!(fixture.entries().len(), 1);
        assert_eq!(fixture.entries()[0]["wireModel"], "direct-model");
        assert_eq!(
            desktop_entries(&fixture.connection, "foreign")
                .unwrap()
                .len(),
            1
        );
        desktop::set_selected_model(&mut fixture.connection, Some("foreign/manual")).unwrap();
        assert!(read_desktop_from(&fixture.snapshot(), &fixture.database()).is_none());
        fixture.restore().await;
        assert_eq!(
            desktop::selected_model(&fixture.connection)
                .unwrap()
                .as_deref(),
            Some("foreign/manual")
        );
    })
    .await;
}

#[tokio::test]
async fn partial_cleanup_failure_restores_all_rows_ids_snapshot_and_selected_model() {
    catalog::with_test_models_async(async {
        let mut fixture = Fixture::new().await;
        fixture.apply(&router()).await.unwrap();
        let selected = format!("{}/{}", fixture.owned_id(), catalog::selector("first"));
        desktop::set_selected_model(&mut fixture.connection, Some(&selected)).unwrap();
        let before = fixture.entries();
        let snapshot = fs::read(fixture.snapshot()).unwrap();
        *fixture.fail_delete.lock().unwrap() = Some(1);
        let mut direct = router();
        direct.base_url = Some("https://direct.invalid/v1".into());
        assert!(fixture.apply(&direct).await.is_err());
        assert_eq!(fixture.entries(), before);
        assert_eq!(fs::read(fixture.snapshot()).unwrap(), snapshot);
        assert_eq!(
            desktop::selected_model(&fixture.connection)
                .unwrap()
                .as_deref(),
            Some(selected.as_str())
        );
    })
    .await;
}

#[tokio::test]
async fn restore_after_native_selector_returns_to_original_official_model() {
    catalog::with_test_models_async(async {
        let mut fixture = Fixture::new().await;
        desktop::set_selected_model(&mut fixture.connection, Some("official-model")).unwrap();
        fixture.apply(&router()).await.unwrap();
        let selected = format!("{}/{}", fixture.owned_id(), catalog::selector("second"));
        desktop::set_selected_model(&mut fixture.connection, Some(&selected)).unwrap();
        fixture.restore().await;
        assert!(!fixture.snapshot().exists());
        assert_eq!(
            desktop::selected_model(&fixture.connection)
                .unwrap()
                .as_deref(),
            Some("official-model")
        );
        assert_eq!(
            desktop_entries(&fixture.connection, "foreign")
                .unwrap()
                .len(),
            1
        );
    })
    .await;
}
