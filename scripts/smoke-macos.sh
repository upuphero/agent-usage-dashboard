#!/usr/bin/env bash
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && -n "${RUNNER_TEMP:-}" ]] || { echo 'Run installation smoke only on the ephemeral Actions runner.' >&2; exit 1; }
target=aarch64-apple-darwin
shopt -s nullglob
images=(target/$target/release/bundle/dmg/*.dmg)
[[ ${#images[@]} == 1 ]] || { echo 'Exactly one ARM64 DMG is required.' >&2; exit 1; }
mount="$RUNNER_TEMP/usage-dmg-mount"
destination="$RUNNER_TEMP/Usage 安装验证"
[[ ! -e "$mount" && ! -e "$destination" ]]
mkdir "$mount" "$destination"
hdiutil attach "${images[0]}" -nobrowse -readonly -mountpoint "$mount" -quiet
trap 'hdiutil detach "$mount" -quiet' EXIT
apps=("$mount"/*.app)
[[ ${#apps[@]} == 1 ]] || { echo 'DMG must contain exactly one app.' >&2; exit 1; }
installed="$destination/Agent Usage Dashboard.app"
ditto "${apps[0]}" "$installed"
codesign --verify --deep --strict "$installed"
node scripts/verify-bundle.mjs "--target=$target" --kind=app-bundle "--root=$installed" "--output=artifacts/$target/bundle-manifest.json"
printf 'CCUSAGE_TEST_BINARY=%s\n' "$installed/Contents/MacOS/ccusage" >> "$GITHUB_ENV"
