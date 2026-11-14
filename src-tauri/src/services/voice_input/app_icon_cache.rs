use std::collections::VecDeque;
use std::time::{Duration, Instant};

const CAPACITY: usize = 64;
const TTL: Duration = Duration::from_secs(300);
const MAX_ICON_BYTES: usize = 256 * 1024;

struct Entry {
    key: String,
    icon: Option<String>,
    created: Instant,
}

#[derive(Default)]
pub(crate) struct Cache {
    entries: VecDeque<Entry>,
}

impl Cache {
    pub fn get(&mut self, key: &str, now: Instant) -> Option<Option<String>> {
        let index = self.entries.iter().position(|entry| entry.key == key)?;
        let entry = self.entries.remove(index).unwrap();
        if now.duration_since(entry.created) >= TTL {
            return None;
        }
        let icon = entry.icon.clone();
        self.entries.push_back(entry);
        Some(icon)
    }

    pub fn insert(&mut self, key: String, icon: Option<String>, now: Instant) {
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            self.entries.remove(index);
        }
        if icon
            .as_ref()
            .is_some_and(|icon| icon.len() > MAX_ICON_BYTES)
        {
            return;
        }
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            key,
            icon,
            created: now,
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn read(
        &mut self,
        key: Option<String>,
        now: Instant,
        load: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        let Some(key) = key else { return load() };
        if let Some(icon) = self.get(&key, now) {
            return icon;
        }
        let icon = load()?;
        self.insert(key, Some(icon.clone()), now);
        Some(icon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn hits_skip_the_loader_and_keep_applications_separate() {
        let mut cache = Cache::default();
        let now = Instant::now();
        let calls = Cell::new(0);
        let load = || {
            calls.set(calls.get() + 1);
            Some(format!("icon-{}", calls.get()))
        };
        for (key, expected) in [
            ("first", "icon-1"),
            ("second", "icon-2"),
            ("first", "icon-1"),
        ] {
            assert_eq!(
                cache.read(Some(key.into()), now, load).as_deref(),
                Some(expected)
            );
        }
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn expiry_is_absolute_and_a_failed_refresh_is_retried() {
        let mut cache = Cache::default();
        let now = Instant::now();
        let key = Some("app".into());
        cache.read(key.clone(), now, || Some("old".into()));
        assert_eq!(
            cache.read(key.clone(), now + TTL - Duration::from_secs(1), || panic!(
                "cache hit"
            )),
            Some("old".into())
        );
        assert!(cache.read(key.clone(), now + TTL, || None).is_none());
        assert_eq!(
            cache.read(key.clone(), now + TTL, || Some("new".into())),
            Some("new".into())
        );
        assert_eq!(
            cache.read(key, now + TTL, || panic!("cache hit")),
            Some("new".into())
        );
    }

    #[test]
    fn missing_identity_failed_reads_and_oversized_icons_are_not_cached() {
        let mut cache = Cache::default();
        let now = Instant::now();
        let calls = Cell::new(0);
        for key in [None, Some("failed".into()), Some("large".into())] {
            for _ in 0..2 {
                cache.read(key.clone(), now, || {
                    calls.set(calls.get() + 1);
                    match key.as_deref() {
                        Some("failed") => None,
                        Some("large") => Some("x".repeat(MAX_ICON_BYTES + 1)),
                        _ => Some("icon".into()),
                    }
                });
            }
        }
        assert_eq!(calls.get(), 6);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn explicit_missing_icons_expire_and_refresh_releases_cached_entries() {
        let mut cache = Cache::default();
        let now = Instant::now();
        cache.insert("missing".into(), None, now);
        assert_eq!(cache.get("missing", now), Some(None));
        assert_eq!(cache.get("missing", now + TTL), None);
        cache.insert("ready".into(), Some("icon".into()), now + TTL);
        assert_eq!(cache.get("ready", now + TTL), Some(Some("icon".into())));
        cache.clear();
        assert!(cache.entries.is_empty());
        assert_eq!(cache.get("ready", now + TTL), None);
        cache.insert(
            "large".into(),
            Some("x".repeat(MAX_ICON_BYTES + 1)),
            now + TTL,
        );
        assert_eq!(cache.get("large", now + TTL), None);
    }

    #[test]
    fn eviction_keeps_recently_used_icons_and_bounds_memory() {
        let mut cache = Cache::default();
        let now = Instant::now();
        for index in 0..CAPACITY {
            cache.read(Some(index.to_string()), now, || Some(index.to_string()));
        }
        assert_eq!(
            cache.read(Some("0".into()), now, || panic!("cache hit")),
            Some("0".into())
        );
        cache.read(Some("extra".into()), now, || Some("extra".into()));
        assert_eq!(cache.entries.len(), CAPACITY);
        assert_eq!(
            cache.read(Some("0".into()), now, || panic!("cache hit")),
            Some("0".into())
        );
        assert_eq!(
            cache.read(Some("1".into()), now, || Some("reloaded".into())),
            Some("reloaded".into())
        );
        assert_eq!(cache.entries.len(), CAPACITY);
    }
}
