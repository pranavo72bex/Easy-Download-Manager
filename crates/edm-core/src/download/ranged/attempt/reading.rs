// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! An attempt's body read into its place: chunk by chunk, within the window and the limits, the
//! lead and the tail checked, and the range's end.

use super::*;

impl Reading<'_> {
    /// Reads the answer into the range until it ends, the range is whole, or the attempt
    /// stops.
    pub(super) async fn read(&mut self, plan: &Plan) -> Result<(), Error> {
        let job = self.job;
        let (start, end, reach) = (plan.start, plan.end, plan.reach);
        loop {
            self.drain().await?;
            self.judge_window().await?;
            let chunk = match self.next_chunk().await? {
                ControlFlow::Continue(chunk) => chunk,
                ControlFlow::Break(Flow::Again) => continue,
                ControlFlow::Break(Flow::Ended) => break,
            };
            // A stream that dies mid-range leaves what it wrote; flushing before
            // giving up is what lets the retry pick up from there instead of
            // fetching those bytes again.
            let mut chunk = match chunk {
                Ok(c) => c,
                // Every byte the range asked for is here, and the server hung
                // up short of the length it stated: the range is whole. Taken as
                // a failure, it was asked for again and again, each time whole,
                // the row at 100 % and running for ever (BUGLOG.md, BUG-001).
                Err(_) if self.written >= self.span_now => break,
                Err(e) => {
                    self.file.flush().await?;
                    put(&mut self.ledger, self.written).await?;
                    return Err(Error::Network(e));
                }
            };
            // The lead bytes are kept aside, not written into this range.
            self.keep_lead(&mut chunk, plan.lead_n);
            let Some(chunk) = self.checked(chunk).await? else {
                continue;
            };

            if !plan.is_probe_body && start + self.written + chunk.len() as u64 > reach.max(end) + 1
            {
                /* More bytes than the request asked for: past the range's end, or
                 * past the ranges after it it asked on over (`reach`).
                 *
                 * A server holding a compressed representation ranges over the
                 * *encoded* bytes and hands back the decoded ones, and nothing in
                 * the response says the two do not correspond. Trimming to length
                 * here is what makes that silent: every part ends up exactly as
                 * long as planned, assembled in order, and wrong all the way
                 * through. A file of the right size and the wrong contents is the
                 * one thing a download manager must never produce.
                 *
                 * Skipped for a probe body: the probe asked for bytes=0- (the
                 * whole file) and the server is legitimately sending all of it;
                 * the clipping below stops at this segment's boundary. */
                self.file.flush().await?;
                put(&mut self.ledger, self.written).await?;
                return Err(Error::DishonestRanges);
            }

            let (cut, chunk) = self.put_chunk(chunk, plan).await?;
            if cut {
                job.downloaded
                    .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                // The rest belongs to the range split off; this one is whole.
                break;
            }
            self.pass_limit(&chunk).await?;
        }
        Ok(())
    }

    /// Paused: what has already come is taken and written before the range lets go, without
    /// waiting for more; then the ledger written down and the stop returned.
    async fn drain(&mut self) -> Result<(), Error> {
        let (job, index) = (self.job, self.index);
        /* Paused: what has already come, in the connection's buffers and the
         * system's, is taken and written before the range lets go, without
         * waiting for more. Dropped, it was fetched again at the resume: on a
         * fast link most of what a stop cost twice (gaps row 186). */
        if self.carry.is_none() {
            if let Some(stopped) = self.draining.take() {
                let ready = (self.drained < DRAIN_MOST)
                    .then(|| futures_util::FutureExt::now_or_never(self.stream.next()))
                    .flatten()
                    .flatten();
                match ready {
                    Some(Ok(more)) => {
                        self.received += more.len() as u64;
                        self.draining = Some(stopped);
                        self.carry = Some(more);
                    }
                    _ => {
                        self.file.flush().await?;
                        put(&mut self.ledger, self.written).await?;
                        // Cancelled meanwhile, it is a cancel.
                        let stopped = job.control.check().err().unwrap_or(stopped);
                        if matches!(stopped, Error::Paused) {
                            stopped_synced(job, index, self.file.get_ref(), self.written).await;
                            job.segments.lock().await[index].done = self.written.min(self.span_now);
                        }
                        return Err(stopped);
                    }
                }
            }
        }
        Ok(())
    }

    /// A connection far slower than the rest dropped, to be asked again.
    async fn judge_window(&mut self) -> Result<(), Error> {
        let (job, index) = (self.job, self.index);
        // The last one still fetching, quiet while the others ended: dropped and asked again
        // at once, rather than the others waiting out the window on it (gaps row K1).
        if job.attempts[index].load(Ordering::Relaxed) <= 2
            && super::helping::holds_the_end(self.quiet_since, job)
        {
            self.file.flush().await?;
            put(&mut self.ledger, self.written).await?;
            return Err(Error::Incomplete {
                got: self.written,
                expected: self.span_now,
            });
        }
        // A connection far slower than the rest is dropped and asked again,
        // rather than held to the read timeout while the others finish.
        if self.window.over() {
            // Only for the first tries of a range: one the server keeps
            // serving slowly is then waited out, not retried to a failure.
            if job.attempts[index].load(Ordering::Relaxed) <= 2
                && self.window.stalled(self.written, job)
            {
                self.file.flush().await?;
                put(&mut self.ledger, self.written).await?;
                return Err(Error::Incomplete {
                    got: self.written,
                    expected: self.span_now,
                });
            }
            self.window = Window::open(self.written, job);
        }
        Ok(())
    }

    /// The next chunk: one in hand, or the next to arrive, the ledger written down while the
    /// server is quiet.
    async fn next_chunk(
        &mut self,
    ) -> Result<ControlFlow<Flow, reqwest::Result<bytes::Bytes>>, Error> {
        let (job, index) = (self.job, self.index);
        let chunk = match self.carry.take() {
            Some(rest) => Ok(rest),
            None => {
                // While what came is not yet written down, no longer than the
                // ledger's quarter second: a server that goes quiet does not
                // keep what it sent in the write buffer, off the disk a preview
                // reads and past the ledger a crash goes back to.
                // Woken once it has been quiet long enough to be judged as holding the end,
                // then twice a second while it stays quiet, as the others may end meanwhile.
                let now = tokio::time::Instant::now();
                let judged_at = (self.quiet_since + super::helping::END_QUIET)
                    .max(now + super::helping::END_LOOK);
                let ends = self.window.ends().min(judged_at);
                let until = if self.written > self.recorded {
                    ends.min(tokio::time::Instant::from_std(
                        self.recorded_at + LEDGER_EVERY_TIME,
                    ))
                } else {
                    ends
                };
                match tokio::time::timeout_at(
                    until,
                    self.reader.next(&mut self.stream, &job.control),
                )
                .await
                {
                    // Quiet past the quarter: what came is written down.
                    Err(_)
                        if self.written > self.recorded
                            && self.recorded_at.elapsed() >= LEDGER_EVERY_TIME =>
                    {
                        self.file.flush().await?;
                        put(&mut self.ledger, self.written).await?;
                        self.recorded = self.written;
                        self.recorded_at = std::time::Instant::now();
                        return Ok(ControlFlow::Break(Flow::Again));
                    }
                    // The window ran out with nothing arriving: judged at the top.
                    Err(_) => return Ok(ControlFlow::Break(Flow::Again)),
                    Ok(Ok(Some(chunk))) => {
                        if let Ok(bytes) = &chunk {
                            self.received += bytes.len() as u64;
                        }
                        chunk
                    }
                    Ok(Ok(None)) => return Ok(ControlFlow::Break(Flow::Ended)),
                    // Paused while waiting: what has come is taken first (above).
                    Ok(Err(Error::Paused)) => {
                        self.draining = Some(Error::Paused);
                        return Ok(ControlFlow::Break(Flow::Again));
                    }
                    // Stopped while waiting: what is in the write buffer is on
                    // disk before the range lets go, or a resume fetches it again.
                    Ok(Err(stopped)) => {
                        self.file.flush().await?;
                        put(&mut self.ledger, self.written).await?;
                        if matches!(stopped, Error::Paused) {
                            stopped_synced(job, index, self.file.get_ref(), self.written).await;
                            job.segments.lock().await[index].done = self.written.min(self.span_now);
                        }
                        return Err(stopped);
                    }
                }
            }
        };
        Ok(ControlFlow::Continue(chunk))
    }

    /// The lead bytes kept aside for assembly's check, not written into this range.
    fn keep_lead(&mut self, chunk: &mut bytes::Bytes, lead_n: usize) {
        let (job, index) = (self.job, self.index);
        if self.want_lead && !chunk.is_empty() {
            let need = lead_n.saturating_sub(self.lead_buf.len());
            let take = need.min(chunk.len());
            self.lead_buf.extend_from_slice(&chunk[..take]);
            *chunk = chunk.slice(take..);
            if self.lead_buf.len() >= lead_n {
                if let Some(slot) = job.overlaps.get(index) {
                    if let Ok(mut guard) = slot.lock() {
                        *guard = Some(self.lead_buf.clone());
                    }
                }
                self.want_lead = false;
            }
        }
    }

    /// The chunk, where no pause or stop has come; a pause takes it, and what else has come,
    /// first (`None`), and a stop ends the attempt.
    async fn checked(&mut self, chunk: bytes::Bytes) -> Result<Option<bytes::Bytes>, Error> {
        let (job, index) = (self.job, self.index);
        /* Checked between chunks rather than mid-write, so what is flushed
         * and what the ledger records stay obviously in step. A pause takes
         * this chunk, and what else has come, first (`draining`). */
        if let (None, Err(Error::Paused)) = (&self.draining, job.control.check()) {
            self.draining = Some(Error::Paused);
            self.carry = Some(chunk);
            return Ok(None);
        }
        if let (None, Err(stopped)) = (&self.draining, job.control.check()) {
            // Flushed first either way: the bytes already written are what a
            // resume picks up from, and a pause also records where it got to.
            self.file.flush().await?;
            put(&mut self.ledger, self.written).await?;
            if matches!(stopped, Error::Paused) {
                stopped_synced(job, index, self.file.get_ref(), self.written).await;
                job.segments.lock().await[index].done = self.written.min(self.span_now);
            }
            return Err(stopped);
        }
        Ok(Some(chunk))
    }

    /// Writes the chunk, claimed against where the range ends now: whether it was cut at the
    /// range's end, and what was written. The disk looked at, and the ledger written down, as
    /// the bytes come.
    async fn put_chunk(
        &mut self,
        mut chunk: bytes::Bytes,
        plan: &Plan,
    ) -> Result<(bool, bytes::Bytes), Error> {
        let (job, index) = (self.job, self.index);
        let (asked_at, carried_on) = (plan.asked_at, plan.carried_on);
        // Claimed before it is written, against where the range ends now:
        // a split under this connection stops it at the new end.
        let arrived = tokio::time::Instant::now();
        let (fits, first) = {
            let mut l = crate::lock(&job.live[index]);
            self.span_now = (l.end - l.start).saturating_add(1);
            let fits = (chunk.len() as u64).min(self.span_now.saturating_sub(self.written));
            l.reserved = self.written + fits;
            let held = l.reserved;
            let first = l.going.came(arrived, held);
            (fits, first)
        };
        // Read on from the range before, its first bytes waited for nothing.
        if first && !carried_on {
            job.note_first_byte(arrived.saturating_duration_since(asked_at));
        }
        let cut = fits < chunk.len() as u64;
        if cut {
            // What is past the range's end: the next range's, where the answer goes on into it.
            self.onward = Some(chunk.slice(fits as usize..));
            chunk = chunk.slice(..fits as usize);
        }
        as_slow_as_the_disk(chunk.len() as u64).await;
        self.file.write_all(&chunk).await?;
        self.written += chunk.len() as u64;
        self.quiet_since = tokio::time::Instant::now();
        if self.draining.is_some() {
            self.drained += chunk.len() as u64;
        }
        self.flowing.get_or_insert_with(|| job.together.bringing());
        // With no length to have planned the room for, the disk is looked at as
        // the bytes come, and the download stopped before it fills it.
        if job.size.is_none() && self.written - self.disk_looked >= DISK_LOOK_EVERY {
            self.disk_looked = self.written;
            if job
                .room
                .free_space(&job.parts)
                .is_some_and(|free| free < DISK_FLOOR)
            {
                self.file.flush().await?;
                put(&mut self.ledger, self.written).await?;
                return Err(Error::Internal(format!(
                    "stopped: the disk has less than {} free and this download has no length to say when it ends",
                    amount(DISK_FLOOR)
                )));
            }
        }
        // Written down every half megabyte or quarter second: a crash then
        // costs no more than that of this range, fetched again.
        if self.written > self.recorded
            && (self.written - self.recorded >= LEDGER_EVERY
                || self.recorded_at.elapsed() >= LEDGER_EVERY_TIME)
        {
            self.file.flush().await?;
            put(&mut self.ledger, self.written).await?;
            self.recorded = self.written;
            self.recorded_at = std::time::Instant::now();
        }
        Ok((cut, chunk))
    }

    /// The chunk let through the speed ceiling and counted; a stop meanwhile ends the attempt.
    async fn pass_limit(&mut self, chunk: &bytes::Bytes) -> Result<(), Error> {
        let (job, index) = (self.job, self.index);
        /* Applied after the bytes have arrived, because there is nowhere
         * earlier to stand: the only lever on an HTTP body is how fast it is
         * read, and a reader that pauses is a sender that stops.
         * Counted as the ceiling lets it through rather than as it arrived:
         * counted on arrival, every connection's chunk was in the count a
         * whole wait early, and the shown speed and ETA ran at twice the
         * limit for the first seconds. */
        let mut through = 0;
        let stopping = if self.draining.is_some() {
            // Taken while stopping: no ceiling to wait for, only counted.
            through = chunk.len() as u64;
            job.downloaded.fetch_add(through, Ordering::Relaxed);
            Ok(())
        } else {
            or_stop(&job.control, async {
                job.limiter
                    .take_counting(chunk.len() as u64, &job.downloaded, &mut through)
                    .await;
                // And the second network's own limit, where this connection goes over it
                // (gaps row K5).
                if let Some(network) = crate::second_path::task_limit() {
                    network.take(chunk.len() as u64).await;
                }
                // And counted to that network's own allowance (gaps row K5c).
                crate::network_allowance::count(
                    crate::second_path::task_network(),
                    chunk.len() as u64,
                    std::time::SystemTime::now(),
                );
                Ok(())
            })
            .await
        };
        if let Err(stopped) = stopping {
            // It is on disk: the count is a ledger of what is, and a retry
            // of this range starts from what its ledger records.
            job.downloaded
                .fetch_add(chunk.len() as u64 - through, Ordering::Relaxed);
            self.file.flush().await?;
            put(&mut self.ledger, self.written).await?;
            if matches!(stopped, Error::Paused) {
                stopped_synced(job, index, self.file.get_ref(), self.written).await;
                job.segments.lock().await[index].done = self.written.min(self.span_now);
            }
            return Err(stopped);
        }
        Ok(())
    }

    /// Counts what the attempt read and did not write, less what it hands on to the range after
    /// it (`handed_on`), which that range writes (`unwritten`, gaps row 186).
    pub(super) fn count_unwritten(&self, plan: &Plan, handed_on: u64) {
        let written = self.written.saturating_sub(plan.already);
        let left = crate::download::unwritten::left_unwritten(self.received, written, handed_on);
        crate::download::unwritten::count(self.index, left);
    }

    /// The range's end, as `end` makes it, and what the attempt read and did not write
    /// counted, whatever the end: what came past the range's end is counted unless it is read on
    /// into the range after it.
    pub(super) async fn finish(
        self,
        plan: &Plan,
        resumable: bool,
    ) -> Result<Option<(usize, Carried)>, Error> {
        let (index, received) = (self.index, self.received);
        let written = self.written.saturating_sub(plan.already);
        let ended = self.end(plan, resumable).await;
        let handed_on = match &ended {
            Ok(Some((_, carried))) => carried.rest.as_ref().map_or(0, |rest| rest.len() as u64),
            _ => 0,
        };
        let left = crate::download::unwritten::left_unwritten(received, written, handed_on);
        crate::download::unwritten::count(index, left);
        ended
    }

    /// The range's end: the ledger and the plan written down, and what the attempt came to.
    async fn end(
        mut self,
        plan: &Plan,
        resumable: bool,
    ) -> Result<Option<(usize, Carried)>, Error> {
        let (job, index) = (self.job, self.index);
        let (start, end, asked_end, reach) = (plan.start, plan.end, plan.asked_end, plan.reach);
        self.file.flush().await?;
        put(&mut self.ledger, self.written).await?;
        // A body that stopped being undone is the download's failure, not a
        // short file that ended.
        let why = self.stopped.lock().ok().and_then(|slot| slot.clone());
        if let Some(why) = why {
            return Err(Error::Internal(why));
        }

        /* The shared plan is touched once, at the end, rather than per chunk.
         *
         * One mutex guards every range, so a lock per chunk is every connection
         * queueing behind every other a hundred thousand times over a large
         * download — to publish a number nothing reads while it runs. */
        {
            // Read under the segment lock, which a split holds from shortening
            // this range to recording the plan: a split whose plan could not be
            // written gives the range its end back before letting go, and the
            // end read here is the one that stands.
            let mut segments = job.segments.lock().await;
            let l = crate::lock(&job.live[index]);
            self.span_now = (l.end - l.start).saturating_add(1);
            drop(l);
            if let Some(segment) = segments.get_mut(index) {
                segment.done = self.written.min(self.span_now);
            }
        }

        /* The whole file, asked for without a range, ended cleanly: that is its
         * end, whatever the plan guessed. An unknown length is planned to
         * `u64::MAX` and an empty file as one byte, and without this both would
         * sit through every retry as "short". A length the response stated is still
         * held to, and assembly holds the total to the probed one. */
        if !resumable && self.stated.is_none_or(|stated| stated == self.written) {
            return Ok(None);
        }

        // A slice that came whole is not a short range: the next is asked for.
        if asked_end < end && self.written < self.span_now && start + self.written > asked_end {
            return Ok(None);
        }

        // A stream that ended early without erroring is not a finished range.
        if self.written < self.span_now {
            return Err(Error::Incomplete {
                got: self.written,
                expected: self.span_now,
            });
        }

        /* Whole, with the answer going on past it: where the server limits this download, read on
         * into the range after it where nobody has that one, rather than asked for again. Only from
         * the range's very end (a range halved under a connection already past the new end stops
         * there), and never through a decoder or under a cap. */
        if let Some(rest) = self.onward {
            let at = start + self.written;
            let may = job.limits.limited()
                && resumable
                && job.chunk.is_none()
                && job.size.is_some()
                && !self.decoded
                && self.written == self.span_now;
            if may {
                if let Some(next) = continue_into(job, index, at, reach).await {
                    return Ok(Some((
                        next,
                        Carried {
                            stream: self.stream,
                            rest: Some(rest).filter(|rest| !rest.is_empty()),
                            reach,
                            out: self.out,
                        },
                    )));
                }
            }
        }
        Ok(None)
    }
}
