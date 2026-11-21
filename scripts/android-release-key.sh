#!/bin/sh
# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.
#
# Makes dm's Android release key once (gaps row N14), and prints the four repository secrets
# .github/workflows/android.yml signs the release APKs with. Without them a release goes out
# unsigned, which Android does not install.
#
#   scripts/android-release-key.sh [keystore-path]
#
# Keep the keystore and its password safe and private: every later release must be signed
# with the same key, or phones refuse it as an update and dm has to be uninstalled first.
# Nothing is sent anywhere: the `gh secret set` lines are printed for you to run.
set -eu

store="${1:-$HOME/dm-android-release.keystore}"
alias="dm"

if [ -e "$store" ]; then
    echo "$store exists already; not made again (a second key would break updates)." >&2
    exit 1
fi
command -v keytool >/dev/null 2>&1 || {
    echo "keytool not found: install a JDK (17) and put its bin on PATH." >&2
    exit 1
}

printf "A password for the keystore (at least 6 characters): "
stty -echo
read -r password
stty echo
echo
[ "${#password}" -ge 6 ] || { echo "Too short." >&2; exit 1; }

keytool -genkeypair -v \
    -keystore "$store" -alias "$alias" \
    -keyalg RSA -keysize 4096 -validity 10000 \
    -storepass "$password" -keypass "$password" \
    -dname "CN=Easy Download Manager, O=dm"
chmod 600 "$store"

echo
echo "Made $store. Its certificate's SHA-256 (what people compare with apksigner):"
keytool -list -v -keystore "$store" -alias "$alias" -storepass "$password" | grep "SHA256:"
echo
echo "To let CI sign releases, run these yourself (gh, in this repository):"
echo "  base64 < \"$store\" | gh secret set DM_ANDROID_KEYSTORE"
echo "  gh secret set DM_ANDROID_KEYSTORE_PASSWORD   # the password above"
echo "  gh secret set DM_ANDROID_KEY_ALIAS --body $alias"
echo "  gh secret set DM_ANDROID_KEY_PASSWORD        # the same password"
echo
echo "Then push a tag android-v<versionName> (app/build.gradle.kts), raising versionCode first."
