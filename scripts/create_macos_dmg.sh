#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?Usage: create_macos_dmg.sh APP_PATH DMG_PATH [STAGING_DIR]}"
DMG_PATH="${2:?Usage: create_macos_dmg.sh APP_PATH DMG_PATH [STAGING_DIR]}"
STAGE_DIR="${3:-"$(dirname "$DMG_PATH")/dmg-root"}"
RW_DMG_PATH="${DMG_PATH%.dmg}-rw.dmg"
VOLUME_NAME="Spencer Macro Utilities"

write_dmg_background() {
  local output_path="$1"
  /usr/bin/python3 - "$output_path" <<'PY'
import struct
import sys
import zlib

path = sys.argv[1]
width, height = 640, 360
bg = (35, 39, 42, 255)
arrow = (0, 156, 190, 255)
arrow_shadow = (12, 18, 22, 160)
pixels = [bg] * (width * height)

def blend_pixel(x, y, rgba):
    if not (0 <= x < width and 0 <= y < height):
        return
    r, g, b, a = rgba
    br, bgc, bb, _ = pixels[y * width + x]
    alpha = a / 255.0
    pixels[y * width + x] = (
        int(r * alpha + br * (1.0 - alpha)),
        int(g * alpha + bgc * (1.0 - alpha)),
        int(b * alpha + bb * (1.0 - alpha)),
        255,
    )

def rect(x0, y0, x1, y1, rgba):
    for y in range(y0, y1):
        for x in range(x0, x1):
            blend_pixel(x, y, rgba)

def triangle(points, rgba):
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    min_x, max_x = max(min(xs), 0), min(max(xs), width - 1)
    min_y, max_y = max(min(ys), 0), min(max(ys), height - 1)
    (x1, y1), (x2, y2), (x3, y3) = points
    denom = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
    if denom == 0:
        return
    for y in range(min_y, max_y + 1):
        for x in range(min_x, max_x + 1):
            a = ((y2 - y3) * (x - x3) + (x3 - x2) * (y - y3)) / denom
            b = ((y3 - y1) * (x - x3) + (x1 - x3) * (y - y3)) / denom
            c = 1.0 - a - b
            if a >= 0 and b >= 0 and c >= 0:
                blend_pixel(x, y, rgba)

rect(246, 171, 397, 183, arrow_shadow)
triangle([(396, 158), (396, 196), (440, 177)], arrow_shadow)
rect(240, 164, 391, 176, arrow)
triangle([(390, 151), (390, 189), (434, 170)], arrow)

raw = bytearray()
for y in range(height):
    raw.append(0)
    for x in range(width):
        raw.extend(pixels[y * width + x])

def chunk(kind, data):
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    )

png = (
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    + chunk(b"IEND", b"")
)
with open(path, "wb") as handle:
    handle.write(png)
PY
}

test -d "$APP_PATH"
command -v hdiutil >/dev/null 2>&1

rm -f "$DMG_PATH" "$RW_DMG_PATH"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/.background"
ditto "$APP_PATH" "$STAGE_DIR/suspend.app"
ln -s /Applications "$STAGE_DIR/Applications"
write_dmg_background "$STAGE_DIR/.background/background.png"

hdiutil create \
  -volname "$VOLUME_NAME" \
  -srcfolder "$STAGE_DIR" \
  -ov \
  -format UDRW \
  "$RW_DMG_PATH" >/dev/null

mount_output="$(hdiutil attach -readwrite -noverify -noautoopen "$RW_DMG_PATH")"
volume_path="$(printf '%s\n' "$mount_output" | grep -o '/Volumes/.*' | tail -n 1)"
if [[ -n "$volume_path" ]]; then
  /usr/bin/osascript >/dev/null <<'APPLESCRIPT' || true
tell application "Finder"
  tell disk "Spencer Macro Utilities"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set bounds of container window to {100, 100, 740, 460}
    set viewOptions to icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 96
    set background picture of viewOptions to file ".background:background.png"
    set position of item "suspend.app" of container window to {180, 180}
    set position of item "Applications" of container window to {460, 180}
    close
  end tell
end tell
APPLESCRIPT
  sync
  hdiutil detach "$volume_path" >/dev/null
fi

hdiutil convert "$RW_DMG_PATH" -format UDZO -imagekey zlib-level=9 -o "$DMG_PATH" >/dev/null
rm -f "$RW_DMG_PATH"
