//! TokenPet reconciliation for local history and metered router responses.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(super) const DAYS: i64 = 14;
const DAY_SECONDS: i64 = 86_400;
pub(super) type NativeTotals = BTreeMap<i64, BTreeMap<String, u64>>;

pub(super) fn day(seconds: i64) -> i64 {
    seconds.div_euclid(DAY_SECONDS)
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageDay {
    router: u64,
    native: BTreeMap<String, u64>,
}

impl UsageDay {
    fn extra(&self) -> u64 {
        self.native
            .values()
            .copied()
            .fold(0, u64::saturating_add)
            .saturating_sub(self.router)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct History {
    since: Option<i64>,
    closed_before: i64,
    closed_extra: u64,
    days: BTreeMap<i64, UsageDay>,
}

impl History {
    pub(super) fn valid(&self) -> bool {
        self.closed_before >= 0
            && self.since.is_none_or(|since| since >= 0)
            && self.days.len() <= DAYS as usize
            && self.days.keys().all(|day| *day >= self.closed_before)
    }

    pub(super) fn since(&self, now: i64, router_total: u64, wallet_reset: bool) -> i64 {
        let oldest = self.closed_before.max(day(now) - (DAYS - 1));
        let since = self.since.unwrap_or_else(|| {
            let covered = self
                .days
                .values()
                .fold(0u64, |total, entry| total.saturating_add(entry.router));
            if !wallet_reset && covered == router_total {
                oldest * DAY_SECONDS
            } else {
                // Older ledgers only kept a lifetime total. Its unknown dates
                // cannot be matched against records already on disk.
                now.saturating_add(1)
            }
        });
        since.max(oldest * DAY_SECONDS)
    }

    pub(super) fn trim(&mut self, now: i64) {
        let oldest = day(now) - (DAYS - 1);
        if oldest <= self.closed_before {
            return;
        }
        self.closed_before = oldest;
        let open = self.days.split_off(&self.closed_before);
        for usage in self.days.values() {
            self.closed_extra = self.closed_extra.saturating_add(usage.extra());
        }
        self.days = open;
    }

    pub(super) fn record_router(&mut self, now: i64, tokens: u64) {
        let day = day(now);
        if tokens > 0 && day >= self.closed_before {
            let usage = self.days.entry(day).or_default();
            usage.router = usage.router.saturating_add(tokens);
        }
    }

    pub(super) fn merge_native(&mut self, since: i64, now: i64, totals: NativeTotals) -> bool {
        let mut changed = self.since.is_none();
        self.since.get_or_insert(since);
        let last = day(now);
        for (day, families) in totals {
            if (self.closed_before..=last).contains(&day) {
                // Retain each family's high-water mark when its logs disappear.
                for (family, total) in families.into_iter().filter(|(_, total)| *total > 0) {
                    let credited = self
                        .days
                        .entry(day)
                        .or_default()
                        .native
                        .entry(family)
                        .or_default();
                    if total > *credited {
                        *credited = total;
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    pub(super) fn extra(&self) -> u64 {
        self.days.values().fold(self.closed_extra, |total, day| {
            total.saturating_add(day.extra())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 20_000 * DAY_SECONDS + 15;

    #[test]
    fn overlapping_router_usage_does_not_credit_twice_even_if_it_arrives_later() {
        let mut history = History::default();
        history.trim(NOW);
        let since = history.since(NOW, 0, false);
        let totals = BTreeMap::from([
            (day(NOW), BTreeMap::from([("codex".into(), 90)])),
            (day(NOW) - 1, BTreeMap::from([("codex".into(), 140)])),
        ]);
        history.merge_native(since, NOW, totals.clone());
        assert_eq!(history.extra(), 230);

        history.record_router(NOW, 75);
        assert_eq!(75 + history.extra(), 230);
        history.record_router(NOW, 45);
        assert_eq!(120 + history.extra(), 260);
        history.merge_native(since, NOW, BTreeMap::new());
        history.merge_native(since, NOW, totals);
        assert_eq!(120 + history.extra(), 260);
    }

    #[test]
    fn legacy_credits_start_native_earnings_after_activation_and_survive_reload() {
        let mut history = History::default();
        let since = history.since(NOW, 600, false);
        assert_eq!(since, NOW + 1);
        history.trim(NOW);
        history.merge_native(since, NOW, BTreeMap::new());
        history = serde_json::from_str(&serde_json::to_string(&history).unwrap()).unwrap();
        assert_eq!(history.since(NOW + 50, 600, false), since);

        let totals = BTreeMap::from([(day(NOW), BTreeMap::from([("claude".into(), 50)]))]);
        history.merge_native(since, NOW + 50, totals.clone());
        history.record_router(NOW + 50, 15);
        assert_eq!(history.extra(), 35);
        let rollover = NOW + DAYS * DAY_SECONDS;
        history.trim(rollover);
        assert_eq!(history.extra(), 35);
        history.merge_native(since, rollover, totals);
        assert_eq!(history.extra(), 35);
        assert!(history.valid());
    }

    #[test]
    fn a_missing_family_does_not_mask_another_familys_new_tokens() {
        let mut history = History::default();
        history.trim(NOW);
        assert_eq!(history.since(NOW, 0, true), NOW + 1);
        let since = history.since(NOW, 0, false);
        let mut merge = |family: &str, total| {
            let totals = BTreeMap::from([(day(NOW), BTreeMap::from([(family.into(), total)]))]);
            history.merge_native(since, NOW, totals);
        };
        merge("claude", 200);
        merge("codex", 150);
        assert_eq!(history.extra(), 350);
    }
}
