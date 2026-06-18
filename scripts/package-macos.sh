#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -d /opt/homebrew/opt/rustup/bin ]]; then
  export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
fi

APP_NAME="Jahan Nama"
APP_SLUG="jahan-nama"
BINARY_NAME="jahan-nama"
VERSION="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"
ARCH="$(uname -m)"
DIST_DIR="$ROOT_DIR/dist"
WORK_DIR="$DIST_DIR/macos"
APP_DIR="$WORK_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
ICONSET_DIR="$WORK_DIR/AppIcon.iconset"
STAGING_DIR="$WORK_DIR/dmg"
DMG_PATH="$DIST_DIR/$APP_SLUG-$VERSION-macos-$ARCH.dmg"

cargo build --release

rm -rf "$WORK_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$ICONSET_DIR" "$STAGING_DIR"

cp "$ROOT_DIR/target/release/$BINARY_NAME" "$MACOS_DIR/$BINARY_NAME"
chmod 755 "$MACOS_DIR/$BINARY_NAME"
cp "$ROOT_DIR/.env.example" "$RESOURCES_DIR/.env.example"
cp "$ROOT_DIR/README.md" "$RESOURCES_DIR/README.md"

for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ROOT_DIR/icon.png" \
    --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null
  double_size=$((size * 2))
  sips -z "$double_size" "$double_size" "$ROOT_DIR/icon.png" \
    --out "$ICONSET_DIR/icon_${size}x${size}@2x.png" >/dev/null
done

if ! iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/AppIcon.icns" >/dev/null 2>&1; then
  python3 - "$ICONSET_DIR" "$RESOURCES_DIR/AppIcon.icns" <<'PY'
import pathlib
import struct
import sys

iconset = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[2])
chunks = [
    ("icp4", "icon_16x16.png"),
    ("icp5", "icon_32x32.png"),
    ("icp6", "icon_32x32@2x.png"),
    ("ic07", "icon_128x128.png"),
    ("ic08", "icon_256x256.png"),
    ("ic09", "icon_512x512.png"),
    ("ic10", "icon_512x512@2x.png"),
]

body = bytearray()
for chunk_type, filename in chunks:
    data = (iconset / filename).read_bytes()
    body.extend(chunk_type.encode("ascii"))
    body.extend(struct.pack(">I", len(data) + 8))
    body.extend(data)

output.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)
PY
fi

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>
  <string>$BINARY_NAME</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundleIdentifier</key>
  <string>app.jahan-nama.client</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>10.15</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

cp -R "$APP_DIR" "$STAGING_DIR/$APP_NAME.app"
ln -s /Applications "$STAGING_DIR/Applications"
rm -f "$DMG_PATH"
hdiutil create \
  -volname "$APP_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

echo "$DMG_PATH"
