use super::*;

impl Queue {
    /// The recheck queue: asks the server about every finished row
    /// of `queue` again and downloads afresh each one whose file changed
    /// there: a different ETag or Last-Modified, or a different length.
    /// Returns how many were checked and which are being fetched again. A
    /// server that cannot be reached, or says nothing to compare, leaves its
    /// row alone; a row whose address is a page yt-dlp read or a stream's
    /// playlist has nothing to compare and is not counted as checked.
    pub async fn synchronize(&self, queue: &str) -> (usize, Vec<Id>) {
        // One at a time: a second check over the same rows, from a click
        // while the timer's runs, held the old validators and fetched again
        // what the first had just fetched.
        if self.inner.rechecking.swap(true, Ordering::SeqCst) {
            return (0, Vec::new());
        }
        let found = self.synchronize_now(queue).await;
        self.inner.rechecking.store(false, Ordering::SeqCst);
        found
    }

    pub(super) async fn synchronize_now(&self, queue: &str) -> (usize, Vec<Id>) {
        // A scheme is case-insensitive: an `HTTP://` row is as much a
        // server's file as an `http://` one, and was never asked about.
        let web = |url: &str| {
            ["http://", "https://"].iter().any(|scheme| {
                url.get(..scheme.len())
                    .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
            })
        };
        let rows: Vec<Entry> = self
            .list()
            .await
            .into_iter()
            .filter(|e| e.queue == queue && e.state == State::Completed)
            .filter(|e| web(&e.url))
            .collect();
        let client = self.inner.client.lock().await.clone();
        let (mut checked, mut changed) = (0, Vec::new());
        for row in &rows {
            let referrer = (!row.referrer.is_empty()).then_some(row.referrer.as_str());
            let sign_in = crate::cookies::SignIn::new(&row.url, &row.cookies);
            let Ok(now) = crate::probe::probe(&client, &row.url, referrer, sign_in.as_ref()).await
            else {
                continue;
            };
            let Some(different) = self
                .compare(&client, row, &now, referrer, sign_in.as_ref())
                .await
            else {
                continue;
            };
            checked += 1;
            if !different {
                continue;
            }
            // Asked a moment ago, from a list taken at the start: the row
            // must still be there, finished, with the same file, or the
            // file is not the one this check is about (removed from the
            // list and kept, or fetched again since).
            let same = self.get(row.id).await.is_some_and(|now| {
                now.state == State::Completed
                    && now.path == row.path
                    && now.validator == row.validator
            });
            if !same {
                continue;
            }
            // The outdated copy stays until the new one is whole, then the
            // new one takes its name rather than arriving beside it as
            // "name (1)". Deleted first, a failed fetch left neither.
            let keep = (!row.path.is_empty()).then(|| row.path.clone());
            if self.fetch_again(row.id, keep).await {
                changed.push(row.id);
            }
        }
        (checked, changed)
    }

    /// Whether a finished row's file differs from what its address serves
    /// now; `None` where there is nothing comparable to ask, which is not
    /// counted as checked.
    ///
    /// What the row recorded is of the file, and the address is not always
    /// the file's: a Metalink row's address is the mirror list, a yt-dlp
    /// row's is a web page, a stream's is its playlist. Compared with the
    /// file's length and ETag, the document always differed, and every such
    /// row was fetched again on every check.
    pub(super) async fn compare(
        &self,
        client: &reqwest::Client,
        row: &Entry,
        now: &crate::Probe,
        referrer: Option<&str>,
        sign_in: Option<&crate::cookies::SignIn>,
    ) -> Option<bool> {
        let kind = now.content_type.as_deref();
        // A page yt-dlp made a video of, or a playlist of segments: what the
        // address serves says nothing about the file made from it.
        if crate::ytdlp::is_page(kind) || crate::stream::kind_of(&row.url, kind).is_some() {
            return None;
        }
        if !crate::metalink::is_metalink(&row.url, kind) {
            return Some(differs(row, now));
        }
        // The file a Metalink names: its length, as the document states it
        // or as a mirror reports it. Not its ETag: the row's is the one
        // mirror's it was probed from, and every mirror has its own.
        // Fetched as a download fetches it: retried, under the speed
        // ceiling, and capped. Retried a bounded number of times even where
        // the setting says to keep trying: nobody is waiting on a check.
        let mut options = crate::Options {
            control: Control::new(),
            referrer: referrer.map(str::to_string),
            sign_in: sign_in.cloned(),
            ..self.inner.options.lock().await.clone()
        };
        if options.attempts == 0 {
            options.attempts = crate::retry::MAX_ATTEMPTS;
        }
        let listed = crate::download::fetch_metalink(client, &now.final_url, &options)
            .await
            .ok()?;
        let size = match listed.size {
            Some(size) => Some(size),
            None => {
                let mut size = None;
                for mirror in &listed.mirrors {
                    if let Ok(probed) = crate::probe::probe(client, mirror, None, None).await {
                        size = probed.size;
                        break;
                    }
                }
                size
            }
        };
        let file = crate::Probe {
            size,
            validator: None,
            ..now.clone()
        };
        Some(differs(row, &file))
    }

    /// Has the recheck queue ask its servers again every `every` on its
    /// own, counted from now; `None` only when asked. Running the check
    /// by hand does not move the clock: a person who asked now still gets
    /// the next one on time.
    pub fn set_recheck_every(&self, every: Option<Duration>) {
        let mut recheck = crate::lock(&self.inner.recheck);
        if recheck.0 != every {
            *recheck = (every, now_millis());
        }
    }

    /// Runs the recheck queue's check if its timer says so, and says what
    /// it found; `None` when there is no timer or it is not yet time.
    pub async fn recheck_if_due(&self) -> Option<(usize, Vec<Id>)> {
        let now = now_millis();
        let due = {
            let mut recheck = crate::lock(&self.inner.recheck);
            let every = recheck.0?;
            if recheck.1 > now {
                // A last run later than now is a clock that was once set
                // ahead. Waited out, it held the recheck until that date;
                // it is stale, and the count starts again from now.
                recheck.1 = now;
                false
            } else if u128::from(now - recheck.1) < every.as_millis() {
                return None;
            } else {
                recheck.1 = now;
                true
            }
        };
        if let Some(at) = self.recheck_file().await {
            if let Err(e) = tokio::fs::write(&at, now.to_string()).await {
                self.note(format!(
                    "when the recheck queue last ran could not be kept in {}: {e}",
                    at.display()
                ));
            }
        }
        if !due {
            return None;
        }
        Some(self.synchronize(RECHECK_QUEUE).await)
    }
}
