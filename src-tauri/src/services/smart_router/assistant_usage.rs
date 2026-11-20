//! Match an assistant's HTTP attempt to the upstream stream the router selected.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::http::HeaderMap;
use serde_json::Value;
use uuid::Uuid;

use crate::models::model::AccountSource;

use super::{
    pricing,
    usage::{UsageAccumulator, UsageReport},
};

pub(crate) const HEADER: &str = "x-echobird-assistant-usage";

pub(super) type Receipt = Arc<Mutex<Option<RoutedUsage>>>;

fn pending() -> &'static Mutex<HashMap<Uuid, Receipt>> {
    static PENDING: OnceLock<Mutex<HashMap<Uuid, Receipt>>> = OnceLock::new();
    PENDING.get_or_init(Mutex::default)
}

pub(crate) struct AssistantUsage {
    pub id: Uuid,
    receipt: Receipt,
}

impl AssistantUsage {
    pub fn register() -> Self {
        let id = Uuid::new_v4();
        let receipt = Receipt::default();
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, receipt.clone());
        Self { id, receipt }
    }

    pub fn selected(&self) -> Option<RoutedUsage> {
        self.receipt
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Drop for AssistantUsage {
    fn drop(&mut self) {
        pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

pub(super) fn claim(headers: &HeaderMap) -> Option<Receipt> {
    let id = Uuid::parse_str(headers.get(HEADER)?.to_str().ok()?).ok()?;
    pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id)
}

#[derive(Clone)]
pub(crate) struct RoutedUsage {
    usage: Arc<Mutex<UsageAccumulator>>,
    pub pricing: RoutedPricing,
}

#[derive(Clone, Debug)]
pub(crate) struct RoutedPricing {
    base_url: String,
    source: Option<AccountSource>,
    model: String,
}

impl RoutedUsage {
    pub(super) fn new(base_url: &str, source: Option<&AccountSource>, model: &str) -> Self {
        Self {
            usage: Arc::default(),
            pricing: RoutedPricing {
                base_url: base_url.to_owned(),
                source: source.cloned(),
                model: model.to_owned(),
            },
        }
    }

    pub(super) fn observe(&self, value: &Value) {
        self.usage
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe(value);
    }

    pub fn report(&self) -> (UsageReport, bool) {
        let usage = self.usage.lock().unwrap_or_else(|e| e.into_inner());
        (usage.report(), usage.total_known())
    }
}

impl RoutedPricing {
    pub fn estimate(&self, report: &UsageReport) -> Option<u64> {
        pricing::lookup(&self.base_url, self.source.as_ref(), &self.model)?.estimate(report)
    }
}
