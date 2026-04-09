use crate::types::ChapterMark;
use std::collections::HashMap;
use std::path::Path;

/// Data extracted from a single MPLS playlist file.
pub struct MplsInfo {
    pub chapters: Vec<ChapterMark>,
    /// Sum of on-disc `.m2ts` clip file sizes in bytes.
    /// Zero if the STREAM directory is missing or all clips fail to stat.
    pub clip_size: u64,
}

/// Parse an MPLS playlist file and extract chapter marks and clip file sizes.
///
/// Reads `BDMV/PLAYLIST/{playlist_num}.mpls` from the mounted disc.
/// For each PlayItem, stats `BDMV/STREAM/{clip_name}.m2ts` and sums the sizes.
/// Only the primary clip per PlayItem is used (angle 0), matching bluback's remux behavior.
///
/// Returns `None` if the MPLS file can't be read, can't be parsed, or has no entry-point marks.
pub fn parse_mpls_info(mount_point: &Path, playlist_num: &str) -> Option<MplsInfo> {
    let mpls_path = mount_point
        .join("BDMV")
        .join("PLAYLIST")
        .join(format!("{}.mpls", playlist_num));

    let file = std::fs::File::open(&mpls_path).ok()?;
    let mpls_data = mpls::Mpls::from(file).ok()?;

    // Extract chapter marks from entry-point marks
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

    // Sum clip file sizes from play items (primary clip only, not multi-angle)
    let stream_dir = mount_point.join("BDMV").join("STREAM");
    let clip_size: u64 = mpls_data
        .play_list
        .play_items
        .iter()
        .map(|item| {
            let clip_path = stream_dir.join(format!("{}.m2ts", item.clip.file_name));
            match std::fs::metadata(&clip_path) {
                Ok(meta) => meta.len(),
                Err(_) => {
                    log::debug!(
                        "Clip file not found: {} (playlist {})",
                        clip_path.display(),
                        playlist_num
                    );
                    0
                }
            }
        })
        .sum();

    Some(MplsInfo {
        chapters,
        clip_size,
    })
}

/// Collect MPLS info (chapters + clip sizes) for multiple playlists in one pass.
///
/// Returns a map of playlist number → `MplsInfo`.
/// Playlists whose MPLS files can't be read are omitted from the result.
pub fn collect_mpls_info(mount_point: &Path, playlist_nums: &[&str]) -> HashMap<String, MplsInfo> {
    let mut info = HashMap::new();
    for &num in playlist_nums {
        if let Some(mpls_info) = parse_mpls_info(mount_point, num) {
            info.insert(num.to_string(), mpls_info);
        }
    }
    info
}

/// Extract chapter marks from an MPLS playlist file on a mounted Blu-ray disc.
///
/// Thin wrapper around `parse_mpls_info` — returns only the chapters.
/// Used by `workflow::prepare_remux_options` during ripping.
pub fn extract_chapters(mount_point: &Path, playlist_num: &str) -> Option<Vec<ChapterMark>> {
    parse_mpls_info(mount_point, playlist_num).map(|info| info.chapters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mpls_info_missing_path() {
        let result = parse_mpls_info(std::path::Path::new("/nonexistent/path"), "00001");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_mpls_info_missing_playlist() {
        let dir = std::env::temp_dir().join("bluback_test_mpls_info");
        let playlist_dir = dir.join("BDMV").join("PLAYLIST");
        std::fs::create_dir_all(&playlist_dir).unwrap();
        let result = parse_mpls_info(&dir, "99999");
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

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
