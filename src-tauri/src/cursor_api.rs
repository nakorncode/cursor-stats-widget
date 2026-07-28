use crate::auth::resolve_session_cookie;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

const CACHE_TTL_MS: u64 = 60_000;
const SCAN_24H_MS: u64 = 86_400_000;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentChat {
    pub conversation_id: String,
    pub cost_usd: f64,
    pub started_ms: u64,
    pub last_ms: u64,
    pub event_count: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub plan_percent_used: f64,
    pub plan_remaining_percent: f64,
    pub membership_type: String,
    /// Summed $ for the newest conversation (not a single event).
    pub last_task_cost_usd: Option<f64>,
    pub last_task_started_ms: Option<u64>,
    pub last_task_last_ms: Option<u64>,
    pub billing_cycle_end_ms: Option<u64>,
    pub days_left: f64,
    pub daily_budget_percent: f64,
    pub today_used_percent: f64,
    pub today_cost_usd: f64,
    pub pace_ratio: f64,
    /// under | on_track | over
    pub pace_label: String,
    pub recent_chats: Vec<RecentChat>,
    pub refreshed_at_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CostBucket {
    pub start_ms: u64,
    pub end_ms: u64,
    pub cost_usd: f64,
    pub event_count: u32,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CostSeries {
    pub range_secs: u64,
    pub bucket_secs: u64,
    pub total_usd: f64,
    pub buckets: Vec<CostBucket>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageSummary {
    #[serde(rename = "membershipType")]
    membership_type: Option<String>,
    #[serde(rename = "billingCycleEnd")]
    billing_cycle_end: Option<String>,
    #[serde(rename = "individualUsage")]
    individual_usage: Option<IndividualUsage>,
}

#[derive(Debug, Deserialize)]
struct IndividualUsage {
    plan: Option<PlanUsage>,
}

#[derive(Debug, Deserialize)]
struct PlanUsage {
    #[serde(rename = "totalPercentUsed")]
    total_percent_used: Option<f64>,
    #[serde(rename = "autoPercentUsed")]
    auto_percent_used: Option<f64>,
    #[serde(default)]
    used: Option<f64>,
    #[serde(default)]
    limit: Option<f64>,
    breakdown: Option<PlanBreakdown>,
}

#[derive(Debug, Deserialize)]
struct PlanBreakdown {
    #[serde(default)]
    total: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct FilteredEvents {
    #[serde(rename = "totalUsageEventsCount")]
    total_usage_events_count: Option<u32>,
    #[serde(rename = "usageEventsDisplay")]
    usage_events_display: Option<Vec<UsageEvent>>,
}

#[derive(Debug, Deserialize, Clone)]
struct UsageEvent {
    timestamp: Option<String>,
    model: Option<String>,
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(rename = "chargedCents")]
    charged_cents: Option<f64>,
    #[serde(rename = "requestsCosts")]
    requests_costs: Option<f64>,
    #[serde(rename = "tokenUsage")]
    token_usage: Option<TokenUsage>,
    #[serde(rename = "usageBasedCosts")]
    usage_based_costs: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct TokenUsage {
    #[serde(rename = "totalCents")]
    total_cents: Option<f64>,
}

struct EventsCache {
    fetched_at_ms: u64,
    start_ms: u64,
    end_ms: u64,
    events: Vec<UsageEvent>,
}

static EVENTS_CACHE: Mutex<Option<EventsCache>> = Mutex::new(None);

fn empty_snapshot(now: u64, error: Option<String>) -> UsageSnapshot {
    UsageSnapshot {
        plan_percent_used: 0.0,
        plan_remaining_percent: 0.0,
        membership_type: String::new(),
        last_task_cost_usd: None,
        last_task_started_ms: None,
        last_task_last_ms: None,
        billing_cycle_end_ms: None,
        days_left: 0.0,
        daily_budget_percent: 0.0,
        today_used_percent: 0.0,
        today_cost_usd: 0.0,
        pace_ratio: 0.0,
        pace_label: String::new(),
        recent_chats: vec![],
        refreshed_at_ms: now,
        error,
    }
}

pub async fn fetch_usage_snapshot(
    day_start_ms: u64,
    recent_limit: usize,
    bypass_cache: bool,
) -> UsageSnapshot {
    let now = now_ms();
    match fetch_usage_snapshot_inner(now, day_start_ms, recent_limit, bypass_cache).await {
        Ok(mut snap) => {
            snap.refreshed_at_ms = now;
            snap
        }
        Err(error) => empty_snapshot(now, Some(error)),
    }
}

pub async fn fetch_cost_series(range_secs: u64, bypass_cache: bool) -> CostSeries {
    let now = now_ms();
    let range_secs = range_secs.clamp(300, 86_400 * 7);
    let bucket_secs = pick_bucket_secs(range_secs);
    match fetch_cost_series_inner(now, range_secs, bucket_secs, bypass_cache).await {
        Ok(series) => series,
        Err(error) => CostSeries {
            range_secs,
            bucket_secs,
            total_usd: 0.0,
            buckets: vec![],
            error: Some(error),
        },
    }
}

async fn fetch_usage_snapshot_inner(
    now: u64,
    day_start_ms: u64,
    recent_limit: usize,
    bypass_cache: bool,
) -> Result<UsageSnapshot, String> {
    let cookie = resolve_session_cookie()?;
    let client = reqwest::Client::new();
    let cookie_header = format!("WorkosCursorSessionToken={cookie}");

    let summary = get_usage_summary(&client, &cookie_header).await?;
    let plan = summary
        .individual_usage
        .as_ref()
        .and_then(|u| u.plan.as_ref());

    let plan_percent_used = plan
        .and_then(|p| p.total_percent_used.or(p.auto_percent_used))
        .unwrap_or(0.0);
    let plan_remaining_percent = (100.0 - plan_percent_used).clamp(0.0, 100.0);

    let capacity = plan
        .and_then(|p| {
            p.breakdown
                .as_ref()
                .and_then(|b| b.total)
                .or(p.limit)
                .or(p.used)
        })
        .unwrap_or(0.0)
        .max(1.0);

    let billing_cycle_end_ms = summary
        .billing_cycle_end
        .as_ref()
        .and_then(|s| parse_iso_ms(s));

    let scan_start = now.saturating_sub(SCAN_24H_MS);
    let events_24h =
        get_events_cached(&client, &cookie_header, scan_start, now, bypass_cache).await?;

    let day_start = day_start_ms.min(now);
    let today_events: Vec<&UsageEvent> = events_24h
        .iter()
        .filter(|e| event_ts_ms(e).is_some_and(|ts| ts >= day_start && ts <= now))
        .collect();
    let today_cost_usd: f64 = today_events.iter().filter_map(|e| event_cost_usd(e)).sum();
    let today_requests: f64 = today_events.iter().filter_map(|e| e.requests_costs).sum();
    let today_used_percent = (today_requests / capacity) * 100.0;

    // Budget is fixed for the day: remaining at local midnight ÷ calendar days.
    // Using live remaining alone shrinks the budget as you spend today (18%→14.4% → 9%→7.2%).
    let (days_left, daily_budget_percent) = daily_budget_from_remaining(
        plan_remaining_percent,
        today_used_percent,
        day_start_ms,
        billing_cycle_end_ms,
    );

    let pace_ratio = if daily_budget_percent > 0.001 {
        today_used_percent / daily_budget_percent
    } else if today_used_percent > 0.0 {
        2.0
    } else {
        1.0
    };
    let pace_label = if pace_ratio < 0.7 {
        "under".into()
    } else if pace_ratio <= 1.3 {
        "on_track".into()
    } else {
        "over".into()
    };

    let chats = group_recent_chats(&events_24h, recent_limit.max(1));
    let top = chats.first();

    Ok(UsageSnapshot {
        plan_percent_used,
        plan_remaining_percent,
        membership_type: summary.membership_type.unwrap_or_default(),
        last_task_cost_usd: top.map(|c| c.cost_usd),
        last_task_started_ms: top.map(|c| c.started_ms),
        last_task_last_ms: top.map(|c| c.last_ms),
        billing_cycle_end_ms,
        days_left,
        daily_budget_percent,
        today_used_percent,
        today_cost_usd,
        pace_ratio,
        pace_label,
        recent_chats: if recent_limit == 0 {
            vec![]
        } else {
            chats
        },
        refreshed_at_ms: now,
        error: None,
    })
}

async fn fetch_cost_series_inner(
    now: u64,
    range_secs: u64,
    bucket_secs: u64,
    bypass_cache: bool,
) -> Result<CostSeries, String> {
    let cookie = resolve_session_cookie()?;
    let client = reqwest::Client::new();
    let cookie_header = format!("WorkosCursorSessionToken={cookie}");
    let start = now.saturating_sub(range_secs * 1000);

    let events = if range_secs <= 86_400 {
        get_events_cached(
            &client,
            &cookie_header,
            now.saturating_sub(SCAN_24H_MS),
            now,
            bypass_cache,
        )
        .await?
        .into_iter()
        .filter(|e| event_ts_ms(e).is_some_and(|ts| ts >= start && ts <= now))
        .collect()
    } else {
        fetch_all_events_in_range(&client, &cookie_header, start, now).await?
    };

    let bucket_ms = bucket_secs * 1000;
    let bucket_count = ((range_secs + bucket_secs - 1) / bucket_secs).max(1) as usize;
    let mut buckets: Vec<CostBucket> = (0..bucket_count)
        .map(|i| {
            let b_start = start + i as u64 * bucket_ms;
            CostBucket {
                start_ms: b_start,
                end_ms: b_start + bucket_ms,
                cost_usd: 0.0,
                event_count: 0,
            }
        })
        .collect();

    for event in &events {
        let Some(ts) = event_ts_ms(event) else {
            continue;
        };
        if ts < start || ts > now {
            continue;
        }
        let idx = ((ts - start) / bucket_ms) as usize;
        if let Some(bucket) = buckets.get_mut(idx) {
            bucket.cost_usd += event_cost_usd(event).unwrap_or(0.0);
            bucket.event_count += 1;
        }
    }

    let total_usd = buckets.iter().map(|b| b.cost_usd).sum();
    Ok(CostSeries {
        range_secs,
        bucket_secs,
        total_usd,
        buckets,
        error: None,
    })
}

fn group_recent_chats(events: &[UsageEvent], limit: usize) -> Vec<RecentChat> {
    // Cursor reuses one conversationId across a long day. Summing the whole id
    // makes "recent chat #1" look like all-day spend. Split into sessions when
    // idle gap exceeds SESSION_GAP_MS (ponytail: 30m; tighten if still too wide).
    const SESSION_GAP_MS: u64 = 30 * 60 * 1000;

    let mut by_conv: HashMap<String, Vec<(u64, f64)>> = HashMap::new();
    for event in events {
        let Some(ts) = event_ts_ms(event) else {
            continue;
        };
        let id = event
            .conversation_id
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "null")
            .unwrap_or_else(|| format!("event:{ts}"));
        let cost = event_cost_usd(event).unwrap_or(0.0);
        by_conv.entry(id).or_default().push((ts, cost));
    }

    let mut chats: Vec<RecentChat> = Vec::new();
    for (conv_id, mut list) in by_conv {
        list.sort_by_key(|(ts, _)| *ts);
        let mut session_start = list[0].0;
        let mut session_last = list[0].0;
        let mut session_cost = list[0].1;
        let mut session_events: u32 = 1;

        let flush = |chats: &mut Vec<RecentChat>,
                     conv_id: &str,
                     start: u64,
                     last: u64,
                     cost: f64,
                     n: u32| {
            chats.push(RecentChat {
                conversation_id: format!("{conv_id}#{start}"),
                cost_usd: cost,
                started_ms: start,
                last_ms: last,
                event_count: n,
            });
        };

        for &(ts, cost) in list.iter().skip(1) {
            if ts.saturating_sub(session_last) > SESSION_GAP_MS {
                flush(
                    &mut chats,
                    &conv_id,
                    session_start,
                    session_last,
                    session_cost,
                    session_events,
                );
                session_start = ts;
                session_last = ts;
                session_cost = cost;
                session_events = 1;
            } else {
                session_last = ts;
                session_cost += cost;
                session_events += 1;
            }
        }
        flush(
            &mut chats,
            &conv_id,
            session_start,
            session_last,
            session_cost,
            session_events,
        );
    }

    chats.sort_by(|a, b| b.last_ms.cmp(&a.last_ms));
    chats.truncate(limit);
    chats
}

fn pick_bucket_secs(range_secs: u64) -> u64 {
    match range_secs {
        0..=600 => 60,
        601..=1800 => 120,
        1801..=3600 => 300,
        3601..=10_800 => 600,
        10_801..=21_600 => 900,
        21_601..=43_200 => 1800,
        43_201..=86_400 => 3600,
        _ => 10_800,
    }
}

async fn get_events_cached(
    client: &reqwest::Client,
    cookie_header: &str,
    start_ms: u64,
    end_ms: u64,
    bypass_cache: bool,
) -> Result<Vec<UsageEvent>, String> {
    if !bypass_cache {
        if let Ok(guard) = EVENTS_CACHE.lock() {
            if let Some(cache) = guard.as_ref() {
                let fresh = end_ms.saturating_sub(cache.fetched_at_ms) <= CACHE_TTL_MS
                    || now_ms().saturating_sub(cache.fetched_at_ms) <= CACHE_TTL_MS;
                if fresh && cache.start_ms <= start_ms && cache.end_ms >= end_ms.saturating_sub(5_000)
                {
                    return Ok(cache
                        .events
                        .iter()
                        .filter(|e| {
                            event_ts_ms(e).is_some_and(|ts| ts >= start_ms && ts <= end_ms)
                        })
                        .cloned()
                        .collect());
                }
            }
        }
    }

    let events = fetch_all_events_in_range(client, cookie_header, start_ms, end_ms).await?;
    if let Ok(mut guard) = EVENTS_CACHE.lock() {
        *guard = Some(EventsCache {
            fetched_at_ms: now_ms(),
            start_ms,
            end_ms,
            events: events.clone(),
        });
    }
    Ok(events)
}

async fn get_usage_summary(
    client: &reqwest::Client,
    cookie_header: &str,
) -> Result<UsageSummary, String> {
    let res = client
        .get("https://cursor.com/api/usage-summary")
        .header("Cookie", cookie_header)
        .send()
        .await
        .map_err(|e| format!("usage-summary request: {e}"))?;
    ensure_ok(res, "usage-summary")
        .await?
        .json()
        .await
        .map_err(|e| format!("usage-summary json: {e}"))
}

async fn fetch_events_page(
    client: &reqwest::Client,
    cookie_header: &str,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    page: u32,
    page_size: u32,
) -> Result<FilteredEvents, String> {
    let mut body = serde_json::json!({
        "page": page,
        "pageSize": page_size,
    });
    if let Some(s) = start_ms {
        body["startDate"] = serde_json::Value::String(s.to_string());
    }
    if let Some(e) = end_ms {
        body["endDate"] = serde_json::Value::String(e.to_string());
    }

    let res = client
        .post("https://cursor.com/api/dashboard/get-filtered-usage-events")
        .header("Cookie", cookie_header)
        .header("Origin", "https://cursor.com")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("events request: {e}"))?;

    ensure_ok(res, "events")
        .await?
        .json()
        .await
        .map_err(|e| format!("events json: {e}"))
}

async fn fetch_all_events_in_range(
    client: &reqwest::Client,
    cookie_header: &str,
    start_ms: u64,
    end_ms: u64,
) -> Result<Vec<UsageEvent>, String> {
    // ponytail: cap pages so a busy day cannot stall the overlay
    const PAGE_SIZE: u32 = 100;
    const MAX_PAGES: u32 = 20;
    let mut page = 1;
    let mut out = Vec::new();
    loop {
        let batch = fetch_events_page(
            client,
            cookie_header,
            Some(start_ms),
            Some(end_ms),
            page,
            PAGE_SIZE,
        )
        .await?;
        let chunk = batch.usage_events_display.unwrap_or_default();
        let n = chunk.len() as u32;
        out.extend(chunk);
        let total = batch.total_usage_events_count.unwrap_or(out.len() as u32);
        if n < PAGE_SIZE || out.len() as u32 >= total || page >= MAX_PAGES {
            break;
        }
        page += 1;
    }
    Ok(out)
}

async fn ensure_ok(res: reqwest::Response, label: &str) -> Result<reqwest::Response, String> {
    let status = res.status();
    if status.as_u16() == 429 {
        return Err("rate limited by Cursor (429) — backing off".into());
    }
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(format!("{label} {status}: {body}"));
    }
    Ok(res)
}

fn event_cost_usd(event: &UsageEvent) -> Option<f64> {
    if let Some(cents) = event
        .charged_cents
        .or_else(|| event.token_usage.as_ref().and_then(|t| t.total_cents))
    {
        return Some(cents / 100.0);
    }
    event.usage_based_costs.as_ref().and_then(|s| {
        let cleaned = s.trim().trim_start_matches('$');
        if cleaned == "-" || cleaned.is_empty() {
            None
        } else {
            cleaned.parse::<f64>().ok()
        }
    })
}

fn event_ts_ms(event: &UsageEvent) -> Option<u64> {
    event.timestamp.as_ref()?.parse().ok()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn parse_iso_ms(s: &str) -> Option<u64> {
    let t = s.trim();
    let (date, rest) = t.split_once('T')?;
    let mut p = date.split('-');
    let y = p.next()?.parse::<i32>().ok()?;
    let mo = p.next()?.parse::<i32>().ok()?;
    let d = p.next()?.parse::<i32>().ok()?;
    let time = rest.trim_end_matches('Z');
    let (hms, _) = time.split_once('.').unwrap_or((time, "0"));
    let mut tp = hms.split(':');
    let h = tp.next()?.parse::<u32>().ok()?;
    let mi = tp.next()?.parse::<u32>().ok()?;
    let sec = tp.next()?.parse::<u32>().ok()?;
    days_from_civil(y, mo, d).map(|days| {
        let secs = days * 86_400 + h as i64 * 3600 + mi as i64 * 60 + sec as i64;
        (secs * 1000) as u64
    })
}

fn days_from_civil(y: i32, m: i32, d: i32) -> Option<i64> {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mpd = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mpd + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era as i64 * 146_097 + doe as i64 - 719_468)
}

/// Calendar days covering `[local day start, billing end]`, ceiled, at least 1.
/// Matches Cursor-style "N days left" better than raw hours/24 (which under-counts
/// when ~24h remain but today+tomorrow are both in play).
fn calendar_days_for_budget(day_start_ms: u64, billing_end_ms: u64) -> f64 {
    if billing_end_ms <= day_start_ms {
        return 1.0;
    }
    let span_days = (billing_end_ms - day_start_ms) as f64 / 86_400_000.0;
    span_days.ceil().max(1.0)
}

/// Returns `(days_left, daily_budget_percent)`.
///
/// Daily budget uses remaining **at local day start** (`liveRemaining + todayUsed`),
/// so spending today does not shrink the day's allowance.
fn daily_budget_from_remaining(
    plan_remaining_percent: f64,
    today_used_percent: f64,
    day_start_ms: u64,
    billing_cycle_end_ms: Option<u64>,
) -> (f64, f64) {
    let days = match billing_cycle_end_ms {
        Some(end) => calendar_days_for_budget(day_start_ms, end),
        None => 1.0,
    };
    let remaining_at_day_start = (plan_remaining_percent + today_used_percent.max(0.0)).max(0.0);
    (days, remaining_at_day_start / days)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400_000;

    fn ev(ts: u64, conv: &str, cents: f64) -> UsageEvent {
        UsageEvent {
            timestamp: Some(ts.to_string()),
            model: None,
            conversation_id: Some(conv.into()),
            charged_cents: Some(cents),
            requests_costs: None,
            token_usage: None,
            usage_based_costs: None,
        }
    }

    #[test]
    fn session_gap_splits_long_conversation() {
        let day = 1_700_000_000_000u64;
        let events = vec![
            ev(day, "c1", 100.0),                        // $1
            ev(day + 5 * 60_000, "c1", 50.0),            // +$0.50 same session
            ev(day + 2 * 3600_000, "c1", 200.0),         // $2 after 2h gap → new session
            ev(day + 2 * 3600_000 + 60_000, "c1", 25.0), // +$0.25
        ];
        let chats = group_recent_chats(&events, 10);
        assert_eq!(chats.len(), 2);
        // newest session first
        assert!((chats[0].cost_usd - 2.25).abs() < 0.001);
        assert!((chats[1].cost_usd - 1.5).abs() < 0.001);
    }

    #[test]
    fn daily_budget_halves_when_two_calendar_days_remain() {
        let day_start = 1_700_000_000_000u64;
        let end = day_start + 2 * DAY;
        let (days, budget) = daily_budget_from_remaining(18.0, 0.0, day_start, Some(end));
        assert!((days - 2.0).abs() < f64::EPSILON);
        assert!((budget - 9.0).abs() < 0.001);
    }

    /// Bug repro: ~24h from mid-morning → fractional days≈1 (wrong 18%),
    /// but calendar span from midnight covers today+tomorrow → 2 days → 9%.
    #[test]
    fn daily_budget_uses_calendar_days_not_fractional_24h() {
        let day_start = 1_700_000_000_000u64;
        let now = day_start + 9 * 3600_000; // 09:00 local
        let end = now + DAY; // 09:00 tomorrow (fractional remaining = 1.0)
        let (days, budget) = daily_budget_from_remaining(18.0, 0.0, day_start, Some(end));
        assert!((days - 2.0).abs() < f64::EPSILON, "days={days}");
        assert!((budget - 9.0).abs() < 0.001, "budget={budget}");
    }

    #[test]
    fn daily_budget_is_full_remaining_on_renewal_day() {
        let day_start = 1_700_000_000_000u64;
        let end = day_start + DAY / 2; // later today
        let (days, budget) = daily_budget_from_remaining(18.0, 0.0, day_start, Some(end));
        assert!((days - 1.0).abs() < f64::EPSILON);
        assert!((budget - 18.0).abs() < 0.001);
    }

    #[test]
    fn daily_budget_defaults_to_one_day_without_billing_end() {
        let (days, budget) = daily_budget_from_remaining(18.0, 0.0, 0, None);
        assert!((days - 1.0).abs() < f64::EPSILON);
        assert!((budget - 18.0).abs() < 0.001);
    }

    /// Bug repro: live remaining drops as you spend today (18→14.4), but the
    /// day's budget must stay 9% (= (14.4+3.6)/2), not shrink to 7.2%.
    #[test]
    fn daily_budget_stays_fixed_as_today_usage_grows() {
        let day_start = 1_700_000_000_000u64;
        let end = day_start + 2 * DAY;
        let morning = daily_budget_from_remaining(18.0, 0.0, day_start, Some(end));
        let later = daily_budget_from_remaining(14.4, 3.6, day_start, Some(end));
        assert!((morning.1 - 9.0).abs() < 0.001, "morning={}", morning.1);
        assert!((later.1 - 9.0).abs() < 0.001, "later={}", later.1);
        // Old buggy formula would have been 14.4/2 = 7.2
    }
}
