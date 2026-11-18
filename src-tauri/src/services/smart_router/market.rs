//! Bounded account/model telemetry. Only numeric measurements and hashed row identities are stored.
use super::{pricing, usage::UsageReport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_BUCKETS: usize = 2048;
pub const MAX_EVENTS: usize = 128;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    #[default]
    Completed,
    Failed,
    Cancelled,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Metrics {
    pub attempts: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub speed_samples: u64,
    pub output_tokens: u64,
    pub generation_ms: u64,
    pub first_output_ms: u64,
    pub first_output_samples: u64,
    pub usage_reports: u64,
    pub total_tokens: u64,
    pub priced_reports: u64,
    pub priced_tokens: u64,
    pub cost_micros: u64,
    pub completed_micros: u64,
    pub failed_micros: u64,
    pub completed_priced_reports: u64,
    pub failed_priced_reports: u64,
}
impl Metrics {
    pub fn add(&mut self, other: &Self) {
        macro_rules! add { ($($field:ident),*) => { $(self.$field=self.$field.saturating_add(other.$field);)* }; }
        add!(
            attempts,
            completed,
            failed,
            cancelled,
            speed_samples,
            output_tokens,
            generation_ms,
            first_output_ms,
            first_output_samples,
            usage_reports,
            total_tokens,
            priced_reports,
            priced_tokens,
            cost_micros,
            completed_micros,
            failed_micros,
            completed_priced_reports,
            failed_priced_reports
        );
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub row: String,
    pub at: u64,
    pub day: i32,
    pub hour: u8,
    pub outcome: Outcome,
    pub elapsed_ms: u64,
    pub first_output_ms: Option<u64>,
    pub generation_ms: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_micros: Option<u64>,
    pub price: Option<pricing::Price>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub row: String,
    pub day: i32,
    pub hour: u8,
    pub metrics: Metrics,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub buckets: Vec<Point>,
    pub events: Vec<Event>,
    pub capped: bool,
}
impl Store {
    pub fn valid(&self, identities: &BTreeMap<String, super::statistics::Identity>) -> bool {
        self.buckets.len() <= MAX_BUCKETS
            && self.events.len() <= MAX_EVENTS
            && self
                .buckets
                .iter()
                .all(|p| p.hour < 24 && identities.contains_key(&p.row))
            && self.events.iter().all(|p| {
                p.hour < 24
                    && identities.contains_key(&p.row)
                    && p.price.as_ref().is_none_or(pricing::valid)
            })
    }
    pub fn trim(&mut self, today: i32, identities: &BTreeMap<String, super::statistics::Identity>) {
        self.buckets.retain(|b| {
            b.day > today - super::statistics::RETENTION_DAYS
                && b.day <= today
                && identities.contains_key(&b.row)
        });
        self.events.retain(|e| {
            e.day > today - super::statistics::RETENTION_DAYS
                && e.day <= today
                && identities.contains_key(&e.row)
        });
    }
    pub fn record(
        &mut self,
        row: String,
        outcome: Outcome,
        report: UsageReport,
        price: Option<pricing::Price>,
        stamp: (i32, u8, u64),
    ) {
        let (day, hour, at) = stamp;
        let known = report.usage.reported_responses > 0;
        let cost = price.as_ref().and_then(|p| p.estimate(&report));
        let speed = outcome == Outcome::Completed && report.generation_ms.is_some();
        let mut metrics = Metrics {
            attempts: 1,
            usage_reports: u64::from(known),
            total_tokens: report.usage.total_tokens,
            ..Metrics::default()
        };
        match outcome {
            Outcome::Completed => metrics.completed = 1,
            Outcome::Failed => metrics.failed = 1,
            Outcome::Cancelled => metrics.cancelled = 1,
        }
        if speed {
            metrics.speed_samples = 1;
            metrics.output_tokens = report.usage.output_tokens;
            metrics.generation_ms = report.generation_ms.unwrap_or(0);
        }
        if let Some(first) = report.first_output_ms {
            metrics.first_output_ms = first;
            metrics.first_output_samples = 1;
        }
        if let Some(cost) = cost {
            metrics.priced_reports = 1;
            metrics.priced_tokens = report.usage.total_tokens;
            metrics.cost_micros = cost;
            match outcome {
                Outcome::Completed => {
                    metrics.completed_micros = cost;
                    metrics.completed_priced_reports = 1;
                }
                Outcome::Failed => {
                    metrics.failed_micros = cost;
                    metrics.failed_priced_reports = 1;
                }
                Outcome::Cancelled => {}
            }
        }
        if let Some(bucket) = self
            .buckets
            .iter_mut()
            .find(|b| b.row == row && b.day == day && b.hour == hour)
        {
            bucket.metrics.add(&metrics);
        } else {
            if self.buckets.len() >= MAX_BUCKETS {
                if let Some((index, _)) = self
                    .buckets
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, b)| (b.day, b.hour))
                {
                    self.buckets.remove(index);
                }
                self.capped = true;
            }
            self.buckets.push(Point {
                row: row.clone(),
                day,
                hour,
                metrics,
            });
        }
        self.events.push(Event {
            row,
            at,
            day,
            hour,
            outcome,
            elapsed_ms: report.elapsed_ms,
            first_output_ms: report.first_output_ms,
            generation_ms: if speed { report.generation_ms } else { None },
            output_tokens: known.then_some(report.usage.output_tokens),
            total_tokens: known.then_some(report.usage.total_tokens),
            cost_micros: cost,
            price,
        });
        if self.events.len() > MAX_EVENTS {
            self.events.remove(0);
        }
    }
    pub fn snapshot(
        &self,
        today: i32,
        days: u8,
        identities: &BTreeMap<String, super::statistics::Identity>,
        group: Option<&str>,
    ) -> Snapshot {
        let start = today - i32::from(days) + 1;
        let allowed = |row: &str| {
            identities
                .get(row)
                .is_some_and(|i| group.is_none_or(|g| i.group == g))
        };
        let mut current: BTreeMap<String, Metrics> = BTreeMap::new();
        let mut previous: BTreeMap<String, Metrics> = BTreeMap::new();
        let mut trend = Vec::new();
        for p in &self.buckets {
            if !allowed(&p.row) {
                continue;
            }
            if (start..=today).contains(&p.day) {
                current.entry(p.row.clone()).or_default().add(&p.metrics);
                trend.push(p.clone());
            } else if (start - 7..start).contains(&p.day) {
                previous.entry(p.row.clone()).or_default().add(&p.metrics);
            }
        }
        trend.sort_by_key(|p| (p.day, p.hour));
        Snapshot {
            rows: current
                .into_iter()
                .map(|(row, metrics)| Row {
                    baseline: previous.remove(&row).unwrap_or_default(),
                    row,
                    metrics,
                })
                .collect(),
            events: self
                .events
                .iter()
                .rev()
                .filter(|e| e.day >= start && e.day <= today && allowed(&e.row))
                .cloned()
                .collect(),
            trend,
            capped: self.capped,
            baseline_start: start - 7,
            baseline_end: start - 1,
            baseline_retained: start - 7 > today - super::statistics::RETENTION_DAYS,
            pricing: pricing::status(),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub row: String,
    pub metrics: Metrics,
    pub baseline: Metrics,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub rows: Vec<Row>,
    pub trend: Vec<Point>,
    pub events: Vec<Event>,
    pub capped: bool,
    pub baseline_start: i32,
    pub baseline_end: i32,
    pub baseline_retained: bool,
    pub pricing: pricing::Status,
}

#[cfg(test)]
mod tests {
    use super::super::{
        statistics::{row_identity, Identity},
        usage::TokenUsage,
    };
    use super::*;
    #[test]
    fn bounds_history_filters_groups_and_excludes_current_window_from_baseline() {
        let today = chrono::Datelike::num_days_from_ce(&chrono::Local::now().date_naive());
        let (row, id) = row_identity("g", "a", "m", None);
        let ids: BTreeMap<String, Identity> = [(row.clone(), id)].into();
        let mut s = Store::default();
        for day in [today - 1, today, today] {
            s.record(
                row.clone(),
                Outcome::Completed,
                UsageReport {
                    usage: TokenUsage {
                        input_tokens: 1,
                        output_tokens: 10,
                        total_tokens: 11,
                        reported_responses: 1,
                    },
                    generation_ms: Some(1000),
                    ..UsageReport::default()
                },
                None,
                (day, 1, 1),
            );
        }
        let snap = s.snapshot(today, 1, &ids, None);
        assert_eq!(snap.rows[0].metrics.speed_samples, 2);
        assert_eq!(snap.rows[0].baseline.speed_samples, 1);
        assert_eq!(snap.rows[0].metrics.priced_reports, 0);
        assert!(snap.events[0].cost_micros.is_none());
        assert!(s.snapshot(today, 1, &ids, Some("another")).rows.is_empty());
        assert!(!s.snapshot(today, 14, &ids, None).baseline_retained);
        for n in 0..MAX_EVENTS + 5 {
            s.record(
                row.clone(),
                Outcome::Failed,
                UsageReport::default(),
                None,
                (today, 1, n as u64),
            );
        }
        assert_eq!(s.events.len(), MAX_EVENTS);
        assert_eq!(s.events.last().unwrap().outcome, Outcome::Failed);
        assert_eq!(
            s.snapshot(today, 1, &ids, None).rows[0]
                .metrics
                .speed_samples,
            2
        );
        assert!(s.valid(&ids));
    }
}
