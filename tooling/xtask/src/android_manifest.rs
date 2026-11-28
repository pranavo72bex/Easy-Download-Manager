// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! `cargo xtask android-manifest [merged.xml]`: the Android app's safety held by a check rather
//! than by hand (gaps row H17). The permissions EDM asks for, the components other apps may
//! reach, backups off and the app not debuggable, compared with
//! `crates/edm-android/android/manifest-kept.txt`. CI's release job names the release build's
//! merged manifest, which holds what Gradle adds; with no argument the source manifest alone.

use std::collections::BTreeSet;
use std::path::Path;

/// The `name="value"` attributes of one tag, `android:` left off their names.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = tag;
    while let Some(eq) = rest.find("=\"") {
        let name = rest[..eq]
            .rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("")
            .trim_start_matches("android:")
            .to_string();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        found.push((name, after[..end].to_string()));
        rest = &after[end + 1..];
    }
    found
}

/// What `xml` (a manifest) asks for and opens, as the kept list's lines say it.
fn said(xml: &str) -> BTreeSet<String> {
    // Comments first: one may hold a tag's words.
    let mut text = String::new();
    let mut rest = xml;
    while let Some(open) = rest.find("<!--") {
        text.push_str(&rest[..open]);
        rest = rest[open..]
            .find("-->")
            .map_or("", |end| &rest[open + end + 3..]);
    }
    text.push_str(rest);
    let mut lines = BTreeSet::new();
    // Each component's tag, and whether an intent filter under it makes it reachable when
    // `exported` is not written (Android 12 and up refuse that, older ones export it).
    let tags: Vec<&str> = text.split('<').skip(1).collect();
    for (i, tag) in tags.iter().enumerate() {
        let tag = tag.split('>').next().unwrap_or("");
        let kind = tag.split_whitespace().next().unwrap_or("");
        let attrs = attributes(tag);
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        };
        match kind {
            "uses-permission" | "uses-permission-sdk-23" => {
                lines.insert(format!("permission {}", get("name").unwrap_or("?")));
            }
            "application" => {
                lines.insert(format!(
                    "application allowBackup {}",
                    get("allowBackup").unwrap_or("unset")
                ));
                if let Some(debuggable) = get("debuggable") {
                    lines.insert(format!("application debuggable {debuggable}"));
                }
            }
            "activity" | "activity-alias" | "service" | "receiver" | "provider" => {
                let filtered = tags[i + 1..]
                    .iter()
                    .take_while(|t| !t.starts_with(&format!("/{kind}")))
                    .any(|t| t.starts_with("intent-filter"));
                let exported = match get("exported") {
                    Some(v) => v == "true",
                    None => filtered,
                };
                if exported {
                    let mut line = format!("exported {kind} {}", get("name").unwrap_or("?"));
                    if let Some(permission) = get("permission") {
                        line.push_str(&format!(" guarded by {permission}"));
                    }
                    lines.insert(line);
                }
            }
            _ => {}
        }
    }
    lines
}

/// The kept list's lines.
fn kept(root: &Path) -> Result<BTreeSet<String>, String> {
    let path = root.join("crates/edm-android/android/manifest-kept.txt");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect())
}

/// Holds the source manifest, and `merged` where given, to the kept list.
pub fn run(root: &Path, merged: Option<&str>) -> Result<String, String> {
    let kept = kept(root)?;
    let mut manifests =
        vec![root.join("crates/edm-android/android/app/src/main/AndroidManifest.xml")];
    manifests.extend(merged.map(std::path::PathBuf::from));
    for manifest in &manifests {
        let xml = std::fs::read_to_string(manifest)
            .map_err(|e| format!("{}: {e}", manifest.display()))?;
        let said = said(&xml);
        let added: Vec<_> = said.difference(&kept).collect();
        let gone: Vec<_> = kept.difference(&said).collect();
        if !added.is_empty() || !gone.is_empty() {
            return Err(format!(
                "{} differs from crates/edm-android/android/manifest-kept.txt: not kept {added:?}, \
                 missing {gone:?}; change the list in the same commit, so the change is seen",
                manifest.display()
            ));
        }
    }
    Ok(format!(
        "the Android manifest holds only what is kept ({} checked)",
        manifests.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_android_manifest_holds_only_what_is_kept() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        run(&root, None).unwrap();
    }

    #[test]
    fn the_reading_finds_what_a_change_would_add() {
        let xml = r#"<manifest>
        <!-- <uses-permission android:name="IN.A.COMMENT" /> -->
        <uses-permission android:name="android.permission.CAMERA" />
        <application android:allowBackup="true" android:debuggable="true">
          <service android:name="a.Open"><intent-filter><action android:name="x"/></intent-filter></service>
          <service android:name="a.Shut" android:exported="false"><intent-filter/></service>
          <provider android:name="a.Files" android:exported="true" android:permission="p.Q" />
          <receiver android:name="a.Plain" />
        </application></manifest>"#;
        let said: Vec<String> = said(xml).into_iter().collect();
        assert_eq!(
            said,
            [
                "application allowBackup true",
                "application debuggable true",
                "exported provider a.Files guarded by p.Q",
                "exported service a.Open",
                "permission android.permission.CAMERA",
            ]
        );
    }
}
