//! Integration test: verify MKV metadata tags are written correctly.
//!
//! Remuxes a test fixture with metadata tags, then reads them back
//! via ffprobe to confirm they appear in the output file.

use std::collections::HashMap;
use std::process::Command;

fn has_ffprobe() -> bool {
    Command::new("ffprobe").arg("-version").output().is_ok()
}

/// Remux a test fixture with metadata tags using the FFmpeg API directly.
/// Returns the path to the output file.
fn remux_with_metadata(input: &str, output: &str, tags: &HashMap<String, String>) {
    ffmpeg_the_third::init().unwrap();

    let mut ictx = ffmpeg_the_third::format::input(input).expect("open input");
    let mut octx = ffmpeg_the_third::format::output(output).expect("create output");

    // Map all streams
    let mut stream_map = vec![];
    for (i, in_stream) in ictx.streams().enumerate() {
        let codec_id = in_stream.parameters().id();
        let mut out_stream = octx.add_stream(codec_id).expect("add stream");
        out_stream.set_parameters(in_stream.parameters());
        out_stream.set_time_base(in_stream.time_base());
        unsafe {
            let st_ptr = out_stream.as_mut_ptr();
            (*(*st_ptr).codecpar).codec_tag = 0;
        }
        stream_map.push(i);
    }

    // Inject metadata
    let mut md = octx.metadata_mut();
    for (k, v) in tags {
        md.set(k, v);
    }

    // Write header and packets
    octx.write_header().expect("write header");

    let mut packet = ffmpeg_the_third::Packet::empty();
    while packet.read(&mut ictx).is_ok() {
        let in_idx = packet.stream();
        if in_idx < stream_map.len() {
            packet.set_stream(in_idx);
            packet.write_interleaved(&mut octx).ok();
        }
    }
    octx.write_trailer().expect("write trailer");
}

/// Read metadata tags from an MKV file using ffprobe.
fn read_metadata(path: &str) -> HashMap<String, String> {
    let output = Command::new("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_format", path])
        .output()
        .expect("ffprobe must be installed");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse ffprobe JSON");

    let mut tags = HashMap::new();
    if let Some(format_tags) = json["format"]["tags"].as_object() {
        for (k, v) in format_tags {
            if let Some(s) = v.as_str() {
                tags.insert(k.to_uppercase(), s.to_string());
            }
        }
    }
    tags
}

#[test]
fn test_auto_metadata_tags_written() {
    if !has_ffprobe() {
        eprintln!("skipping: ffprobe not found");
        return;
    }
    let input = "tests/fixtures/media/test_video.mkv";
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("auto_metadata.mkv");
    let output_str = output.to_str().unwrap();

    // Simulate auto-generated TV metadata
    let mut tags = HashMap::new();
    tags.insert("TITLE".into(), "The Rains of Castamere".into());
    tags.insert("SHOW".into(), "Game of Thrones".into());
    tags.insert("SEASON_NUMBER".into(), "3".into());
    tags.insert("EPISODE_SORT".into(), "9".into());
    tags.insert("DATE_RELEASED".into(), "2013-06-02".into());
    tags.insert("REMUXED_WITH".into(), "bluback v0.9.2".into());

    remux_with_metadata(input, output_str, &tags);

    let read_tags = read_metadata(output_str);

    assert_eq!(read_tags["TITLE"], "The Rains of Castamere");
    assert_eq!(read_tags["SHOW"], "Game of Thrones");
    assert_eq!(read_tags["SEASON_NUMBER"], "3");
    assert_eq!(read_tags["EPISODE_SORT"], "9");
    assert_eq!(read_tags["DATE_RELEASED"], "2013-06-02");
    assert_eq!(read_tags["REMUXED_WITH"], "bluback v0.9.2");
}

#[test]
fn test_custom_tags_written() {
    if !has_ffprobe() {
        eprintln!("skipping: ffprobe not found");
        return;
    }
    let input = "tests/fixtures/media/test_video.mkv";
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("custom_metadata.mkv");
    let output_str = output.to_str().unwrap();

    // Mix of auto + custom tags (simulating custom override)
    let mut tags = HashMap::new();
    tags.insert("TITLE".into(), "Custom Override Title".into());
    tags.insert("STUDIO".into(), "HBO".into());
    tags.insert("COLLECTION".into(), "My Blu-rays".into());
    tags.insert("REMUXED_WITH".into(), "bluback v0.9.2".into());

    remux_with_metadata(input, output_str, &tags);

    let read_tags = read_metadata(output_str);

    assert_eq!(read_tags["TITLE"], "Custom Override Title");
    assert_eq!(read_tags["STUDIO"], "HBO");
    assert_eq!(read_tags["COLLECTION"], "My Blu-rays");
    assert_eq!(read_tags["REMUXED_WITH"], "bluback v0.9.2");
}
