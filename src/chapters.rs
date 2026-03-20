use crate::types::ChapterMark;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Format a chapter timestamp as HH:MM:SS.mmm for OGM chapter format.
fn format_chapter_time(secs: f64) -> String {
    let total_ms = (secs * 1000.0).round() as u64;
    let hrs = total_ms / 3_600_000;
    let mins = (total_ms % 3_600_000) / 60_000;
    let s = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;
    format!("{:02}:{:02}:{:02}.{:03}", hrs, mins, s, ms)
}

/// Generate OGM-format chapter text from a list of chapter marks.
pub fn chapters_to_ogm(chapters: &[ChapterMark]) -> String {
    let mut out = String::new();
    for ch in chapters {
        out.push_str(&format!(
            "CHAPTER{:02}={}\nCHAPTER{:02}NAME=Chapter {}\n",
            ch.index,
            format_chapter_time(ch.start_secs),
            ch.index,
            ch.index,
        ));
    }
    out
}

/// Count chapters for each playlist by reading MPLS files from a mounted disc.
/// Returns a map of playlist number → chapter count.
pub fn count_chapters_for_playlists(
    mount_point: &Path,
    playlist_nums: &[&str],
) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for &num in playlist_nums {
        if let Some(chapters) = extract_chapters(mount_point, num) {
            counts.insert(num.to_string(), chapters.len());
        }
    }
    counts
}

/// Extract chapter marks from an MPLS playlist file on a mounted Blu-ray disc.
///
/// `mount_point` is the filesystem root of the mounted disc.
/// `playlist_num` is the zero-padded playlist number (e.g., "00001").
///
/// Returns chapter marks with timestamps relative to the start of the playlist,
/// or None if the MPLS file can't be read or has no entry-point marks.
pub fn extract_chapters(mount_point: &Path, playlist_num: &str) -> Option<Vec<ChapterMark>> {
    let mpls_path = Path::new(mount_point)
        .join("BDMV")
        .join("PLAYLIST")
        .join(format!("{}.mpls", playlist_num));

    let file = std::fs::File::open(&mpls_path).ok()?;
    let mpls_data = mpls::Mpls::from(file).ok()?;

    let entry_marks: Vec<_> = mpls_data
        .marks
        .iter()
        .filter(|m| matches!(m.mark_type, mpls::types::MarkType::EntryPoint))
        .collect();

    if entry_marks.is_empty() {
        return None;
    }

    let base_secs = entry_marks[0].time_stamp.seconds();

    let chapters: Vec<ChapterMark> = entry_marks
        .iter()
        .enumerate()
        .map(|(i, mark)| ChapterMark {
            index: (i + 1) as u32,
            start_secs: mark.time_stamp.seconds() - base_secs,
        })
        .collect();

    Some(chapters)
}

/// Write chapters to an MKV file using mkvpropedit.
///
/// Writes a temporary OGM chapter file next to the output, runs mkvpropedit,
/// and cleans up the temp file. Returns Ok(true) if chapters were applied,
/// Ok(false) if there were no chapters, or Err on failure.
pub fn apply_chapters(outfile: &Path, chapters: &[ChapterMark]) -> anyhow::Result<bool> {
    if chapters.is_empty() {
        return Ok(false);
    }

    let ogm = chapters_to_ogm(chapters);

    let chapter_file = outfile.with_extension("chapters.txt");
    {
        let mut f = std::fs::File::create(&chapter_file)?;
        f.write_all(ogm.as_bytes())?;
    }

    let output = Command::new("mkvpropedit")
        .arg(outfile)
        .args(["--chapters", &chapter_file.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    let _ = std::fs::remove_file(&chapter_file);

    match output {
        Ok(o) if o.status.success() => Ok(true),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            anyhow::bail!("mkvpropedit failed: {}", stderr.trim())
        }
        Err(e) => anyhow::bail!("failed to run mkvpropedit: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_chapter_time_zero() {
        assert_eq!(format_chapter_time(0.0), "00:00:00.000");
    }

    #[test]
    fn test_format_chapter_time_minutes() {
        assert_eq!(format_chapter_time(202.119), "00:03:22.119");
    }

    #[test]
    fn test_format_chapter_time_hours() {
        assert_eq!(format_chapter_time(3661.5), "01:01:01.500");
    }

    #[test]
    fn test_chapters_to_ogm_single() {
        let chapters = vec![ChapterMark {
            index: 1,
            start_secs: 0.0,
        }];
        let ogm = chapters_to_ogm(&chapters);
        assert_eq!(ogm, "CHAPTER01=00:00:00.000\nCHAPTER01NAME=Chapter 1\n");
    }

    #[test]
    fn test_chapters_to_ogm_multiple() {
        let chapters = vec![
            ChapterMark {
                index: 1,
                start_secs: 0.0,
            },
            ChapterMark {
                index: 2,
                start_secs: 202.119,
            },
            ChapterMark {
                index: 3,
                start_secs: 772.689,
            },
        ];
        let ogm = chapters_to_ogm(&chapters);
        assert!(ogm.contains("CHAPTER01=00:00:00.000"));
        assert!(ogm.contains("CHAPTER02=00:03:22.119"));
        assert!(ogm.contains("CHAPTER03=00:12:52.689"));
        assert!(ogm.contains("CHAPTER03NAME=Chapter 3"));
    }

    #[test]
    fn test_chapters_to_ogm_empty() {
        assert_eq!(chapters_to_ogm(&[]), "");
    }
}
