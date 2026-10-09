//! Schedule math for local automations: a semantic port of the renderer's
//! `src/shared/automation-schedule-parsing.ts`,
//! `src/shared/automation-schedule-occurrences.ts`, and
//! `src/shared/automation-cron-occurrence.ts`.
//!
//! All timestamps are Unix milliseconds evaluated in the machine's local wall
//! clock (`chrono::Local`); a schedule's timezone is stored but never used for
//! math (spec §3.1). DST disambiguation is fixed to the renderer's `Date`
//! "compatible" semantics: a nonexistent local time shifts forward by the gap
//! length (e.g. 02:30 → 03:30 across a 1-hour spring-forward), and an ambiguous
//! local time resolves to the earliest instant.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike};

pub const AUTOMATION_CRON_EXPRESSION_MAX_BYTES: usize = 2 * 1024;

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
// Why: valid cron expressions like Feb 29 can have an 8-year gap across
// non-leap centuries.
const CRON_SCAN_MINUTES: i64 = 9 * 366 * 24 * 60;
const RRULE_SCAN_DAYS: i64 = 370;
const DAY_CODES: [&str; 7] = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"];

const DAY_NAMES: [(&str, i64); 14] = [
    ("SU", 0),
    ("MO", 1),
    ("TU", 2),
    ("WE", 3),
    ("TH", 4),
    ("FR", 5),
    ("SA", 6),
    ("SUN", 0),
    ("MON", 1),
    ("TUE", 2),
    ("WED", 3),
    ("THU", 4),
    ("FRI", 5),
    ("SAT", 6),
];

const MONTH_NAMES: [(&str, i64); 12] = [
    ("JAN", 1),
    ("FEB", 2),
    ("MAR", 3),
    ("APR", 4),
    ("MAY", 5),
    ("JUN", 6),
    ("JUL", 7),
    ("AUG", 8),
    ("SEP", 9),
    ("OCT", 10),
    ("NOV", 11),
    ("DEC", 12),
];

/// Errors produced while parsing or evaluating a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("invalid automation schedule")]
    Invalid,
    #[error("automation schedule has no occurrence within the scan window")]
    NoOccurrence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedSchedule {
    Rrule(ParsedRrule),
    Cron(ParsedCron),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RruleFrequency {
    Hourly,
    Daily,
    Weekly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRrule {
    frequency: RruleFrequency,
    by_day: Vec<String>,
    by_hour: u32,
    by_minute: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCron {
    minutes: BTreeSet<u32>,
    hours: BTreeSet<u32>,
    days_of_month: BTreeSet<u32>,
    months: BTreeSet<u32>,
    days_of_week: BTreeSet<u32>,
    day_of_month_restricted: bool,
    day_of_week_restricted: bool,
}

/// Accepts a schedule as new input: oversized cron steps are refused (#15895)
/// and a cron schedule must produce at least one occurrence within the
/// 9×366-day scan window.
pub fn is_valid_schedule(text: &str) -> bool {
    match parse_schedule(text, true) {
        Err(_) => false,
        Ok(ParsedSchedule::Rrule(_)) => true,
        Ok(ParsedSchedule::Cron(cron)) => {
            let now = Local::now().timestamp_millis();
            next_cron_occurrence(&cron, now, now).is_ok()
        }
    }
}

/// First occurrence strictly after `after_ms` and at or after `dtstart_ms`;
/// `None` when the schedule is invalid or has no occurrence in the scan window.
pub fn next_occurrence_after(text: &str, dtstart_ms: i64, after_ms: i64) -> Option<i64> {
    let parsed = parse_schedule(text, false).ok()?;
    next_occurrence_after_parsed(&parsed, dtstart_ms, after_ms).ok()
}

/// Latest occurrence at or before `now_ms` and at or after `dtstart_ms`; `None`
/// when `now_ms < dtstart_ms`, the schedule is invalid, or none was found.
pub fn latest_occurrence_at_or_before(text: &str, dtstart_ms: i64, now_ms: i64) -> Option<i64> {
    if now_ms < dtstart_ms {
        return None;
    }
    let parsed = parse_schedule(text, false).ok()?;
    latest_occurrence_parsed(&parsed, dtstart_ms, now_ms).ok()
}

fn next_occurrence_after_parsed(
    parsed: &ParsedSchedule,
    dtstart_ms: i64,
    after_ms: i64,
) -> Result<i64, ScheduleError> {
    match parsed {
        ParsedSchedule::Cron(cron) => next_cron_occurrence(cron, dtstart_ms, after_ms),
        ParsedSchedule::Rrule(rule) => match rule.frequency {
            RruleFrequency::Hourly => {
                let start = local_datetime(dtstart_ms.max(after_ms));
                let mut candidate =
                    from_local_parts(start.date_naive(), start.hour(), rule.by_minute);
                if candidate <= after_ms || candidate < dtstart_ms {
                    candidate += HOUR_MS;
                }
                Ok(candidate)
            }
            RruleFrequency::Daily | RruleFrequency::Weekly => {
                scan_rrule_day_candidates(rule, (dtstart_ms - 1).max(after_ms), 1)
            }
        },
    }
}

fn latest_occurrence_parsed(
    parsed: &ParsedSchedule,
    dtstart_ms: i64,
    now_ms: i64,
) -> Result<i64, ScheduleError> {
    match parsed {
        ParsedSchedule::Cron(cron) => latest_cron_occurrence(cron, dtstart_ms, now_ms),
        ParsedSchedule::Rrule(rule) => match rule.frequency {
            RruleFrequency::Hourly => {
                let now = local_datetime(now_ms);
                let mut candidate = from_local_parts(now.date_naive(), now.hour(), rule.by_minute);
                if candidate > now_ms {
                    candidate -= HOUR_MS;
                }
                if candidate >= dtstart_ms {
                    Ok(candidate)
                } else {
                    Err(ScheduleError::NoOccurrence)
                }
            }
            RruleFrequency::Daily | RruleFrequency::Weekly => {
                let candidate = scan_rrule_day_candidates(rule, now_ms, -1)?;
                if candidate >= dtstart_ms {
                    Ok(candidate)
                } else {
                    Err(ScheduleError::NoOccurrence)
                }
            }
        },
    }
}

fn parse_schedule(
    text: &str,
    reject_oversized_step: bool,
) -> Result<ParsedSchedule, ScheduleError> {
    let trimmed = trim_js_whitespace(text);
    if trimmed.contains('=') {
        parse_rrule(trimmed).map(ParsedSchedule::Rrule)
    } else {
        parse_cron_expression(trimmed, reject_oversized_step).map(ParsedSchedule::Cron)
    }
}

fn parse_rrule(rrule: &str) -> Result<ParsedRrule, ScheduleError> {
    let mut entries: HashMap<String, String> = HashMap::new();
    for part in rrule.split(';') {
        let mut segments = part.split('=');
        let key = segments.next().unwrap_or("");
        let value = segments.next().unwrap_or("");
        if !key.is_empty() && !value.is_empty() {
            entries.insert(key.to_uppercase(), value.to_string());
        }
    }
    let frequency = match entries.get("FREQ").map(String::as_str) {
        Some("HOURLY") => RruleFrequency::Hourly,
        Some("DAILY") => RruleFrequency::Daily,
        Some("WEEKLY") => RruleFrequency::Weekly,
        _ => return Err(ScheduleError::Invalid),
    };
    let by_hour = parse_js_number(entries.get("BYHOUR").map(String::as_str).unwrap_or("9"));
    if !is_js_integer(by_hour) || !(0.0..=23.0).contains(&by_hour) {
        return Err(ScheduleError::Invalid);
    }
    let by_minute = parse_js_number(entries.get("BYMINUTE").map(String::as_str).unwrap_or("0"));
    if !is_js_integer(by_minute) || !(0.0..=59.0).contains(&by_minute) {
        return Err(ScheduleError::Invalid);
    }
    let by_day: Vec<String> = entries
        .get("BYDAY")
        .map(|value| {
            value
                .split(',')
                .filter(|day| !day.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if frequency == RruleFrequency::Weekly
        && (by_day.is_empty() || by_day.iter().any(|day| !DAY_CODES.contains(&day.as_str())))
    {
        return Err(ScheduleError::Invalid);
    }
    Ok(ParsedRrule {
        frequency,
        by_day,
        by_hour: by_hour as u32,
        by_minute: by_minute as u32,
    })
}

fn parse_cron_expression(
    expression: &str,
    reject_oversized_step: bool,
) -> Result<ParsedCron, ScheduleError> {
    let fields = cron_expression_fields(expression, 6);
    if fields.len() != 5 {
        return Err(ScheduleError::Invalid);
    }
    let days_of_month = parse_cron_field(
        CronFieldSpec {
            value: fields[2],
            min: 1,
            max: 31,
            names: None,
            normalize_day_of_week: false,
            distinct_value_count: None,
        },
        reject_oversized_step,
    )?;
    let days_of_week = parse_cron_field(
        CronFieldSpec {
            value: fields[4],
            min: 0,
            max: 7,
            names: Some(&DAY_NAMES),
            normalize_day_of_week: true,
            distinct_value_count: Some(7),
        },
        reject_oversized_step,
    )?;
    let minutes = parse_cron_field(
        CronFieldSpec {
            value: fields[0],
            min: 0,
            max: 59,
            names: None,
            normalize_day_of_week: false,
            distinct_value_count: None,
        },
        reject_oversized_step,
    )?;
    let hours = parse_cron_field(
        CronFieldSpec {
            value: fields[1],
            min: 0,
            max: 23,
            names: None,
            normalize_day_of_week: false,
            distinct_value_count: None,
        },
        reject_oversized_step,
    )?;
    let months = parse_cron_field(
        CronFieldSpec {
            value: fields[3],
            min: 1,
            max: 12,
            names: Some(&MONTH_NAMES),
            normalize_day_of_week: false,
            distinct_value_count: None,
        },
        reject_oversized_step,
    )?;
    Ok(ParsedCron {
        day_of_month_restricted: days_of_month.len() != 31,
        day_of_week_restricted: days_of_week.len() != 7,
        minutes,
        hours,
        days_of_month,
        months,
        days_of_week,
    })
}

fn cron_expression_fields(expression: &str, max_fields: usize) -> Vec<&str> {
    if expression.len() > AUTOMATION_CRON_EXPRESSION_MAX_BYTES {
        return Vec::new();
    }
    let mut fields = Vec::new();
    let mut token_start: Option<usize> = None;
    for (index, ch) in expression.char_indices() {
        if !is_js_whitespace(ch) {
            if token_start.is_none() {
                token_start = Some(index);
            }
            continue;
        }
        if let Some(start) = token_start.take() {
            fields.push(&expression[start..index]);
            if fields.len() >= max_fields {
                return fields;
            }
        }
    }
    if let Some(start) = token_start {
        fields.push(&expression[start..]);
    }
    fields
}

struct CronFieldSpec<'a> {
    value: &'a str,
    min: i64,
    max: i64,
    names: Option<&'a [(&'a str, i64)]>,
    normalize_day_of_week: bool,
    distinct_value_count: Option<i64>,
}

fn parse_cron_field(
    spec: CronFieldSpec<'_>,
    reject_oversized_step: bool,
) -> Result<BTreeSet<u32>, ScheduleError> {
    let mut result = BTreeSet::new();
    for raw_part in spec.value.split(',') {
        let part = trim_js_whitespace(raw_part);
        if part.is_empty() {
            return Err(ScheduleError::Invalid);
        }
        let mut step_segments = part.split('/');
        let range_part = step_segments.next().unwrap_or("");
        let step_part = step_segments.next();
        if step_segments.next().is_some() || range_part.is_empty() {
            return Err(ScheduleError::Invalid);
        }
        let step = match step_part {
            None => 1.0,
            Some(text) => parse_js_number(text),
        };
        if !is_js_integer(step) || step < 1.0 {
            return Err(ScheduleError::Invalid);
        }
        let domain_size = spec.distinct_value_count.unwrap_or(spec.max - spec.min + 1) as f64;
        if reject_oversized_step && step > domain_size {
            return Err(ScheduleError::Invalid);
        }
        let (start, end) = if range_part == "*" {
            (spec.min, spec.max)
        } else if range_part.contains('-') {
            let range_parts: Vec<&str> = range_part.split('-').collect();
            if range_parts.len() != 2 || range_parts[0].is_empty() || range_parts[1].is_empty() {
                return Err(ScheduleError::Invalid);
            }
            (
                parse_cron_number(range_parts[0], spec.names)?,
                parse_cron_number(range_parts[1], spec.names)?,
            )
        } else {
            let single = parse_cron_number(range_part, spec.names)?;
            (single, single)
        };
        let normalize = |value: i64| -> i64 {
            if spec.normalize_day_of_week && value == 7 {
                0
            } else {
                value
            }
        };
        let normalized_start = normalize(start);
        let normalized_end = normalize(end);
        if start < spec.min
            || start > spec.max
            || end < spec.min
            || end > spec.max
            || normalized_start < spec.min
            || normalized_start > spec.max
            || normalized_end < spec.min
            || normalized_end > spec.max
            || start > end
        {
            return Err(ScheduleError::Invalid);
        }
        let step = step as i64;
        let mut value = start;
        while value <= end {
            result.insert(normalize(value) as u32);
            if value > end - step {
                break;
            }
            value += step;
        }
    }
    if result.is_empty() {
        return Err(ScheduleError::Invalid);
    }
    Ok(result)
}

fn parse_cron_number(value: &str, names: Option<&[(&str, i64)]>) -> Result<i64, ScheduleError> {
    let normalized = value.to_uppercase();
    let named = names.and_then(|names| {
        names
            .iter()
            .find(|(name, _)| *name == normalized.as_str())
            .map(|(_, number)| *number as f64)
    });
    let parsed = named.unwrap_or_else(|| parse_js_number(&normalized));
    if !is_js_integer(parsed) {
        return Err(ScheduleError::Invalid);
    }
    Ok(parsed as i64)
}

/// `Number(value)` semantics for the subset cron/RRULE fields can express.
fn parse_js_number(raw: &str) -> f64 {
    let trimmed = trim_js_whitespace(raw);
    if trimmed.is_empty() {
        return 0.0;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if let Some(hex) = lowered.strip_prefix("0x") {
        return i64::from_str_radix(hex, 16)
            .map(|value| value as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(octal) = lowered.strip_prefix("0o") {
        return i64::from_str_radix(octal, 8)
            .map(|value| value as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(binary) = lowered.strip_prefix("0b") {
        return i64::from_str_radix(binary, 2)
            .map(|value| value as f64)
            .unwrap_or(f64::NAN);
    }
    if matches!(lowered.as_str(), "infinity" | "+infinity") {
        return f64::INFINITY;
    }
    if lowered == "-infinity" {
        return f64::NEG_INFINITY;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

fn is_js_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'..='\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

fn trim_js_whitespace(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

fn local_datetime(timestamp_ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .expect("timestamp within chrono's representable range")
}

/// Local wall clock -> absolute instant, matching the renderer's `Date`
/// disambiguation: ambiguous times resolve to the earliest instant, and
/// nonexistent times (DST gaps) shift forward by the gap length.
///
/// Real time zones only ever offer one or two offsets near a wall time, so the
/// offsets sampled around the UTC guess yield every candidate instant. Those
/// whose local time round-trips are real (two of them means ambiguous, take the
/// earliest); when none round-trips the wall time is in a gap, and applying the
/// smallest (pre-transition) offset shifts it forward by exactly the gap.
fn from_local_naive(naive: NaiveDateTime) -> i64 {
    let utc_guess_ms = naive.and_utc().timestamp_millis();
    let mut offsets = vec![
        offset_seconds_at(utc_guess_ms - 26 * HOUR_MS),
        offset_seconds_at(utc_guess_ms),
        offset_seconds_at(utc_guess_ms + 26 * HOUR_MS),
    ];
    offsets.sort_unstable();
    offsets.dedup();
    let matches: Vec<i64> = offsets
        .iter()
        .filter_map(|offset| {
            let candidate = utc_guess_ms - offset * 1000;
            (local_datetime(candidate).naive_local() == naive).then_some(candidate)
        })
        .collect();
    if let Some(earliest) = matches.iter().min() {
        return *earliest;
    }
    utc_guess_ms - offsets[0] * 1000
}

fn offset_seconds_at(instant_ms: i64) -> i64 {
    local_datetime(instant_ms).offset().fix().local_minus_utc() as i64
}

fn from_local_parts(date: NaiveDate, hour: u32, minute: u32) -> i64 {
    let naive = date
        .and_hms_opt(hour, minute, 0)
        .expect("hour and minute validated by the schedule parser");
    from_local_naive(naive)
}

fn start_of_local_day(timestamp_ms: i64) -> i64 {
    let local = local_datetime(timestamp_ms);
    from_local_parts(local.date_naive(), 0, 0)
}

fn start_of_next_local_day(timestamp_ms: i64) -> i64 {
    let local = local_datetime(timestamp_ms);
    let next_date = local
        .date_naive()
        .succ_opt()
        .expect("date within chrono's representable range");
    from_local_parts(next_date, 0, 0)
}

fn at_local_time(day_ms: i64, hour: u32, minute: u32) -> i64 {
    let local = local_datetime(day_ms);
    from_local_parts(local.date_naive(), hour, minute)
}

fn floor_to_minute(timestamp_ms: i64) -> i64 {
    let local = local_datetime(timestamp_ms);
    from_local_parts(local.date_naive(), local.hour(), local.minute())
}

fn scan_rrule_day_candidates(
    rule: &ParsedRrule,
    anchor_ms: i64,
    direction: i64,
) -> Result<i64, ScheduleError> {
    let mut day_ms = start_of_local_day(anchor_ms);
    for _ in 0..RRULE_SCAN_DAYS {
        let candidate = at_local_time(day_ms, rule.by_hour, rule.by_minute);
        if rrule_day_matches(rule, candidate)
            && ((direction == 1 && candidate > anchor_ms)
                || (direction == -1 && candidate <= anchor_ms))
        {
            return Ok(candidate);
        }
        day_ms += direction * DAY_MS;
    }
    Err(ScheduleError::NoOccurrence)
}

fn rrule_day_matches(rule: &ParsedRrule, candidate_ms: i64) -> bool {
    if rule.frequency == RruleFrequency::Daily {
        return true;
    }
    let code = DAY_CODES[local_datetime(candidate_ms)
        .weekday()
        .num_days_from_sunday() as usize];
    rule.by_day.iter().any(|day| day.as_str() == code)
}

fn cron_date_matches(cron: &ParsedCron, timestamp_ms: i64) -> bool {
    let local = local_datetime(timestamp_ms);
    if !cron.months.contains(&local.month()) {
        return false;
    }
    let day_of_month_matches = cron.days_of_month.contains(&local.day());
    let day_of_week_matches = cron
        .days_of_week
        .contains(&local.weekday().num_days_from_sunday());
    if cron.day_of_month_restricted && cron.day_of_week_restricted {
        return day_of_month_matches || day_of_week_matches;
    }
    day_of_month_matches && day_of_week_matches
}

fn cron_time_matches(cron: &ParsedCron, timestamp_ms: i64) -> bool {
    let local = local_datetime(timestamp_ms);
    cron.hours.contains(&local.hour()) && cron.minutes.contains(&local.minute())
}

/// Minute scan with one optimization: when the current local date cannot match,
/// jump straight to the next local day, consuming the same scan budget the
/// literal minute-by-minute loop would have consumed.
fn next_cron_occurrence(
    cron: &ParsedCron,
    dtstart_ms: i64,
    after_ms: i64,
) -> Result<i64, ScheduleError> {
    let mut candidate = floor_to_minute(dtstart_ms.max(after_ms));
    if candidate <= after_ms {
        candidate += MINUTE_MS;
    }
    if candidate < dtstart_ms {
        candidate = floor_to_minute(dtstart_ms);
        if candidate < dtstart_ms {
            candidate += MINUTE_MS;
        }
    }
    let mut remaining = CRON_SCAN_MINUTES;
    while remaining > 0 {
        if cron_date_matches(cron, candidate) {
            if cron_time_matches(cron, candidate) {
                return Ok(candidate);
            }
            candidate += MINUTE_MS;
            remaining -= 1;
        } else {
            let next_day = start_of_next_local_day(candidate);
            remaining -= (next_day - candidate) / MINUTE_MS;
            candidate = next_day;
        }
    }
    Err(ScheduleError::NoOccurrence)
}

fn latest_cron_occurrence(
    cron: &ParsedCron,
    dtstart_ms: i64,
    now_ms: i64,
) -> Result<i64, ScheduleError> {
    let mut candidate = floor_to_minute(now_ms);
    let mut remaining = CRON_SCAN_MINUTES;
    while remaining > 0 && candidate >= dtstart_ms {
        if cron_date_matches(cron, candidate) {
            if cron_time_matches(cron, candidate) {
                return Ok(candidate);
            }
            candidate -= MINUTE_MS;
            remaining -= 1;
        } else {
            let previous_day_end = start_of_local_day(candidate) - MINUTE_MS;
            remaining -= (candidate - previous_day_end) / MINUTE_MS;
            candidate = previous_day_end;
        }
    }
    Err(ScheduleError::NoOccurrence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};

    fn local_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .earliest()
            .expect("unambiguous local test time")
            .timestamp_millis()
    }

    fn local_at(ms: i64) -> DateTime<Local> {
        Local
            .timestamp_millis_opt(ms)
            .single()
            .expect("valid local timestamp")
    }

    #[test]
    fn cron_weekday_schedule_never_returns_weekend() {
        // 9:30 on weekdays: "30 9 * * 1-5"
        let dtstart = local_ms(2026, 10, 5, 0, 0); // Monday
        let mut cursor = dtstart;
        for _ in 0..7 {
            cursor = next_occurrence_after("30 9 * * 1-5", dtstart, cursor).unwrap();
            let weekday = local_at(cursor).weekday();
            assert!(
                weekday.num_days_from_monday() < 5,
                "weekend candidate: {cursor}"
            );
        }
    }

    #[test]
    fn cron_latest_returns_latest_overdue_occurrence() {
        let dtstart = local_ms(2026, 10, 1, 0, 0);
        let now = local_ms(2026, 10, 9, 12, 0);
        let latest = latest_occurrence_at_or_before("0 9 * * *", dtstart, now).unwrap();
        assert_eq!(latest, local_ms(2026, 10, 9, 9, 0));
        assert_eq!(now - latest, 3 * 60 * 60 * 1000);
    }

    #[test]
    fn custom_cron_next_and_latest_over_a_weekend() {
        let dtstart = local_ms(2026, 5, 1, 0, 0);
        let after = local_ms(2026, 5, 15, 12, 0); // Friday noon
        let next = next_occurrence_after("15 10 * * 1-5", dtstart, after).unwrap();
        assert_eq!(next, local_ms(2026, 5, 18, 10, 15)); // Monday
        let latest = latest_occurrence_at_or_before("15 10 * * 1-5", dtstart, after).unwrap();
        assert_eq!(latest, local_ms(2026, 5, 15, 10, 15));
    }

    #[test]
    fn leap_day_cron_schedule_is_found_in_both_directions() {
        let next = next_occurrence_after(
            "0 0 29 2 *",
            local_ms(2026, 5, 1, 0, 0),
            local_ms(2026, 5, 15, 12, 0),
        )
        .unwrap();
        assert_eq!(next, local_ms(2028, 2, 29, 0, 0));

        let latest = latest_occurrence_at_or_before(
            "0 0 29 2 *",
            local_ms(2020, 1, 1, 0, 0),
            local_ms(2029, 1, 1, 0, 0),
        )
        .unwrap();
        assert_eq!(latest, local_ms(2028, 2, 29, 0, 0));
    }

    #[test]
    fn hourly_rrule_respects_minute_and_off_minute_dtstart() {
        let dtstart = local_ms(2026, 10, 9, 10, 20);
        let next = next_occurrence_after("FREQ=HOURLY;BYMINUTE=15", dtstart, dtstart).unwrap();
        assert_eq!(next, local_ms(2026, 10, 9, 11, 15));

        // dtstart is 10:30 but the schedule fires at :00; the first candidate is 11:00.
        let off_minute_dtstart = local_ms(2026, 5, 13, 10, 30);
        let after = local_ms(2026, 5, 13, 9, 0);
        let next =
            next_occurrence_after("FREQ=HOURLY;BYMINUTE=0", off_minute_dtstart, after).unwrap();
        assert_eq!(next, local_ms(2026, 5, 13, 11, 0));
    }

    #[test]
    fn hourly_latest_uses_latest_overdue_occurrence() {
        let dtstart = local_ms(2026, 5, 12, 0, 0);
        let now = local_ms(2026, 5, 13, 14, 20);
        let latest =
            latest_occurrence_at_or_before("FREQ=HOURLY;BYMINUTE=0", dtstart, now).unwrap();
        assert_eq!(latest, local_ms(2026, 5, 13, 14, 0));
    }

    #[test]
    fn daily_rrule_next_and_latest() {
        let dtstart = local_ms(2026, 10, 9, 10, 0);
        let next =
            next_occurrence_after("FREQ=DAILY;BYHOUR=9;BYMINUTE=0", dtstart, dtstart).unwrap();
        assert_eq!(next, local_ms(2026, 10, 10, 9, 0));

        let latest = latest_occurrence_at_or_before(
            "FREQ=DAILY;BYHOUR=9;BYMINUTE=0",
            local_ms(2026, 10, 1, 0, 0),
            local_ms(2026, 10, 9, 12, 0),
        )
        .unwrap();
        assert_eq!(latest, local_ms(2026, 10, 9, 9, 0));
    }

    #[test]
    fn weekly_sunday_occurrence_stays_on_sunday() {
        let dtstart = local_ms(2026, 10, 9, 0, 0); // Friday
        let next =
            next_occurrence_after("FREQ=WEEKLY;BYDAY=SU;BYHOUR=8;BYMINUTE=0", dtstart, dtstart)
                .unwrap();
        let dt = local_at(next);
        assert_eq!(dt.weekday(), chrono::Weekday::Sun);
        assert_eq!((dt.hour(), dt.minute()), (8, 0));
        assert_eq!(next, local_ms(2026, 10, 11, 8, 0));
    }

    #[test]
    fn weekly_malformed_or_missing_byday_is_rejected() {
        assert!(!is_valid_schedule(
            "FREQ=WEEKLY;BYDAY=NO;BYHOUR=10;BYMINUTE=15"
        ));
        assert!(!is_valid_schedule(
            "FREQ=WEEKLY;BYDAY=MO,NO;BYHOUR=10;BYMINUTE=15"
        ));
        assert!(!is_valid_schedule("FREQ=WEEKLY;BYHOUR=9;BYMINUTE=0"));
        assert_eq!(
            next_occurrence_after(
                "FREQ=WEEKLY;BYHOUR=9;BYMINUTE=0",
                local_ms(2026, 5, 1, 0, 0),
                local_ms(2026, 5, 2, 0, 0),
            ),
            None
        );
    }

    #[test]
    fn invalid_schedules_are_rejected() {
        assert!(!is_valid_schedule("not a schedule"));
        assert!(!is_valid_schedule("61 9 * * *"));
        assert!(!is_valid_schedule("FREQ=SECONDLY;BYMINUTE=0"));
        assert!(!is_valid_schedule("0 0 * * * *"));
        assert!(!is_valid_schedule(""));
    }

    #[test]
    fn impossible_cron_is_rejected() {
        // Feb 31 never occurs.
        assert!(!is_valid_schedule("0 0 31 2 *"));
    }

    #[test]
    fn no_occurrence_in_scan_window_returns_none() {
        assert_eq!(
            next_occurrence_after(
                "0 0 31 2 *",
                local_ms(2026, 1, 1, 0, 0),
                local_ms(2026, 1, 1, 0, 0),
            ),
            None
        );
    }

    #[test]
    fn oversized_cron_text_is_rejected_after_trimming() {
        // The 2 KiB cap applies to the trimmed text (parseSchedule trims first),
        // so pure trailing whitespace is not oversized...
        let padded = format!("0 0 * * *{}", " ".repeat(2100));
        assert!(is_valid_schedule(&padded));
        // ...but more than 2048 bytes of meaningful text is.
        let oversized = format!("0 0 * * *{}x", " ".repeat(2100));
        assert!(!is_valid_schedule(&oversized));
        let pasted = "secret-cron-field ".repeat(AUTOMATION_CRON_EXPRESSION_MAX_BYTES);
        assert!(!is_valid_schedule(&pasted));
    }

    #[test]
    fn malformed_cron_separators_are_rejected() {
        assert!(!is_valid_schedule("*/15/2 9 * * *"));
        assert!(!is_valid_schedule("0 9 1--5 * *"));
    }

    #[test]
    fn dom_and_dow_both_restricted_match_either_field() {
        let dtstart = local_ms(2026, 5, 1, 0, 0);
        let after = local_ms(2026, 5, 12, 0, 0);
        let next = next_occurrence_after("0 9 1 * MON", dtstart, after).unwrap();
        assert_eq!(next, local_ms(2026, 5, 18, 9, 0)); // Monday, before June 1
        let latest =
            latest_occurrence_at_or_before("0 9 1 * MON", dtstart, local_ms(2026, 5, 20, 0, 0))
                .unwrap();
        assert_eq!(latest, local_ms(2026, 5, 18, 9, 0));
    }

    #[test]
    fn all_value_dom_cron_is_unrestricted_for_matching() {
        let next = next_occurrence_after(
            "0 9 */1 * MON",
            local_ms(2026, 5, 1, 0, 0),
            local_ms(2026, 5, 15, 12, 0),
        )
        .unwrap();
        assert_eq!(next, local_ms(2026, 5, 18, 9, 0));
        assert!(is_valid_schedule("0 9 * * 0-7"));
        assert!(is_valid_schedule("0 9 * * 1-7"));
    }

    #[test]
    fn day_of_week_names_and_seven_normalize_to_sunday() {
        let dtstart = local_ms(2026, 5, 1, 0, 0);
        let after = local_ms(2026, 5, 15, 12, 0);
        let next = next_occurrence_after("30 12 * * 7", dtstart, after).unwrap();
        let dt = local_at(next);
        assert_eq!(dt.weekday(), chrono::Weekday::Sun);
        assert_eq!((dt.hour(), dt.minute()), (12, 30));
        assert_eq!(next, local_ms(2026, 5, 17, 12, 30));

        let named = next_occurrence_after("15 10 * * MON-FRI", dtstart, after).unwrap();
        assert_eq!(named, local_ms(2026, 5, 18, 10, 15));
    }

    #[test]
    fn cron_fields_tokenize_unicode_whitespace() {
        let schedule = format!("15{}10\n*\t*\rMON-FRI", '\u{00a0}');
        let next = next_occurrence_after(
            &schedule,
            local_ms(2026, 5, 1, 0, 0),
            local_ms(2026, 5, 15, 12, 0),
        )
        .unwrap();
        assert_eq!(next, local_ms(2026, 5, 18, 10, 15));
    }

    #[test]
    fn latest_returns_none_before_dtstart() {
        let dtstart = local_ms(2026, 10, 9, 9, 0);
        let now = local_ms(2026, 10, 9, 8, 0);
        assert_eq!(
            latest_occurrence_at_or_before("0 9 * * *", dtstart, now),
            None
        );
    }
}
