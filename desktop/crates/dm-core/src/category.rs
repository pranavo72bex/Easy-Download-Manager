//! Sorting finished files into folders by what they are.
//!
//! The rules are the extension's, in `naming.js`, deliberately: the two
//! implementations save the same file, and a video that lands in `Videos`
//! under one and `Other` under the other is a bug people report as lost
//! downloads.

use std::path::{Path, PathBuf};

/// What kind of thing a download turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    // Rows saved before the categories had these names still read.
    #[serde(alias = "video")]
    Videos,
    #[serde(alias = "music")]
    Audio,
    Documents,
    #[serde(alias = "programs")]
    Apps,
    #[serde(alias = "compressed")]
    Archives,
    #[serde(alias = "images")]
    Pictures,
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
    // something (gap 168). `uninstall` and `reinstall` do not say so.
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
/// Windows, after the whole file had arrived (gap 64). A name already saved
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
mod tests {
    /// Gap 168: a `.bin` is not taken for a program by its extension.
    #[test]
    fn a_bin_file_is_other_unless_its_name_says_it_installs() {
        use super::{classify, Category};
        for name in ["firmware.bin", "bulk001.bin", "disc.BIN", "weights.bin"] {
            assert_eq!(classify(name, None), Category::Other, "{name}");
        }
        // The server's type does not make one a program either.
        assert_eq!(
            classify("router.bin", Some("application/octet-stream")),
            Category::Other
        );
        for name in ["jdk-setup-x64.bin", "Product-Installer.bin", "Setup64.bin"] {
            assert_eq!(classify(name, None), Category::Apps, "{name}");
        }
        // Words that only contain "install" are not an installer.
        for name in ["uninstall-firmware.bin", "reinstall_image.bin"] {
            assert_eq!(classify(name, None), Category::Other, "{name}");
        }
        // A type the server names still places it.
        assert_eq!(classify("stream.bin", Some("video/mp2t")), Category::Videos);
        assert_eq!(classify("song.bin", Some("audio/mpeg")), Category::Audio);
        assert_eq!(classify("tool.exe", None), Category::Apps);
        assert_eq!(classify("tool.run", None), Category::Apps);
    }

    #[test]
    fn a_folder_name_is_judged_as_it_is_used() {
        assert!(!super::one_folder(" .. "));
        assert!(!super::one_folder(" . "));
        assert!(super::one_folder(" Books "));
    }

    /// Gap 64: a new name Windows cannot make is refused on every system.
    #[test]
    fn a_folder_name_windows_cannot_make_is_refused_everywhere() {
        for bad in [
            "Docs: work",
            "CON",
            "nul.txt",
            "COM1",
            "a?",
            "x*y",
            "<a>",
            "pipe|",
            "q\"",
            "end.",
            "tab\there",
        ] {
            assert!(super::portable_folder(bad).is_err(), "{bad:?} was taken");
            // Gap 77: a saved name that works here is still used here.
            assert_eq!(super::one_folder(bad), !cfg!(windows), "{bad:?} here");
        }
        assert!(super::portable_folder("Console games").is_ok());
        assert!(super::one_folder("Console games"));
        assert!(super::portable_folder(" .. ").is_err());
        let mut categories = super::Categories {
            enabled: true,
            ..super::Categories::default()
        };
        categories.folders[0] = " Films ".into();
        assert_eq!(
            categories.destination(std::path::Path::new("/d"), super::Category::ALL[0]),
            std::path::Path::new("/d/Films")
        );
    }

    use super::*;

    fn custom(name: &str, extensions: &[&str], sites: &[&str]) -> Custom {
        Custom {
            name: name.into(),
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            sites: sites.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_category_a_person_made_takes_its_types_from_its_sites() {
        let lectures = custom("Lectures", &["mp4", ".PDF"], &["university.edu"]);
        assert!(lectures.takes("week1.mp4", "university.edu"));
        assert!(
            lectures.takes("notes.pdf", "media.university.edu"),
            "a subdomain"
        );
        assert!(
            !lectures.takes("week1.mp4", "notuniversity.edu"),
            "not a suffix match"
        );
        assert!(
            !lectures.takes("week1.mkv", "university.edu"),
            "another type"
        );

        let anything_from = custom("Work", &[], &["*.corp.example"]);
        assert!(anything_from.takes("q3.xlsx", "files.corp.example"));
        assert!(!anything_from.takes("q3.xlsx", "example.com"));

        let any_site = custom("Fonts", &["ttf", "otf"], &[]);
        assert!(any_site.takes("Inter.ttf", "anywhere.example"));

        assert!(
            !custom("Everything", &[], &[]).takes("a.zip", "x.com"),
            "takes nothing"
        );
    }

    #[test]
    fn a_made_category_comes_before_the_built_in_kinds_and_stays_one_folder() {
        let root = Path::new("/d");
        let mut categories = Categories {
            enabled: true,
            custom: vec![
                custom("../escape", &["zip"], &[]),
                custom("Fonts", &["ttf"], &[]),
                custom("Lectures", &["mp4"], &["university.edu"]),
            ],
            site: "university.edu".into(),
            ..Categories::default()
        };
        assert_eq!(
            categories.place(root, Category::Videos, "w.mp4"),
            root.join("Lectures")
        );
        assert_eq!(
            categories.place(root, Category::Other, "Inter.ttf"),
            root.join("Fonts")
        );
        assert_eq!(
            categories.place(root, Category::Archives, "a.zip"),
            root.join("Archives"),
            "a name that leaves the folder is skipped, not followed"
        );
        categories.site = "youtube.com".into();
        assert_eq!(
            categories.place(root, Category::Videos, "w.mp4"),
            root.join("Videos")
        );
        categories.enabled = false;
        assert_eq!(categories.place(root, Category::Other, "Inter.ttf"), root);
        assert_eq!(
            host_of("https://Media.University.EDU:8443/x.mp4"),
            "media.university.edu"
        );
        assert_eq!(host_of("magnet:?xt=urn:btih:abc"), "");
    }

    #[test]
    fn the_common_kinds_land_where_they_should() {
        for (name, expected) in [
            ("Sintel.2010.1080p.mkv", Category::Videos),
            ("song.MP3", Category::Audio),
            ("report.pdf", Category::Documents),
            ("ubuntu-24.04.iso", Category::Archives),
            ("installer.dmg", Category::Apps),
            ("photo.jpeg", Category::Pictures),
            ("notes", Category::Other),
        ] {
            assert_eq!(classify(name, None), expected, "for {name}");
        }
    }

    #[test]
    fn media_with_no_extension_is_sorted_by_what_the_server_called_it() {
        // Player-served media routinely arrives from a URL with no extension,
        // and those are the downloads people most want sorted.
        assert_eq!(classify("clip", Some("video/mp4")), Category::Videos);
        assert_eq!(
            classify("track", Some("audio/mpeg; charset=x")),
            Category::Audio
        );
        assert_eq!(classify("thing", Some("application/zip")), Category::Other);
    }

    #[test]
    fn the_extension_wins_over_the_content_type() {
        // Servers label things application/octet-stream constantly.
        assert_eq!(
            classify("film.mkv", Some("application/octet-stream")),
            Category::Videos
        );
    }

    #[test]
    fn sorting_off_leaves_everything_in_one_folder() {
        let categories = Categories::default();
        assert!(!categories.enabled);
        assert_eq!(
            categories.destination(Path::new("/d"), Category::Videos),
            Path::new("/d")
        );
    }

    #[test]
    fn sorting_on_nests_by_category() {
        let categories = Categories {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(
            categories.destination(Path::new("/d"), Category::Videos),
            Path::new("/d/Videos")
        );
    }

    #[test]
    fn an_empty_folder_name_means_do_not_nest_that_one() {
        let mut categories = Categories {
            enabled: true,
            ..Default::default()
        };
        categories.folders[Category::ALL
            .iter()
            .position(|c| *c == Category::Other)
            .unwrap()] = String::new();
        assert_eq!(
            categories.destination(Path::new("/d"), Category::Other),
            Path::new("/d")
        );
        assert_eq!(
            categories.destination(Path::new("/d"), Category::Audio),
            Path::new("/d/Audio")
        );
    }

    #[test]
    fn a_folder_name_cannot_escape_the_download_folder() {
        /* These come from a JSON file anyone can edit, and "../.." would put
         * downloads somewhere nobody asked for. */
        let categories = Categories {
            enabled: true,
            folders: vec!["../../etc".into(); Category::ALL.len()],
            ..Categories::default()
        };
        assert_eq!(
            categories.destination(Path::new("/d"), Category::Videos),
            Path::new("/d")
        );
    }
}
