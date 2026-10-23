use super::*;

/// The ranges this download is fetched in: the plan its staged parts were
/// fetched under where there are any, a fresh one otherwise.
///
/// Parts staged under a plan that does not match — the file's length has
/// changed, or the parts predate the plan being written down — are not
/// adopted, because there is no offset to trust them at. Starting over costs
/// the bytes; adopting them costs the file.
pub(super) async fn planned_segments(
    info: &Probe,
    options: &Options,
) -> Result<Vec<Segment>, Error> {
    let parts_dir = options.parts_dir.clone();
    let recorded = tokio::task::spawn_blocking(move || plan::recorded(&parts_dir))
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

    match recorded {
        /* Only parts of this very file, fetched from a server that serves
         * ranges. A server that does not is asked for the whole file every
         * time, so nothing staged from it can be continued; and a file
         * whose validator moved is another file of the same length. */
        Some(recorded)
            if recorded.size == info.size
                && recorded.resumable
                && info.resumable
                && recorded.validator == info.validator =>
        {
            /* Ledgers (and an earlier dm's part files) past the plan's last
             * range belong to a split the plan does not remember — a power
             * cut can leave the plan from before it, which is safe only if
             * they go: a later split reusing the index would take its bytes
             * as done at another offset. */
            let ranges = recorded.segments().len();
            if let Ok(mut listing) = fs::read_dir(&options.parts_dir).await {
                while let Ok(Some(entry)) = listing.next_entry().await {
                    let name = entry.file_name();
                    let index = name.to_str().and_then(staged_index);
                    if index.is_some_and(|i| i >= ranges) {
                        let _ = fs::remove_file(entry.path()).await;
                    }
                }
            }
            return Ok(recorded.segments());
        }
        _ => {}
    }

    // Nothing here can be finished under any plan this run could write.
    // Only the parts go: the directory itself may be watched by a caller.
    let mut listing = fs::read_dir(&options.parts_dir).await?;
    while let Some(entry) = listing.next_entry().await? {
        let _ = fs::remove_file(entry.path()).await;
    }

    // One range a connection: as few requests as there are connections, so
    // a server that limits how fast requests may come (nginx's 429) is not
    // met with a burst; the halving below shares the work out as it goes.
    let segments = plan::plan(info.size, options.connections, info.resumable);
    let (dir, size, resumable, validator, written) = (
        options.parts_dir.clone(),
        info.size,
        info.resumable,
        info.validator.clone(),
        segments.clone(),
    );
    tokio::task::spawn_blocking(move || {
        plan::record(&dir, size, resumable, validator.as_deref(), &written)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;
    Ok(segments)
}

/// Probes every mirror and keeps the ones describing the same file.
///
/// Mirrors that disagree about the length are not mirrors — they are a stale
/// copy and a current one, and splicing ranges from both produces a file that
/// is the right size and wrong throughout. That is the one failure this
/// engine must never produce, so disagreement costs the mirror rather than
/// the download.
pub(super) async fn agreeing_mirrors(
    client: &reqwest::Client,
    listed: &Metalink,
) -> Result<(Vec<Source>, Probe), Error> {
    let probes = futures_util::future::join_all(
        listed
            .mirrors
            .iter()
            // A Metalink's mirrors are their own hosts; the page the
            // download came from is not the referer for any of them.
            .map(|url| probe(client, url, None, None)),
    )
    .await;

    let mut reference: Option<Probe> = None;
    let mut sources = Vec::new();
    let mut last_error = None;
    // A mirror that states no length, kept in case none states the right one.
    let mut lengthless: Option<Probe> = None;
    let mut disagreed = false;

    /* The document's length, where it states one, is the reference: it is
     * what the publisher signed off on. Taken from the best mirror instead,
     * a stale first mirror set the length and the current ones were dropped
     * as the ones that disagreed. */
    let expected = listed.size;

    // In the document's order, which is best first, so the reference is the
    // best mirror that answered rather than whichever answered first.
    for found in probes {
        let found = match found {
            Ok(found) => found,
            Err(e) => {
                last_error = Some(e);
                continue;
            }
        };
        let agrees = match (expected, &reference) {
            (Some(_), _) if found.size.is_none() => {
                if lengthless.is_none() {
                    lengthless = Some(found);
                }
                continue;
            }
            (Some(size), _) => found.size == Some(size),
            (None, None) => true,
            // An unstated length cannot be shown to agree, so it does not
            // get to join in.
            (None, Some(first)) => found.size.is_some() && found.size == first.size,
        };
        if !agrees {
            disagreed = true;
            continue;
        }
        sources.push(Source {
            url: found.final_url.clone(),
            validator: found.validator.clone(),
        });
        if reference.is_none() {
            reference = Some(found);
        }
    }

    /* No mirror stated the document's length, but one stated none: fetched
     * alone, held to the document's length, rather than refused. */
    if reference.is_none() {
        if let Some(only) = lengthless {
            sources.push(Source {
                url: only.final_url.clone(),
                validator: only.validator.clone(),
            });
            reference = Some(only);
        }
    }

    match reference {
        Some(mut agreed) => {
            if agreed.size.is_none() {
                agreed.size = listed.size;
            }
            Ok((sources, agreed))
        }
        None if disagreed => Err(Error::Internal(
            "no mirror in that Metalink has the file it describes: every one that answered \
             disagreed about its length"
                .into(),
        )),
        None => Err(last_error
            .unwrap_or_else(|| Error::Internal("not one mirror in that Metalink answered".into()))),
    }
}
