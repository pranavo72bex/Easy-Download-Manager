// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Sorting finished files into folders by what they are.
//!
//! The rules are dm's alone: the extension hands a download over and dm
//! decides where it lands, so a file lands in the same folder however it was
//! added. A video that lands in `Videos` one way and `Other` another is a bug
//! people report as lost downloads.

use std::path::{Path, PathBuf};

/// What kind of thing a download turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    // Rows saved before the categories had these names still read.
    /// Video files.
    #[serde(alias = "video")]
    Videos,
    /// Sound files.
    #[serde(alias = "music")]
    Audio,
    /// Documents: text, office files, books.
    Documents,
    /// Programs and installers.
    #[serde(alias = "programs")]
    Apps,
    /// Compressed archives.
    #[serde(alias = "compressed")]
    Archives,
    /// Pictures.
    #[serde(alias = "images")]
    Pictures,
    /// Anything the other kinds do not take.
    Other,
}

impl Category {
    /// The folder name this category uses unless told otherwise.
    pub fn folder(self) -> &'static str {
        match self {
            Category::Videos => "Videos",
            Category::Audio => "Audio",
            Category::Documents => "Documents",
            Category::Apps => "Apps",
            Category::Archives => "Archives",
            Category::Pictures => "Pictures",
            Category::Other => "Other",
        }
    }

    /// Every kind, in the order the lists and the folder names use.
    pub const ALL: [Category; 7] = [
        Category::Videos,
        Category::Audio,
        Category::Documents,
        Category::Apps,
        Category::Archives,
        Category::Pictures,
        Category::Other,
    ];
}

/// Extension to category, matching `EXT_CATEGORY` in the extension.
const BY_EXTENSION: &[(&str, Category)] = &[
    ("mp4", Category::Videos),
    ("mkv", Category::Videos),
    ("avi", Category::Videos),
    ("mov", Category::Videos),
    ("wmv", Category::Videos),
    ("flv", Category::Videos),
    ("webm", Category::Videos),
    ("m4v", Category::Videos),
    ("mpg", Category::Videos),
    ("mpeg", Category::Videos),
    ("3gp", Category::Videos),
    ("ts", Category::Videos),
    ("mp3", Category::Audio),
    ("flac", Category::Audio),
    ("wav", Category::Audio),
    ("aac", Category::Audio),
    ("ogg", Category::Audio),
    ("m4a", Category::Audio),
    ("wma", Category::Audio),
    ("opus", Category::Audio),
    ("aiff", Category::Audio),
    ("alac", Category::Audio),
    ("pdf", Category::Documents),
    ("doc", Category::Documents),
    ("docx", Category::Documents),
    ("xls", Category::Documents),
    ("xlsx", Category::Documents),
    ("ppt", Category::Documents),
    ("pptx", Category::Documents),
    ("txt", Category::Documents),
    ("rtf", Category::Documents),
    ("odt", Category::Documents),
    ("ods", Category::Documents),
    ("epub", Category::Documents),
    ("mobi", Category::Documents),
    ("csv", Category::Documents),
    ("md", Category::Documents),
    ("exe", Category::Apps),
    ("msi", Category::Apps),
    ("dmg", Category::Apps),
    ("pkg", Category::Apps),
    ("deb", Category::Apps),
    ("rpm", Category::Apps),
    ("appimage", Category::Apps),
    ("apk", Category::Apps),
    ("run", Category::Apps),
    ("snap", Category::Apps),
    ("flatpak", Category::Apps),
    ("zip", Category::Archives),
    ("rar", Category::Archives),
    ("7z", Category::Archives),
    ("tar", Category::Archives),
    ("gz", Category::Archives),
    ("bz2", Category::Archives),
    ("xz", Category::Archives),
    ("zst", Category::Archives),
    ("iso", Category::Archives),
    ("tgz", Category::Archives),
    ("cab", Category::Archives),
    ("jpg", Category::Pictures),
    ("jpeg", Category::Pictures),
    ("png", Category::Pictures),
    ("gif", Category::Pictures),
    ("bmp", Category::Pictures),
    ("svg", Category::Pictures),
    ("webp", Category::Pictures),
    ("tiff", Category::Pictures),
    ("heic", Category::Pictures),
    ("avif", Category::Pictures),
    ("ico", Category::Pictures),
];

/// Classifies by extension, falling back to the content type's top level.
///
/// The fallback matters more than it looks: plenty of media arrives from a
/// URL with no extension at all, and putting all of it in `Other` defeats
/// the feature for exactly the downloads people most want sorted.
pub fn classify(filename: &str, content_type: Option<&str>) -> Category {
    if let Some(category) = by_name(filename) {
        return category;
    }
    let mime = crate::probe::mime_essence(content_type);
    match mime.split('/').next().unwrap_or("") {
        "video" => return Category::Videos,
        "audio" => return Category::Audio,
        "image" => return Category::Pictures,
        _ => {}
    }
    // A `.bin` is as often firmware, a disc image or a model's weights as a
    // program, and dm cannot tell which from the name: Other, the honest
    // place, unless the server typed it or the name says it installs
    // something. `uninstall` and `reinstall` do not say so.
    if extension_of(filename) == "bin"
        && filename
            .split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::to_ascii_lowercase)
            .any(|w| w.starts_with("setup") || w.starts_with("install"))
    {
        return Category::Apps;
    }
    Category::Other
}

/// The kind a file's name alone decides: an extension dm knows. A name
/// that does not say — none, `.php`, `.bin` — is placed by what the server
/// says of it.
pub fn by_name(filename: &str) -> Option<Category> {
    let extension = extension_of(filename);
    BY_EXTENSION
        .iter()
        .find(|(e, _)| *e == extension)
        .map(|(_, category)| *category)
}

fn extension_of(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A category a person made: a name, which is also its
/// folder, the file types it takes and, optionally, the only sites it takes
/// them from.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Custom {
    /// The name the person gave the kind; also its folder.
    pub name: String,
    /// Without the dot. Empty means any type, from the sites below.
    pub extensions: Vec<String>,
    /// Hosts; each also covers its subdomains. Empty means any site.
    pub sites: Vec<String>,
}

impl Custom {
    /// Whether a file called `name`, fetched from `host`, belongs here. A
    /// category with neither types nor sites takes nothing: it would
    /// otherwise swallow every download.
    pub fn takes(&self, name: &str, host: &str) -> bool {
        if self.extensions.is_empty() && self.sites.is_empty() {
            return false;
        }
        let extension = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let typed = self.extensions.is_empty()
            || self.extensions.iter().any(|e| {
                e.trim()
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
            });
        let host = host.to_ascii_lowercase();
        let sited = self.sites.is_empty()
            || self.sites.iter().any(|site| {
                let site = site.trim().trim_start_matches("*.").to_ascii_lowercase();
                !site.is_empty() && (host == site || host.ends_with(&format!(".{site}")))
            });
        typed && sited
    }
}

/// A folder name that stays one folder under the download folder, and that
/// this computer can make. Names come from a hand-editable settings file,
/// and one with a separator in it would put files somewhere else entirely.
/// This decides whether a saved name is loaded and used; a name being made
/// or renamed in Options is held to [`portable_folder`] as well.
pub fn one_folder(name: &str) -> bool {
    // Judged as it will be used, trimmed: " .. " passed as a name and was
    // joined as "..", the folder above the download folder.
    let name = name.trim();
    let here = !name.is_empty() && !name.contains(['/', '\\']) && name != ".." && name != ".";
    // On Windows a name Windows cannot make could never have worked.
    if cfg!(windows) {
        here && portable_folder(name).is_ok()
    } else {
        here
    }
}

/// Whether a folder name can be made on every computer dm runs on, and if
/// not, why. For a name being made or renamed: settings travel, and "Docs:
/// work" or "CON" accepted on a Mac failed every download into it on
/// Windows, after the whole file had arrived. A name already saved
/// that works here is kept ([`one_folder`]); this is not applied to it.
pub fn portable_folder(name: &str) -> Result<(), &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("it is empty");
    }
    if name.contains(['/', '\\']) || name == ".." || name == "." {
        return Err("it is not one folder");
    }
    if name.contains(|c: char| "<>:\"|?*".contains(c) || c.is_control()) {
        return Err("it holds a character Windows cannot use in a name: < > : \" | ? *");
    }
    if name.ends_with('.') {
        return Err("Windows cannot make a name ending in a dot");
    }
    if crate::probe::reserved_on_windows(name) {
        return Err("Windows keeps that name for a device");
    }
    Ok(())
}

/// The host of a download's address, lowercased; empty when it has none.
pub fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default()
}

/// Whether to sort at all, and what to call each folder.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Categories {
    /// Whether finished files are sorted into folders by kind at all.
    pub enabled: bool,
    /// One name per category, in the order of `Category::ALL`. An empty name
    /// means that category is not nested — a way to say "sort videos out but
    /// leave everything else where it lands" without a second setting.
    pub folders: Vec<String>,
    /// Categories a person made, looked at before the built-in kinds, first
    /// match wins.
    #[serde(default)]
    pub custom: Vec<Custom>,
    /// The host of the download being placed, filled in per download by the
    /// queue, for categories limited to sites.
    #[serde(default)]
    pub site: String,
}

impl Default for Categories {
    fn default() -> Self {
        Categories {
            enabled: false,
            folders: Category::ALL
                .iter()
                .map(|c| c.folder().to_string())
                .collect(),
            custom: Vec::new(),
            site: String::new(),
        }
    }
}

impl Categories {
    /// The category a person made that a file called `name` from `host`
    /// belongs to, if any.
    pub fn custom_for(&self, name: &str, host: &str) -> Option<&Custom> {
        self.custom
            .iter()
            .find(|c| one_folder(&c.name) && c.takes(name, host))
    }

    /// Where a finished file called `name`, of kind `category`, goes: the
    /// folder of a category a person made that takes it, else its kind's.
    pub fn place(&self, root: &Path, category: Category, name: &str) -> PathBuf {
        if !self.enabled {
            return root.to_path_buf();
        }
        match self.custom_for(name, &self.site) {
            Some(custom) => root.join(custom.name.trim()),
            None => self.destination(root, category),
        }
    }

    /// Where a file of this kind belongs, under `root`.
    pub fn destination(&self, root: &Path, category: Category) -> PathBuf {
        if !self.enabled {
            return root.to_path_buf();
        }
        let index = Category::ALL.iter().position(|c| *c == category);
        let name = index.and_then(|i| self.folders.get(i)).cloned();
        match name {
            // A folder name is a single component. Anything with a separator
            // in it would put files outside the download folder entirely,
            // and this string comes from a hand-editable settings file.
            Some(name) if one_folder(&name) => root.join(name.trim()),
            _ => root.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests;
