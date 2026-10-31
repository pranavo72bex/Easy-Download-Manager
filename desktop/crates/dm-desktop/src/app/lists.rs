use super::*;

impl DmApp {
    /// Imports the list in the file at `path`. Addresses already in the
    /// list are asked about first, as Add URL asks.
    pub fn import_from(&mut self, path: &std::path::Path) -> Result<(), String> {
        // A .torrent is a download, not a list of them: read as text it was
        // "not valid UTF-8".
        if let Some(local) = crate::dialogs::local_torrent(&path.to_string_lossy()) {
            return match self.add(&[local], Default::default()) {
                Ok(_) => Ok(()),
                Err(e) => {
                    self.said = e.clone().into();
                    Err(e)
                }
            };
        }
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                self.said = crate::i18n::Said::new(
                    "{} could not be read: {}",
                    &[&path.display(), &format::file_error(&e)],
                );
                return Err(self.said.shown());
            }
        };
        let listed = Backend::listed(&text);
        let already = listed
            .iter()
            .filter(|url| crate::dialogs::already_listed(self, url).is_some())
            .count();
        if already > 0 {
            self.dialogs.import_ask = Some(crate::dialogs::ImportAsk {
                text,
                already,
                total: listed.len(),
            });
            return Ok(());
        }
        self.import_text(&text, false);
        Ok(())
    }

    /// Adds the downloads a list holds; `skip_listed`, not those whose
    /// address is already a row.
    pub fn import_text(&mut self, text: &str, skip_listed: bool) {
        let before: HashSet<Id> = self.backend.list().iter().map(|r| r.id).collect();
        let listed: HashSet<String> = if skip_listed {
            Backend::listed(text)
                .into_iter()
                .filter(|url| crate::dialogs::already_listed(self, url).is_some())
                .collect()
        } else {
            HashSet::new()
        };
        let added = self.backend.import_where(text, |url| !listed.contains(url));
        // A list is not a run of progress windows.
        self.own.extend(
            self.backend
                .list()
                .iter()
                .map(|r| r.id)
                .filter(|id| !before.contains(id)),
        );
        // Lines of a plain list with no address dm can read are said, not
        // passed over: "127.0.0.1:8000/notes.txt" went without a word.
        let unread = if text.trim_start().starts_with('{') {
            0
        } else {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .filter(|l| !l.contains("://") && !l.to_ascii_lowercase().contains("magnet:?"))
                .count()
        };
        self.said = if unread > 0 {
            crate::i18n::Said::naming(
                "Imported {}. Skipped {} lines with no link in them (a link starts with http://, https://, ftp:// or magnet:).",
                &[&added, &unread],
            )
        } else {
            crate::i18n::Said::counting(
                "Imported {} download.||Imported {} downloads.",
                added as u64,
                &[&added],
            )
        };
        self.refresh();
    }

    /// Writes the list to `path`, and says where or why not.
    pub fn export_to(&mut self, path: &std::path::Path) -> Result<(), String> {
        match std::fs::write(path, self.backend.export()) {
            Ok(()) => {
                self.said = crate::i18n::Said::new("Exported to {}.", &[&path.display()]);
                Ok(())
            }
            Err(e) => {
                self.said = crate::i18n::Said::new(
                    "{} could not be written: {}",
                    &[&path.display(), &format::file_error(&e)],
                );
                Err(self.said.shown())
            }
        }
    }
}
