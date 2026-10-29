#!/bin/sh
# The mock video site: a plain <video> page, an HLS stream, an HLS stream
# with a segment missing, and a page with no media at all.
set -eu
site=$1
mkdir -p "$site/hls" "$site/hls-missing"
ffmpeg -loglevel error -f lavfi -i testsrc=size=320x240:rate=25 -f lavfi -i sine=frequency=440 \
  -t 6 -c:v libx264 -g 25 -keyint_min 25 -pix_fmt yuv420p -c:a aac -shortest "$site/clip.mp4"
ffmpeg -loglevel error -i "$site/clip.mp4" -c copy -f hls -hls_time 1 -hls_list_size 0 \
  -hls_segment_filename "$site/hls/seg%03d.ts" "$site/hls/index.m3u8"
cp "$site"/hls/* "$site/hls-missing/"
rm "$site/hls-missing/seg002.ts"
cat > "$site/video.html" <<H
<!doctype html><html><head><title>QA clip</title>
<meta property="og:title" content="QA clip"></head>
<body><video controls src="/clip.mp4" width="320" height="240"></video></body></html>
H
cat > "$site/hls.html" <<H
<!doctype html><html><head><title>QA stream</title></head>
<body><video controls><source src="/hls/index.m3u8" type="application/x-mpegURL"></video></body></html>
H
cat > "$site/nomedia.html" <<H
<!doctype html><html><head><title>Nothing here</title></head><body><p>No video.</p></body></html>
H
# A watch address with no extension, as video sites have: yt-dlp's alone
# (dm saves a page whose address names a document, `.html`, when yt-dlp
# cannot make a video of it).
cp "$site/video.html" "$site/watch"
ls -la "$site" "$site/hls" | head -30
