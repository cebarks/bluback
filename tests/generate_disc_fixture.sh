#!/usr/bin/env bash
#
# Generate synthetic Blu-ray m2ts streams for the disc test fixture.
#
# The BDMV metadata (index.bdmv, MovieObject.bdmv, MPLS, CLPI) is committed
# to the repo. This script generates the matching m2ts stream files which are
# too large to commit. Run this before running integration tests that need
# the full disc fixture.
#
# Requirements: ffmpeg, tsMuxeR (https://github.com/justdan96/tsMuxer)
#
# The fixture represents a synthetic disc with 6 playlists:
#   00001-00004: 4 regular episodes (240s each, solid color + sine tone)
#   00005:       1 double-length episode (480s)
#   00006:       1 short special (200s)
#
# The index.bdmv has title order [3,1,2,4,5,6] to test playlist reordering.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FIXTURE_DIR="$SCRIPT_DIR/fixtures/disc"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

# Check dependencies
if ! command -v ffmpeg &>/dev/null; then
    echo "Error: ffmpeg is required" >&2
    exit 1
fi

TSMUXER=""
if command -v tsMuxeR &>/dev/null; then
    TSMUXER="tsMuxeR"
elif [ -x "/tmp/tsmuxer_bin/tsMuxeR" ]; then
    TSMUXER="/tmp/tsmuxer_bin/tsMuxeR"
else
    echo "Error: tsMuxeR is required. Install from https://github.com/justdan96/tsMuxer/releases" >&2
    echo "  or place the binary at /tmp/tsmuxer_bin/tsMuxeR" >&2
    exit 1
fi

# Check if streams already exist
if [ -f "$FIXTURE_DIR/BDMV/STREAM/00001.m2ts" ]; then
    echo "Disc fixture streams already exist. Delete BDMV/STREAM/*.m2ts to regenerate."
    exit 0
fi

echo "Generating synthetic Blu-ray streams in $FIXTURE_DIR ..."
mkdir -p "$FIXTURE_DIR/BDMV/STREAM"

# Clip definitions: name duration_secs audio_freq color
declare -a CLIPS=(
    "00001 240 440 blue"
    "00002 240 550 red"
    "00003 240 660 green"
    "00004 240 770 yellow"
    "00005 480 880 purple"
    "00006 200 990 orange"
)

for clip in "${CLIPS[@]}"; do
    read -r num dur freq color <<< "$clip"
    echo "  Generating $num (${dur}s, $color)..."

    # Generate raw elementary streams (minimal bitrate for solid color)
    ffmpeg -f lavfi -i "color=c=${color}:s=720x480:r=24000/1001:d=${dur}" \
           -c:v libx264 -preset ultrafast -profile:v high -level 4.1 \
           -pix_fmt yuv420p -g 48 -bf 2 -b:v 100k \
           "$WORK_DIR/${num}.264" -y -v error

    ffmpeg -f lavfi -i "sine=frequency=${freq}:sample_rate=48000:duration=${dur}" \
           -c:a ac3 -b:a 128k -ac 2 \
           "$WORK_DIR/${num}.ac3" -y -v error

    # Mux into Blu-ray structure with tsMuxeR
    cat > "$WORK_DIR/${num}.meta" << METAEOF
MUXOPT --blu-ray
V_MPEG4/ISO/AVC, "$WORK_DIR/${num}.264", lang=eng
A_AC3, "$WORK_DIR/${num}.ac3", lang=eng
METAEOF

    "$TSMUXER" "$WORK_DIR/${num}.meta" "$WORK_DIR/mux_${num}" > /dev/null 2>&1

    # tsMuxeR outputs as 00000.m2ts — rename to match our playlist numbering
    cp "$WORK_DIR/mux_${num}/BDMV/STREAM/00000.m2ts" "$FIXTURE_DIR/BDMV/STREAM/${num}.m2ts"
done

echo ""
echo "Generated streams:"
ls -lh "$FIXTURE_DIR/BDMV/STREAM/"
echo ""
echo "Total fixture size:"
du -sh "$FIXTURE_DIR"
echo ""
echo "Done. Run 'bluback --list-playlists -d tests/fixtures/disc --min-duration 180' to verify."
