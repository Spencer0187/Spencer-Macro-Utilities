#!/usr/bin/env bash
set -euo pipefail

APP_PATH="${1:?Usage: create_macos_dmg.sh APP_PATH DMG_PATH [STAGING_DIR]}"
DMG_PATH="${2:?Usage: create_macos_dmg.sh APP_PATH DMG_PATH [STAGING_DIR]}"
STAGE_DIR="${3:-"$(dirname "$DMG_PATH")/dmg-root"}"
RW_DMG_PATH="${DMG_PATH%.dmg}-rw.dmg"
VOLUME_NAME="Spencer Macro Utilities"
PUBLIC_APP_NAME="Spencer Macro Utilities.app"
WINDOW_LEFT=100
WINDOW_TOP=100
WINDOW_WIDTH=640
WINDOW_HEIGHT=360
APP_ICON_X=180
APPLICATIONS_ICON_X=460
ICON_Y=180
ICON_SIZE=96

write_dmg_background() {
  local output_path="$1"
  /usr/bin/python3 - \
    "$output_path" \
    "$WINDOW_WIDTH" \
    "$WINDOW_HEIGHT" \
    "$APP_ICON_X" \
    "$APPLICATIONS_ICON_X" \
    "$ICON_Y" \
    "$ICON_SIZE" <<'PY'
import struct
import sys
import zlib

path = sys.argv[1]
width, height, app_x, applications_x, icon_y, icon_size = map(int, sys.argv[2:])
if not (0 < app_x < applications_x < width and 0 < icon_y < height):
    raise SystemExit("invalid DMG layout geometry")

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

arrow_center_y = icon_y
edge_padding = max(20, icon_size // 4)
arrow_start_x = app_x + icon_size // 2 + edge_padding
arrow_tip_x = applications_x - icon_size // 2 - edge_padding
arrow_length = arrow_tip_x - arrow_start_x
if arrow_length < 64:
    raise SystemExit("DMG icon spacing is too narrow for the drag arrow")

arrow_head_length = min(44, max(28, arrow_length // 3))
arrow_body_end_x = arrow_tip_x - arrow_head_length
body_half_height = 6
head_half_height = 19
shadow_offset = 6

rect(
    arrow_start_x + shadow_offset,
    arrow_center_y - body_half_height + shadow_offset,
    arrow_body_end_x + shadow_offset,
    arrow_center_y + body_half_height + shadow_offset,
    arrow_shadow,
)
triangle(
    [
        (arrow_body_end_x + shadow_offset, arrow_center_y - head_half_height + shadow_offset),
        (arrow_body_end_x + shadow_offset, arrow_center_y + head_half_height + shadow_offset),
        (arrow_tip_x + shadow_offset, arrow_center_y + shadow_offset),
    ],
    arrow_shadow,
)
rect(
    arrow_start_x,
    arrow_center_y - body_half_height,
    arrow_body_end_x,
    arrow_center_y + body_half_height,
    arrow,
)
triangle(
    [
        (arrow_body_end_x, arrow_center_y - head_half_height),
        (arrow_body_end_x, arrow_center_y + head_half_height),
        (arrow_tip_x, arrow_center_y),
    ],
    arrow,
)

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

detach_volume() {
  local volume_path="$1"

  # GitHub-hosted macOS runners can briefly keep the mounted image busy after
  # Finder finishes applying the layout. Retry before using force; this image
  # is disposable and has already been synced.
  for attempt in 1 2 3 4 5; do
    if hdiutil detach "$volume_path" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  echo "hdiutil detach remained busy; forcing detach of $volume_path" >&2
  hdiutil detach -force "$volume_path" >/dev/null
}

test -d "$APP_PATH"
command -v hdiutil >/dev/null 2>&1
test -x /usr/bin/osascript

rm -f "$DMG_PATH" "$RW_DMG_PATH"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/.background"
ditto "$APP_PATH" "$STAGE_DIR/$PUBLIC_APP_NAME"
ln -s /Applications "$STAGE_DIR/Applications"
write_dmg_background "$STAGE_DIR/.background/background.png"

hdiutil create \
  -volname "$VOLUME_NAME" \
  -srcfolder "$STAGE_DIR" \
  -ov \
  -format UDRW \
  "$RW_DMG_PATH" >/dev/null

attached_volume_path=""
cleanup_attached_volume() {
  if [[ -n "$attached_volume_path" ]]; then
    detach_volume "$attached_volume_path" || true
  fi
}
trap cleanup_attached_volume EXIT

mount_output="$(hdiutil attach -readwrite -noverify -noautoopen "$RW_DMG_PATH")"
volume_path="$(printf '%s\n' "$mount_output" | grep -o '/Volumes/.*' | tail -n 1)"
if [[ -z "$volume_path" ]]; then
  echo "ERROR: hdiutil did not report a mounted volume path." >&2
  exit 1
fi
attached_volume_path="$volume_path"
finder_volume_name="${volume_path##*/}"
window_right=$((WINDOW_LEFT + WINDOW_WIDTH))
window_bottom=$((WINDOW_TOP + WINDOW_HEIGHT))

if ! /usr/bin/osascript >/dev/null <<APPLESCRIPT
tell application "Finder"
  tell disk "$finder_volume_name"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set bounds of container window to {$WINDOW_LEFT, $WINDOW_TOP, $window_right, $window_bottom}
    set viewOptions to icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to $ICON_SIZE
    set background picture of viewOptions to file ".background:background.png"
    set position of item "$PUBLIC_APP_NAME" of container window to {$APP_ICON_X, $ICON_Y}
    set position of item "Applications" of container window to {$APPLICATIONS_ICON_X, $ICON_Y}
    close
  end tell
end tell
APPLESCRIPT
then
  echo "ERROR: Finder failed to apply the DMG layout; refusing to publish an unlaid-out DMG." >&2
  exit 1
fi

sync
detach_volume "$volume_path"
attached_volume_path=""
trap - EXIT

hdiutil convert "$RW_DMG_PATH" -format UDZO -imagekey zlib-level=9 -o "$DMG_PATH" >/dev/null
rm -f "$RW_DMG_PATH"
