use super::*;

fn cookie(edition: Edition, name: &str, value: &str) -> Cookie {
    Cookie {
        name: name.into(),
        value: value.into(),
        domain: format!(".{}", edition.domain()),
        path: "/".into(),
        secure: true,
        http_only: true,
        expires: None,
    }
}
fn credentials(edition: Edition) -> Credentials {
    Credentials {
        edition,
        user_id: "fixture-user".into(),
        enterprise_type: "0".into(),
        cookies: vec![
            cookie(edition, "sessionid", "fixture-session"),
            cookie(edition, "ttwid", "fixture-device"),
        ],
    }
}

#[test]
fn edition_contract_and_identity_separate_regions_and_enterprise() {
    assert_eq!(
        serde_json::to_value(Edition::DoubaoWork).unwrap(),
        "doubaowork"
    );
    assert_eq!(serde_json::to_value(Edition::Dola).unwrap(), "dola");
    let personal = credentials(Edition::DoubaoWork);
    let mut enterprise = personal.clone();
    enterprise.enterprise_type = "1".into();
    assert_ne!(personal.id(), enterprise.id());
    assert_ne!(personal.id(), credentials(Edition::Dola).id());
}

#[test]
fn passport_profile_does_not_fabricate_subscription_or_quota() {
    let value = json!({"message":"success","data":{"user_id_str":"fixture-user","name":"Fixture","passport_enterprise_user_type":0}});
    let (account, user, enterprise) = api::parse_profile(Edition::Dola, &value).unwrap();
    assert_eq!(account.name, "Fixture");
    assert_eq!(user, "fixture-user");
    assert_eq!(enterprise, "0");
    assert!(
        account.plan.is_none()
            && account.expires_at.is_none()
            && account.remaining.is_none()
            && account.total.is_none()
    );
    assert_eq!(account.id, credentials(Edition::Dola).id());
    assert!(!account.active);
    assert!(api::parse_profile(
        Edition::Dola,
        &json!({"message":"error","data":{"user_id_str":"fixture-user"}})
    )
    .is_err());
    assert!(api::parse_profile(
        Edition::Dola,
        &json!({"message":"success","data":{"name":"Anonymous"}})
    )
    .is_err());
    assert!(api::parse_profile(Edition::Dola, &json!({"data":{"user_id_str":" "}})).is_err());
}

#[test]
fn credentials_only_send_current_first_party_root_cookies() {
    let mut c = credentials(Edition::Dola);
    c.cookies.push(Cookie {
        domain: "accounts.dola.com".into(),
        ..cookie(Edition::Dola, "other-host", "private")
    });
    c.cookies.push(Cookie {
        path: "/client".into(),
        ..cookie(Edition::Dola, "other-path", "private")
    });
    c.cookies.push(Cookie {
        expires: Some(1),
        ..cookie(Edition::Dola, "expired", "private")
    });
    assert_eq!(
        c.cookie_header().unwrap(),
        "sessionid=fixture-session; ttwid=fixture-device"
    );
    c.cookies
        .push(cookie(Edition::DoubaoWork, "sessionid", "wrong-region"));
    assert!(c.validate().is_err());
    c.cookies.pop();
    c.cookies.push(Cookie {
        domain: "evil-dola.com".into(),
        ..cookie(Edition::Dola, "sessionid", "wrong-domain")
    });
    assert!(c.validate().is_err());
}

#[test]
fn missing_expired_and_injected_sessions_are_not_accepted() {
    let mut c = credentials(Edition::Dola);
    c.cookies.retain(|cookie| cookie.name != "sessionid");
    assert_eq!(c.cookie_header().unwrap_err(), "accountError.loginRequired");
    c.cookies.push(Cookie {
        expires: Some(1),
        ..cookie(Edition::Dola, "sessionid", "old")
    });
    assert!(c.cookie_header().is_err());
    c.cookies
        .push(cookie(Edition::Dola, "sessionid", "a\r\nX-Test: value"));
    assert!(c.validate().is_err());
    assert!(c.cookie_header().is_err());
}

#[test]
fn passive_missing_store_is_empty_and_does_not_create_native_state() {
    let root = std::env::temp_dir().join(format!("doubao-passive-{}", uuid::Uuid::new_v4()));
    let path = root.join("accounts.json");
    assert!(load(&path).unwrap().is_empty());
    assert!(!root.exists());
}

#[test]
fn plaintext_and_wrong_region_saved_credentials_are_rejected_before_decryption() {
    let c = credentials(Edition::Dola);
    let (account, _, _) = api::parse_profile(
        Edition::Dola,
        &json!({"data":{"user_id_str":"fixture-user"}}),
    )
    .unwrap();
    let store = Store::from([(
        c.id(),
        Saved {
            account,
            credential: "plaintext-session".into(),
        },
    )]);
    assert_eq!(
        credential(&store, Edition::Dola, &c.id()).err().unwrap(),
        "accountError.format"
    );
    assert_eq!(
        credential(&store, Edition::DoubaoWork, &c.id())
            .err()
            .unwrap(),
        "accountError.invalidAccount"
    );
}

#[tokio::test]
#[ignore = "Explicit read-only identity/model-directory checks; never sends chat, claims, or native changes"]
async fn saved_doubao_identity_and_catalog_read_only_probe() {
    assert_eq!(
        std::env::var("ECHOBIRD_DOUBAO_READ_ONLY_PROBE").as_deref(),
        Ok("1")
    );
    let key = dirs::home_dir()
        .unwrap()
        .join(".echobird/config/.encryption_key");
    assert_eq!(std::fs::metadata(key).unwrap().len(), 32);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    for edition in [Edition::DoubaoWork, Edition::Dola] {
        let store = load(&path(edition).unwrap()).unwrap();
        for (index, (id, _)) in store.iter().enumerate().take(1) {
            let c = credential(&store, edition, id).unwrap();
            match api::identity(&c).await {
                Ok((account,user,enterprise)) => println!("edition={} index={index} identity_ok=true account_id_equal={} user_id_equal={} enterprise_equal={}", edition.tool(), account.id==*id,user==c.user_id,enterprise==c.enterprise_type),
                Err(error) => println!("edition={} index={index} identity_ok=false known_error_key={}",edition.tool(),match error.split('|').next().unwrap_or("") {"accountError.loginRequired"=>"loginRequired","accountError.network"=>"network","accountError.format"=>"format","accountError.quota"=>"http",_=>"other"}),
            }
            let provider = match edition {
                Edition::DoubaoWork => crate::models::model::AccountProvider::DoubaoWork,
                Edition::Dola => crate::models::model::AccountProvider::Dola,
            };
            let secret = proxy_credentials(edition, id).await.unwrap();
            match super::super::account_proxy::list_models(provider,secret,&client).await {
                Ok(models)=>println!("edition={} index={index} catalog_ok=true model_count={}",edition.tool(),models.len()),
                Err(error)=>println!("edition={} index={index} catalog_ok=false http_status={} error_code_present={} launch_missing_assistant={} empty_directory={} decode_error={} send_error={}",edition.tool(),error.status.as_u16(),error.error_code.is_some(),error.message.contains("launch response has no assistant identifier"),error.message.contains("returned no model directory"),error.message.contains("decoding response"),error.message.contains("sending request")),
            }
        }
    }
}
