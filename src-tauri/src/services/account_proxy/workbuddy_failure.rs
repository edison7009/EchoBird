use super::ProxyError;
use axum::http::{HeaderValue, StatusCode};
use chrono::{FixedOffset, NaiveDateTime, TimeZone};
pub(super) fn normalize(mut error: ProxyError) -> ProxyError {
    match error.error_code.as_deref() {
        Some("11128") => {
            error.error_code = Some("request_incompatible".into());
            error.status = StatusCode::BAD_REQUEST;
            error.model_scope = true;
        }
        Some("6004") => {
            error.status = StatusCode::TOO_MANY_REQUESTS;
            error.model_scope = true;
            let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
            if let Some(reset) = written_reset(&error.message, now) {
                error.retry_after =
                    HeaderValue::from_str(&(reset.saturating_sub(now).div_ceil(1000)).to_string())
                        .ok();
            }
        }
        _ => {}
    }
    error
}
fn written_reset(message: &str, now: u64) -> Option<u64> {
    for zone in ["UTC+08:00", "UTC+8"] {
        for (offset, _) in message.match_indices(zone) {
            if message[offset + zone.len()..].starts_with(|c: char| c.is_ascii_digit() || c == ':')
            {
                continue;
            }
            let before = message[..offset]
                .trim_end()
                .trim_end_matches('(')
                .trim_end();
            let Some(date) = before.get(before.len().saturating_sub(19)..) else {
                continue;
            };
            let Ok(date) =
                NaiveDateTime::parse_from_str(&date.replace('T', " "), "%Y-%m-%d %H:%M:%S")
            else {
                continue;
            };
            let reset = FixedOffset::east_opt(8 * 3600)?
                .from_local_datetime(&date)
                .single()?
                .timestamp_millis();
            if let Ok(reset) = u64::try_from(reset) {
                if reset > now && reset - now <= 7 * 24 * 60 * 60 * 1000 {
                    return Some(reset);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_time_has_an_explicit_beijing_offset_and_bounded_future_deadline() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-22T00:00:00Z")
            .unwrap()
            .timestamp_millis() as u64;
        for message in [
            "于 2026-10-22 09:00:00 (UTC+8) 可用",
            "will reset at 2026-10-22 09:00:00 UTC+08:00, alternatively use another model",
        ] {
            assert_eq!(written_reset(message, now), Some(now + 3_600_000));
        }
        for message in [
            "2026-10-22 09:00:00",
            "2026-10-22 09:00:00 UTC+0",
            "2026-10-22 09:00:00 UTC+80",
            "2026-02-30 09:00:00 UTC+8",
            "2026-10-22 07:59:59 UTC+8",
            "2026-11-22 09:00:00 UTC+8",
            "2026-10-22 09:00 UTC+8",
            "2026-10-22 09:00:00 UTC+8:30",
        ] {
            assert_eq!(written_reset(message, now), None, "{message}");
        }
    }

    #[test]
    fn typed_vendor_failures_become_standard_model_scoped_errors() {
        let mut error = ProxyError::new(StatusCode::BAD_REQUEST, "limit");
        error.error_code = Some("6004".into());
        let error = normalize(error);
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
        assert!(error.model_scope);
        let mut error = ProxyError::new(StatusCode::BAD_GATEWAY, "refused");
        error.error_code = Some("11128".into());
        let error = normalize(error);
        assert_eq!(error.error_code.as_deref(), Some("request_incompatible"));
        assert!(error.model_scope);
    }
}
