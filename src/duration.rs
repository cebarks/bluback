//! Duration parsing for history retention and CLI filters.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, Duration, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedDuration {
    Relative(u32),    // days
    Absolute(String), // ISO date string "YYYY-MM-DD"
}

// ponytail: strip_suffix instead of 4 per-call Regex::new()
pub fn parse_duration(s: &str) -> Result<ParsedDuration> {
    if s.is_empty() {
        return Err(anyhow!("duration cannot be empty"));
    }

    let err = || {
        anyhow!(
            "invalid duration format: '{}'. Expected formats: '30d', '6months', '1year', or 'YYYY-MM-DD'",
            s
        )
    };

    if let Some(n) = s.strip_suffix('d') {
        let days: u32 = n.parse().map_err(|_| err())?;
        return if days == 0 {
            Err(anyhow!("duration must be greater than 0"))
        } else {
            Ok(ParsedDuration::Relative(days))
        };
    }

    if let Some(n) = s.strip_suffix("months").or_else(|| s.strip_suffix("month")) {
        let months: u32 = n.parse().map_err(|_| err())?;
        return if months == 0 {
            Err(anyhow!("duration must be greater than 0"))
        } else {
            Ok(ParsedDuration::Relative(months * 30))
        };
    }

    if let Some(n) = s.strip_suffix("years").or_else(|| s.strip_suffix("year")) {
        let years: u32 = n.parse().map_err(|_| err())?;
        return if years == 0 {
            Err(anyhow!("duration must be greater than 0"))
        } else {
            Ok(ParsedDuration::Relative(years * 365))
        };
    }

    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if !(1970..=9999).contains(&date.year()) {
            return Err(anyhow!("invalid year: {}", date.year()));
        }
        return Ok(ParsedDuration::Absolute(s.to_string()));
    }

    Err(err())
}

impl ParsedDuration {
    /// Convert to an ISO 8601 cutoff timestamp for DB queries.
    ///
    /// - Relative(30) → "2026-03-15T00:00:00" (now minus 30 days)
    /// - Absolute("2026-04-01") → "2026-04-01T00:00:00"
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
        assert_eq!(
            parse_duration("6months").unwrap(),
            ParsedDuration::Relative(180)
        );
        assert_eq!(
            parse_duration("6month").unwrap(),
            ParsedDuration::Relative(180)
        );
        assert_eq!(
            parse_duration("1month").unwrap(),
            ParsedDuration::Relative(30)
        );
    }

    #[test]
    fn test_parse_years() {
        assert_eq!(
            parse_duration("1year").unwrap(),
            ParsedDuration::Relative(365)
        );
        assert_eq!(
            parse_duration("2years").unwrap(),
            ParsedDuration::Relative(730)
        );
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
