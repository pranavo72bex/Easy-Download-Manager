//! The Windows program's own description: its icon, and the product name
//! and version Explorer, SmartScreen and a code signature read off it.
//! Nothing is done for any other system.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts = version
        .split(['.', '-'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let icon = std::path::Path::new(&dir).join("icons").join("icon.ico");
    let icon = icon.display().to_string().replace('\\', "\\\\");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "dm contributors"
      VALUE "FileDescription", "dm download manager"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "dm-desktop"
      VALUE "LegalCopyright", "Copyright (c) 2026 dm contributors. MIT License."
      VALUE "OriginalFilename", "dm-desktop.exe"
      VALUE "ProductName", "dm"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    let path = std::path::Path::new(&out).join("dm.rc");
    if let Err(e) = std::fs::write(&path, rc) {
        println!("cargo:warning=the program's description could not be written: {e}");
        return;
    }
    // A release is built on CI, which must not ship without it; a build
    // from another system with no resource compiler goes on without.
    if let Err(e) = embed_resource::compile(&path, embed_resource::NONE).manifest_optional() {
        if std::env::var_os("CI").is_some() {
            println!("cargo:warning=the program's description could not be compiled: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=built without the program's description: {e}");
    }
}
