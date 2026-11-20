// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! The folders of the phone's storage dm can write by their path (gaps row N9), found once as
//! dm starts: the shared `Download/dm` (Android's `Environment.getExternalStoragePublicDirectory`),
//! where the phone lets dm write there (Android 8 and 9 once storage is allowed, 11 and up), and
//! the app's own folder on the storage. They are where downloads go by default, what Browse…
//! offers (`crate::phone::Places`), and where Export and Back up dm write on a phone, whose
//! GPUI has no save panel.

use std::path::{Path, PathBuf};

use android_activity::AndroidApp;
use jni::{jni_sig, jni_str, JValue};

use super::jvm;
use crate::phone::Places;

/// The folders dm can write by path: each made and probed with a file, left out where dm
/// cannot write there.
pub(super) fn places(app: &AndroidApp) -> Places {
    let shared = public_downloads()
        .map(|folder| folder.join("dm"))
        .filter(|folder| writable(folder));
    let own = app
        .external_data_path()
        .map(|own| own.join("Download"))
        .filter(|folder| writable(folder));
    Places { shared, own }
}

/// The phone's shared Download folder, as Android names it.
fn public_downloads() -> Option<PathBuf> {
    let public = jvm::with("find the Download folder", |env, _| {
        let name = env
            .get_static_field(
                jni_str!("android/os/Environment"),
                jni_str!("DIRECTORY_DOWNLOADS"),
                jni_sig!(java.lang.String),
            )?
            .l()?;
        let folder = env
            .call_static_method(
                jni_str!("android/os/Environment"),
                jni_str!("getExternalStoragePublicDirectory"),
                jni_sig!("(Ljava/lang/String;)Ljava/io/File;"),
                &[JValue::Object(&name)],
            )?
            .l()?;
        if folder.is_null() {
            return Ok(None);
        }
        let path = env
            .call_method(
                &folder,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        jvm::string_of(env, path)
    });
    public.ok().flatten().map(PathBuf::from)
}

/// Whether dm can make `folder` and write a file in it.
fn writable(folder: &Path) -> bool {
    if std::fs::create_dir_all(folder).is_err() {
        return false;
    }
    let probe = folder.join(".dm-probe");
    let written = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    written
}
