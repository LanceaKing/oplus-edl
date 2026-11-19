#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

case "$(uname -s):$(uname -m)" in
    Darwin:arm64)
        target=aarch64-apple-darwin
        suffix=macos-arm64.zip
        export MACOSX_DEPLOYMENT_TARGET=14.0
        ;;
    Linux:x86_64)
        target=x86_64-unknown-linux-gnu
        suffix=linux-x86_64.tar.gz
        ;;
    *)
        echo "Run this script on an arm64 Mac or x86_64 Linux host." >&2
        exit 1
        ;;
esac

features=oplus-edl-cli/usb,oplus-edl-cli/serial,oplus-edl-cli/cloud,oplus-edl-desktop/usb,oplus-edl-desktop/serial,oplus-edl-desktop/cloud
cargo build --locked --offline --release --bins --target "$target" --target-dir "$root/target" \
    -p oplus-edl-cli -p oplus-edl-desktop --no-default-features --features "$features"

mkdir -p "$root/dist"
stage=$(mktemp -d "$root/dist/.release.XXXXXX")
trap 'rm -rf "$stage"' EXIT
trap 'exit 1' HUP INT TERM

version=$(python3 scripts/desktop-release.py stage "$target" "$features" "$root/target/$target/release" "$stage/oplus-edl")
archive="oplus-edl-$version-$suffix"
if [ "$target" = aarch64-apple-darwin ]; then
    plutil -lint "$stage/oplus-edl/Oplus EDL.app/Contents/Info.plist"
    ditto -c -k --norsrc --keepParent "$stage/oplus-edl" "$stage/$archive"
else
    tar --numeric-owner --owner=0 --group=0 -czf "$stage/$archive" -C "$stage" oplus-edl
fi

python3 scripts/desktop-release.py verify "$target" "$stage/$archive"
mv -f "$stage/$archive" "$root/dist/$archive"
printf '%s\n' "$root/dist/$archive"
