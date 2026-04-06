use std::path::Path;

/// Parse index.bdmv and return MPLS playlist numbers in title order.
///
/// Reads the disc's `BDMV/index.bdmv` file and extracts the title table.
/// Each HDMV title entry references an MPLS playlist by number.
/// Returns playlist numbers (zero-padded to 5 digits) in the disc author's
/// intended title order, or `None` if the file can't be read/parsed.
#[allow(dead_code)]
pub fn parse_title_order(mount_point: &Path) -> Option<Vec<String>> {
    let index_path = mount_point.join("BDMV").join("index.bdmv");
    let data = std::fs::read(&index_path).ok()?;

    // Validate header: need at least 40 bytes, magic = "INDX"
    if data.len() < 40 || &data[0..4] != b"INDX" {
        log::debug!("index.bdmv: invalid header or too short");
        return None;
    }

    // Read indexes_start offset (u32 BE at offset 8)
    let indexes_start = u32::from_be_bytes(data[8..12].try_into().ok()?) as usize;
    if indexes_start >= data.len() {
        log::debug!(
            "index.bdmv: indexes_start ({}) beyond file length",
            indexes_start
        );
        return None;
    }

    // Skip AppInfoBDMV section: 4-byte length + data
    let pos = indexes_start;
    if pos + 4 > data.len() {
        return None;
    }
    let app_info_len = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
    let pos = pos + 4 + app_info_len;

    // Indexes section: 4-byte length
    if pos + 4 > data.len() {
        return None;
    }
    let _indexes_len = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
    let pos = pos + 4;

    // Skip First Playback (12 bytes) + Top Menu (12 bytes)
    let pos = pos + 24;
    if pos + 2 > data.len() {
        return None;
    }

    // Read number of titles (u16 BE)
    let num_titles = u16::from_be_bytes(data[pos..pos + 2].try_into().ok()?) as usize;
    let pos = pos + 2;

    // Parse title entries (12 bytes each)
    let mut playlist_nums = Vec::new();
    for i in 0..num_titles {
        let entry_start = pos + i * 12;
        if entry_start + 12 > data.len() {
            log::debug!("index.bdmv: truncated at title entry {}", i);
            break;
        }

        // Byte 0-3: object_type in bits 31-30
        let word0 = u32::from_be_bytes(data[entry_start..entry_start + 4].try_into().ok()?);
        let object_type = (word0 >> 30) & 0x03;

        if object_type == 1 {
            // HDMV: id_ref at bytes 6-7 (u16 BE)
            let id_ref =
                u16::from_be_bytes(data[entry_start + 6..entry_start + 8].try_into().ok()?);
            playlist_nums.push(format!("{:05}", id_ref));
        }
        // BD-J (object_type == 2) and others: skip
    }

    if playlist_nums.is_empty() {
        None
    } else {
        Some(playlist_nums)
    }
}

/// Reorder a playlist vec using title order from index.bdmv.
///
/// Playlists referenced in `title_order` are moved to the front in that order.
/// Remaining playlists are appended in MPLS number order.
/// If `title_order` is `None`, falls back to sorting all playlists by MPLS number.
#[allow(dead_code)]
pub fn reorder_playlists(playlists: &mut [crate::types::Playlist], title_order: Option<&[String]>) {
    match title_order {
        Some(order) => {
            // Build position map from title order, first occurrence wins
            let mut pos_map: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for (i, num) in order.iter().enumerate() {
                pos_map.entry(num.as_str()).or_insert(i);
            }

            playlists.sort_by(|a, b| {
                match (pos_map.get(a.num.as_str()), pos_map.get(b.num.as_str())) {
                    (Some(pa), Some(pb)) => pa.cmp(pb),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a.num.cmp(&b.num),
                }
            });
        }
        None => {
            playlists.sort_by(|a, b| a.num.cmp(&b.num));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Playlist;

    fn make_pl(num: &str, secs: u32) -> Playlist {
        Playlist {
            num: num.into(),
            duration: String::new(),
            seconds: secs,
            video_streams: 0,
            audio_streams: 0,
            subtitle_streams: 0,
        }
    }

    // --- reorder_playlists tests ---

    #[test]
    fn test_reorder_with_title_order() {
        let mut pls = vec![
            make_pl("00800", 2640),
            make_pl("00801", 2640),
            make_pl("00802", 2640),
        ];
        let order = vec!["00802".into(), "00800".into(), "00801".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00802");
        assert_eq!(pls[1].num, "00800");
        assert_eq!(pls[2].num, "00801");
    }

    #[test]
    fn test_reorder_unindexed_appended_sorted() {
        let mut pls = vec![
            make_pl("00900", 2640),
            make_pl("00800", 2640),
            make_pl("00801", 2640),
        ];
        let order = vec!["00801".into(), "00800".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00801");
        assert_eq!(pls[1].num, "00800");
        assert_eq!(pls[2].num, "00900");
    }

    #[test]
    fn test_reorder_fallback_sorts_by_num() {
        let mut pls = vec![
            make_pl("00802", 2640),
            make_pl("00800", 2640),
            make_pl("00801", 2640),
        ];
        reorder_playlists(&mut pls, None);
        assert_eq!(pls[0].num, "00800");
        assert_eq!(pls[1].num, "00801");
        assert_eq!(pls[2].num, "00802");
    }

    #[test]
    fn test_reorder_deduplicates_title_entries() {
        let mut pls = vec![make_pl("00001", 2640), make_pl("00002", 2640)];
        let order = vec!["00002".into(), "00001".into(), "00002".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00002");
        assert_eq!(pls[1].num, "00001");
    }

    #[test]
    fn test_reorder_title_order_references_missing_playlist() {
        let mut pls = vec![make_pl("00001", 2640), make_pl("00002", 2640)];
        let order = vec!["00099".into(), "00002".into(), "00001".into()];
        reorder_playlists(&mut pls, Some(&order));
        assert_eq!(pls[0].num, "00002");
        assert_eq!(pls[1].num, "00001");
    }

    // --- parse_title_order tests (synthetic binary) ---

    fn build_index_bdmv(titles: &[(u8, u16)]) -> Vec<u8> {
        let mut buf = Vec::new();

        // Header (40 bytes)
        buf.extend_from_slice(b"INDX");
        buf.extend_from_slice(b"0200");
        let indexes_start: u32 = 40;
        buf.extend_from_slice(&indexes_start.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.extend_from_slice(&[0u8; 24]);

        // AppInfoBDMV section: length + minimal data
        let app_info_len: u32 = 34;
        buf.extend_from_slice(&app_info_len.to_be_bytes());
        buf.extend_from_slice(&vec![0u8; app_info_len as usize]);

        // Indexes section
        let first_play_top_menu = 12 + 12;
        let titles_data = 2 + titles.len() * 12;
        let indexes_len = first_play_top_menu + titles_data;
        buf.extend_from_slice(&(indexes_len as u32).to_be_bytes());

        // First Playback (12 bytes, all zeros)
        buf.extend_from_slice(&[0u8; 12]);
        // Top Menu (12 bytes, all zeros)
        buf.extend_from_slice(&[0u8; 12]);

        // Number of titles
        buf.extend_from_slice(&(titles.len() as u16).to_be_bytes());

        // Title entries (12 bytes each)
        for &(obj_type, id_ref) in titles {
            let word0: u32 = (obj_type as u32) << 30;
            buf.extend_from_slice(&word0.to_be_bytes());
            buf.extend_from_slice(&[0u8; 2]);
            buf.extend_from_slice(&id_ref.to_be_bytes());
            buf.extend_from_slice(&[0u8; 4]);
        }

        buf
    }

    #[test]
    fn test_parse_synthetic_index() {
        let dir = std::env::temp_dir().join("bluback_test_index");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();

        let data = build_index_bdmv(&[(1, 800), (1, 801), (1, 802)]);
        std::fs::write(bdmv_dir.join("index.bdmv"), &data).unwrap();

        let result = parse_title_order(&dir).unwrap();
        assert_eq!(result, vec!["00800", "00801", "00802"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_mixed_hdmv_bdj() {
        let dir = std::env::temp_dir().join("bluback_test_index_mixed");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();

        let data = build_index_bdmv(&[(1, 42), (2, 0), (1, 43)]);
        std::fs::write(bdmv_dir.join("index.bdmv"), &data).unwrap();

        let result = parse_title_order(&dir).unwrap();
        assert_eq!(result, vec!["00042", "00043"]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("bluback_test_index_missing");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(parse_title_order(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_bad_magic_returns_none() {
        let dir = std::env::temp_dir().join("bluback_test_index_bad");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();
        std::fs::write(bdmv_dir.join("index.bdmv"), b"NOT_INDX_FILE").unwrap();
        assert!(parse_title_order(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_truncated_returns_none() {
        let dir = std::env::temp_dir().join("bluback_test_index_trunc");
        let bdmv_dir = dir.join("BDMV");
        std::fs::create_dir_all(&bdmv_dir).unwrap();
        std::fs::write(bdmv_dir.join("index.bdmv"), b"INDX0200").unwrap();
        assert!(parse_title_order(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
