//! Shared pools grouped by transport policy. Credentials stay on individual requests.
use std::{sync::OnceLock, time::Duration};

pub(crate) fn catalog() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default()
    })
}
