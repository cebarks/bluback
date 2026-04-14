//! Duration parsing for history retention and CLI filters.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;

#[allow(dead_code)] // Public API — used by config and CLI modules (tasks 7+)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedDuration {
    Relative(u32),     // days
    Absolute(String),  // ISO date string "YYYY-MM-DD"
}

/// Parse a duration string into a ParsedDuration.
///
/// Accepts:
/// - "30d" → Relative(30)
/// - "1d" → Relative(1)
/// - "6months" or "6month" → Relative(180) (30 days per month)
/// - "1month" → Relative(30)
/// - "1year" or "2years" → Relative(365) / Relative(730)
/// - "2026-04-01" → Absolute("2026-04-01")
/// - Empty string or "0d" → error
#[allow(dead_code)] // Public API — used by config and CLI modules (tasks 7+)
pub fn parse_duration(s: &str) -> Result<ParsedDuration> {
    if s.is_empty() {
        return Err(anyhow!("duration cannot be empty"));
    }

    // Try absolute date first (YYYY-MM-DD)
    let date_re = Regex::new(r"^(\d{4})-(\d{2})-(\d{2})$").unwrap();
    if let Some(caps) = date_re.captures(s) {
        let year: i32 = caps[1].parse()?;
        let month: u32 = caps[2].parse()?;
        let day: u32 = caps[3].parse()?;

        // Validate the date is reasonable
        if !(1..=12).contains(&month) {
            return Err(anyhow!("invalid month: {}", month));
        }
        if !(1..=31).contains(&day) {
            return Err(anyhow!("invalid day: {}", day));
        }
        if !(1970..=9999).contains(&year) {
            return Err(anyhow!("invalid year: {}", year));
        }

        return Ok(ParsedDuration::Absolute(s.to_string()));
    }

    // Try relative durations
    let days_re = Regex::new(r"^(\d+)d$").unwrap();
    if let Some(caps) = days_re.captures(s) {
        let days: u32 = caps[1].parse()?;
        if days == 0 {
            return Err(anyhow!("duration must be greater than 0"));
        }
        return Ok(ParsedDuration::Relative(days));
    }

    let months_re = Regex::new(r"^(\d+)months?$").unwrap();
    if let Some(caps) = months_re.captures(s) {
        let months: u32 = caps[1].parse()?;
        if months == 0 {
            return Err(anyhow!("duration must be greater than 0"));
        }
        return Ok(ParsedDuration::Relative(months * 30));
    }

    let years_re = Regex::new(r"^(\d+)years?$").unwrap();
    if let Some(caps) = years_re.captures(s) {
        let years: u32 = caps[1].parse()?;
        if years == 0 {
            return Err(anyhow!("duration must be greater than 0"));
        }
        return Ok(ParsedDuration::Relative(years * 365));
    }

    Err(anyhow!(
        "invalid duration format: '{}'. Expected formats: '30d', '6months', '1year', or 'YYYY-MM-DD'",
        s
    ))
}

impl ParsedDuration {
    /// Convert to an ISO 8601 cutoff timestamp for DB queries.
    ///
    /// - Relative(30) → "2026-03-15T00:00:00" (now minus 30 days)
    /// - Absolute("2026-04-01") → "2026-04-01T00:00:00"
    #[allow(dead_code)] // Public API — used by config and CLI modules (tasks 7+)
    pub fn to_cutoff_date(&self) -> Result<String> {
        match self {
            ParsedDuration::Relative(days) => {
                let now: DateTime<Utc> = Utc::now();
                let cutoff = now - Duration::days(*days as i64);
                Ok(cutoff.format("%Y-%m-%dT%H:%M:%S").to_string())
            }
            ParsedDuration::Absolute(date) => {
                // Validate the date can be parsed
                let _parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .map_err(|e| anyhow!("invalid date '{}': {}", date, e))?;
                Ok(format!("{}T00:00:00", date))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_days() {
        assert_eq!(parse_duration("30d").unwrap(), ParsedDuration::Relative(30));
        assert_eq!(parse_duration("1d").unwrap(), ParsedDuration::Relative(1));
    }

    #[test]
    fn test_parse_months() {
        assert_eq!(parse_duration("6months").unwrap(), ParsedDuration::Relative(180));
        assert_eq!(parse_duration("6month").unwrap(), ParsedDuration::Relative(180));
        assert_eq!(parse_duration("1month").unwrap(), ParsedDuration::Relative(30));
    }

    #[test]
    fn test_parse_years() {
        assert_eq!(parse_duration("1year").unwrap(), ParsedDuration::Relative(365));
        assert_eq!(parse_duration("2years").unwrap(), ParsedDuration::Relative(730));
    }

    #[test]
    fn test_parse_absolute_date() {
        match parse_duration("2026-04-01").unwrap() {
            ParsedDuration::Absolute(d) => assert_eq!(d, "2026-04-01"),
            _ => panic!("expected absolute date"),
        }
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_duration("bogus").is_err());
        assert!(parse_duration("").is_err());
        assert!(parse_duration("0d").is_err());
    }

    #[test]
    fn test_to_cutoff_date_relative() {
        let dur = ParsedDuration::Relative(30);
        let cutoff = dur.to_cutoff_date().unwrap();
        assert!(cutoff.starts_with("20"));
        assert_eq!(cutoff.len(), 19); // "YYYY-MM-DDTHH:MM:SS"
    }

    #[test]
    fn test_to_cutoff_date_absolute() {
        let dur = ParsedDuration::Absolute("2026-04-01".to_string());
        let cutoff = dur.to_cutoff_date().unwrap();
        assert_eq!(cutoff, "2026-04-01T00:00:00");
    }
}
