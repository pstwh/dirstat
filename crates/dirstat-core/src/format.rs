pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["KB", "MB", "GB", "TB", "PB", "EB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

pub fn human_count(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub fn short_count(count: u64) -> String {
    if count >= 10_000_000 {
        format!("{:.0}M", count as f64 / 1e6)
    } else if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1e6)
    } else if count >= 10_000 {
        format!("{:.0}K", count as f64 / 1e3)
    } else {
        human_count(count)
    }
}

pub fn human_duration(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs_f64();
    if seconds < 1.0 {
        format!("{:.0} ms", seconds * 1000.0)
    } else if seconds < 60.0 {
        format!("{seconds:.1} s")
    } else {
        let whole = seconds as u64;
        format!("{}m {:02}s", whole / 60, whole % 60)
    }
}

pub fn human_date(seconds: u32) -> String {
    if seconds == 0 {
        return String::new();
    }
    jiff::Timestamp::from_second(seconds as i64)
        .map(|ts| {
            ts.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

pub fn now_secs() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().min(u32::MAX as u64) as u32)
        .unwrap_or(0)
}

pub fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim().to_ascii_lowercase();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: f64 = number.parse().ok()?;
    let multiplier: f64 = match unit.trim().trim_end_matches("ib").trim_end_matches('b') {
        "" => 1.0,
        "k" => 1024.0,
        "m" => 1024.0 * 1024.0,
        "g" => 1024.0 * 1024.0 * 1024.0,
        "t" => 1024.0f64.powi(4),
        "p" => 1024.0f64.powi(5),
        _ => return None,
    };
    Some((value * multiplier) as u64)
}

pub fn parse_age(text: &str) -> Option<u32> {
    let text = text.trim().to_ascii_lowercase();
    let split = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: u64 = number.parse().ok()?;
    let seconds = match unit {
        "s" => 1,
        "m" | "min" => 60,
        "h" => 3600,
        "" | "d" => 86400,
        "w" => 7 * 86400,
        "mo" => 30 * 86400,
        "y" => 365 * 86400,
        _ => return None,
    };
    Some((value * seconds).min(u32::MAX as u64) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_counts_read_well() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.00 KB");
        assert_eq!(human_size(15 * 1024 * 1024), "15.0 MB");
        assert_eq!(human_size(300 * 1024 * 1024 * 1024), "300 GB");
        assert_eq!(human_count(1234567), "1,234,567");
        assert_eq!(human_count(12), "12");
        assert_eq!(short_count(1_500_000), "1.5M");
    }

    #[test]
    fn sizes_and_ages_parse() {
        assert_eq!(parse_size("100mb"), Some(100 * 1024 * 1024));
        assert_eq!(parse_size("1.5G"), Some(1610612736));
        assert_eq!(parse_size("12"), Some(12));
        assert_eq!(parse_size("x"), None);
        assert_eq!(parse_age("2d"), Some(172800));
        assert_eq!(parse_age("1y"), Some(31536000));
    }
}
