//! What the installers are built from, checked before a CI runner finds out.

/* Every icon the packager names has to exist, and be what it claims.
 *
 * A missing `.icns` or `.ico` fails the macOS or Windows job minutes into a
 * build on a machine nobody is watching — this project once had neither. A
 * missing file is something a test can see here in a millisecond. */
#[test]
fn the_icons_the_installers_name_are_all_there() {
    let manifest = include_str!("../Cargo.toml");
    let listed: Vec<&str> = manifest
        .split_once("icons = [")
        .expect("the packager names no icons")
        .1
        .split_once(']')
        .unwrap()
        .0
        .split(',')
        .map(|s| s.trim().trim_matches('"'))
        .filter(|s| !s.is_empty())
        .collect();

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut seen = Vec::new();
    for name in &listed {
        let bytes = std::fs::read(root.join(name))
            .unwrap_or_else(|e| panic!("{name} is named by the packager but {e}"));
        assert!(bytes.len() > 100, "{name} is empty");
        let kind = name.rsplit('.').next().unwrap();
        let magic: &[u8] = match kind {
            "png" => b"\x89PNG",
            "icns" => b"icns",
            "ico" => &[0x00, 0x00, 0x01, 0x00],
            other => panic!("{other} is not an icon format the packager reads"),
        };
        assert!(
            bytes.starts_with(magic),
            "{name} does not begin like a {kind} file"
        );
        seen.push(kind);
    }
    for needed in ["icns", "ico", "png"] {
        assert!(
            seen.contains(&needed),
            "no {needed} — that platform cannot be packaged"
        );
    }
}

/* The binary keeps the name everything else already uses: the installer
 * (`make register`), the browser's native-host manifest, the Makefile,
 * the PKGBUILD and the CI jobs all look for `dm-desktop`. */
#[test]
fn the_binary_is_named_what_the_installer_looks_for() {
    let manifest = include_str!("../Cargo.toml");
    assert!(manifest.contains("[[bin]]\nname = \"dm-desktop\""));
    assert!(manifest.contains("binaries = [{ path = \"dm-desktop\", main = true }]"));
    let makefile = include_str!("../../../../Makefile");
    assert!(makefile.contains("target/release/dm-desktop"));
}

/// Open and Open folder go through xdg-open on Linux, which comes with
/// xdg-utils: every Linux package has to bring it, or a minimal desktop
/// installs dm and cannot open what it downloaded.
#[test]
fn every_linux_package_requires_what_opens_files() {
    for (package, text) in [
        (".deb", include_str!("../Cargo.toml")),
        (".rpm", include_str!("../../../packaging/nfpm.yaml")),
        ("Arch", include_str!("../../../packaging/PKGBUILD")),
    ] {
        assert!(
            text.contains("xdg-utils"),
            "the {package} does not require xdg-utils"
        );
    }
}
