/// Formats a non-negative countdown duration in whole seconds using Zeus-style compact units.
fn format_countdown(secs: i64) -> String {
    if secs <= 0 {
        return "now".to_owned();
    }
    let days = secs / 86_400;
    let rem_after_days = secs % 86_400;
    let hours = rem_after_days / 3_600;
    let rem_after_hours = rem_after_days % 3_600;
    let minutes = rem_after_hours / 60;
    let seconds = rem_after_hours % 60;
    if days > 0 {
        return format!("{days}d{hours:02}h");
    }
    if hours > 0 {
        return format!("{hours}h{minutes:02}m");
    }
    if minutes > 0 {
        return format!("{minutes}m");
    }
    format!("{seconds}s")
}

/// Converts a duration literal or ISO timestamp into a compact countdown string.
pub(crate) fn time_left(raw: &str, now_unix_secs: i64) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }

    if let Some(duration) = parse_duration_seconds(raw) {
        return format_countdown(duration);
    }

    parse_iso_timestamp_seconds(raw)
        .map(|resets_at| format_countdown(resets_at - now_unix_secs))
        .unwrap_or_default()
}

fn parse_duration_seconds(raw: &str) -> Option<i64> {
    if raw.contains('T') {
        return None;
    }
    let mut remaining = raw.trim();
    if remaining.is_empty() {
        return None;
    }
    let mut total_seconds = 0.0;
    while !remaining.is_empty() {
        let unit_start = remaining.find(|ch: char| !ch.is_ascii_digit() && ch != '.')?;
        let amount = remaining[..unit_start].parse::<f64>().ok()?;
        let unit_tail = &remaining[unit_start..];
        let (unit, rest) = if let Some(rest) = unit_tail.strip_prefix("ms") {
            ("ms", rest)
        } else if let Some(rest) = unit_tail.strip_prefix('d') {
            ("d", rest)
        } else if let Some(rest) = unit_tail.strip_prefix('h') {
            ("h", rest)
        } else if let Some(rest) = unit_tail.strip_prefix('m') {
            ("m", rest)
        } else if let Some(rest) = unit_tail.strip_prefix('s') {
            ("s", rest)
        } else {
            return None;
        };
        total_seconds += match unit {
            "ms" => amount / 1000.0,
            "s" => amount,
            "m" => amount * 60.0,
            "h" => amount * 3600.0,
            "d" => amount * 86_400.0,
            _ => unreachable!(),
        };
        remaining = rest;
    }
    Some(round_ties_to_even(total_seconds))
}

fn parse_iso_timestamp_seconds(raw: &str) -> Option<i64> {
    let (date, time) = raw.split_once('T')?;
    let (year, month, day) = parse_date(date)?;
    let (hour, minute, second, offset_seconds) = parse_time(time)?;
    let days = days_from_civil(year, month, day)?;
    Some(
        days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second)
            - i64::from(offset_seconds),
    )
}

fn parse_date(raw: &str) -> Option<(i32, u32, u32)> {
    let mut parts = raw.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn parse_time(raw: &str) -> Option<(u32, u32, u32, i32)> {
    if let Some(stripped) = raw.strip_suffix('Z') {
        return parse_time_components(stripped).map(|(h, m, s)| (h, m, s, 0));
    }

    let plus_offset = raw.rfind('+');
    let minus_offset = raw.rfind('-');
    let offset_index = match (plus_offset, minus_offset) {
        (Some(plus), Some(minus)) => plus.max(minus),
        (Some(index), None) | (None, Some(index)) => index,
        (None, None) => return parse_time_components(raw).map(|(h, m, s)| (h, m, s, 0)),
    };
    let (time_part, offset_part) = raw.split_at(offset_index);
    let (hour, minute, second) = parse_time_components(time_part)?;
    let offset_seconds = parse_offset_seconds(offset_part)?;
    Some((hour, minute, second, offset_seconds))
}

fn parse_time_components(raw: &str) -> Option<(u32, u32, u32)> {
    let mut parts = raw.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next()?.parse::<u32>().ok()?;
    let second_part = parts.next()?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let second_digits = second_part.split('.').next()?;
    let second = second_digits.parse::<u32>().ok()?;
    if second > 59 {
        return None;
    }
    Some((hour, minute, second))
}

fn parse_offset_seconds(raw: &str) -> Option<i32> {
    let sign = if raw.starts_with('+') {
        1
    } else if raw.starts_with('-') {
        -1
    } else {
        return None;
    };
    let offset = &raw[1..];
    let (hours, minutes) = if let Some((hours, minutes)) = offset.split_once(':') {
        (hours, minutes)
    } else if offset.len() == 4 {
        (&offset[..2], &offset[2..])
    } else {
        return None;
    };
    let hours = hours.parse::<i32>().ok()?;
    let minutes = minutes.parse::<i32>().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60))
}

fn round_ties_to_even(value: f64) -> i64 {
    let floor = value.floor();
    let fraction = value - floor;
    if fraction < 0.5 {
        floor as i64
    } else if fraction > 0.5 {
        value.ceil() as i64
    } else if (floor as i64) % 2 == 0 {
        floor as i64
    } else {
        floor as i64 + 1
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let month = i32::try_from(month).ok()?;
    let day = i32::try_from(day).ok()?;
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(i64::from(era * 146097 + day_of_era - 719468))
}

#[cfg(test)]
mod tests {
    use super::{parse_iso_timestamp_seconds, time_left};

    #[test]
    fn time_left_supports_duration_minutes() {
        assert_eq!(time_left("5m", 0), "5m");
        assert_eq!(time_left("30m", 0), "30m");
    }

    #[test]
    fn time_left_supports_duration_hours() {
        assert_eq!(time_left("2h", 0), "2h00m");
        assert_eq!(time_left("1h", 0), "1h00m");
    }

    #[test]
    fn time_left_supports_duration_days() {
        assert_eq!(time_left("24h", 0), "1d00h");
        assert_eq!(time_left("111h", 0), "4d15h");
    }

    #[test]
    fn time_left_supports_compound_duration_literals() {
        assert_eq!(time_left("4h55m", 0), "4h55m");
        assert_eq!(time_left("5d06h", 0), "5d06h");
        assert_eq!(time_left("1m30s", 0), "1m");
    }

    #[test]
    fn time_left_supports_duration_seconds() {
        assert_eq!(time_left("45s", 0), "45s");
        assert_eq!(time_left("0s", 0), "now");
    }

    #[test]
    fn time_left_supports_duration_millis() {
        assert_eq!(time_left("500ms", 0), "now");
        assert_eq!(time_left("1500ms", 0), "2s");
    }

    #[test]
    fn time_left_supports_future_iso() {
        let now = parse_iso_timestamp_seconds("2026-03-29T12:00:00Z").unwrap();
        assert_eq!(time_left("2026-03-29T14:30:00Z", now), "2h30m");
    }

    #[test]
    fn time_left_supports_past_iso() {
        let now = parse_iso_timestamp_seconds("2026-03-29T12:00:00Z").unwrap();
        assert_eq!(time_left("2026-03-29T11:00:00Z", now), "now");
    }

    #[test]
    fn time_left_returns_empty_for_invalid_or_empty_values() {
        assert_eq!(time_left("", 0), "");
        assert_eq!(time_left("   ", 0), "");
        assert_eq!(time_left("not-a-time", 0), "");
    }
}
