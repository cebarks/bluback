use crate::types::ChapterMark;
use std::collections::HashMap;
use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_chapters_missing_path() {
        let result = extract_chapters(std::path::Path::new("/nonexistent/path"), "00001");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_chapters_missing_playlist() {
        let dir = std::env::temp_dir().join("bluback_test_chapters");
        let playlist_dir = dir.join("BDMV").join("PLAYLIST");
        std::fs::create_dir_all(&playlist_dir).unwrap();
        let result = extract_chapters(&dir, "99999");
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
