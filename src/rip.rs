use crate::types::RipProgress;

pub fn estimate_final_size(progress: &RipProgress, total_seconds: u32) -> Option<u64> {
    if progress.out_time_secs > 0 && total_seconds > 0 {
        Some(progress.total_size / progress.out_time_secs as u64 * total_seconds as u64)
    } else {
        None
    }
}

pub fn estimate_eta(progress: &RipProgress, total_seconds: u32) -> Option<u32> {
    if progress.speed > 0.0 && total_seconds > 0 && progress.out_time_secs < total_seconds {
        let remaining_content = total_seconds - progress.out_time_secs;
        Some((remaining_content as f64 / progress.speed) as u32)
    } else {
        None
    }
}

pub fn format_eta(seconds: u32) -> String {
    let hrs = seconds / 3600;
    let mins = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hrs > 0 {
        format!("{}:{:02}:{:02}", hrs, mins, secs)
    } else {
        format!("{}:{:02}", mins, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_final_size() {
        let progress = RipProgress {
            total_size: 1_000_000,
            out_time_secs: 100,
            ..Default::default()
        };
        assert_eq!(estimate_final_size(&progress, 2600), Some(26_000_000));
    }

    #[test]
    fn test_estimate_eta() {
        let progress = RipProgress {
            out_time_secs: 1000,
            speed: 2.0,
            ..Default::default()
        };
        assert_eq!(estimate_eta(&progress, 2600), Some(800));
    }

    #[test]
    fn test_format_eta_with_hours() {
        assert_eq!(format_eta(3661), "1:01:01");
    }

    #[test]
    fn test_format_eta_minutes_only() {
        assert_eq!(format_eta(125), "2:05");
    }
}
