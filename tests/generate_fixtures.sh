#!/usr/bin/env bash
set -euo pipefail

FIXTURES_DIR="$(cd "$(dirname "$0")" && pwd)/fixtures"

echo "Generating test fixtures in $FIXTURES_DIR ..."

mkdir -p "$FIXTURES_DIR/media"

if [ ! -f "$FIXTURES_DIR/media/test_video.mkv" ]; then
    ffmpeg -y -f lavfi -i testsrc=duration=2:size=320x240:rate=25 \
           -f lavfi -i sine=frequency=440:duration=2 \
           -c:v libx264 -preset ultrafast -crf 51 \
           -c:a aac -b:a 32k \
           -t 2 "$FIXTURES_DIR/media/test_video.mkv" 2>/dev/null
    echo "  Created test_video.mkv"
fi

if [ ! -f "$FIXTURES_DIR/media/test_multi_audio.mkv" ]; then
    ffmpeg -y -f lavfi -i testsrc=duration=2:size=320x240:rate=25 \
           -f lavfi -i "sine=frequency=440:duration=2" \
           -f lavfi -i "sine=frequency=440:duration=2" \
           -c:v libx264 -preset ultrafast -crf 51 \
           -c:a aac -b:a 32k \
           -map 0:v -map 1:a -map 2:a \
           -metadata:s:a:0 title="Stereo" \
           -ac:a:0 2 \
           -metadata:s:a:1 title="Surround" \
           -ac:a:1 6 \
           -t 2 "$FIXTURES_DIR/media/test_multi_audio.mkv" 2>/dev/null
    echo "  Created test_multi_audio.mkv"
fi

echo "Done."
