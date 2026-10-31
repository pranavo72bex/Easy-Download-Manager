use super::*;

impl Queue {
    /// What to call whenever the queue changes. One at a time: a second
    /// replaces the first.
    pub fn on_change(&self, tell: impl Fn() + Send + Sync + 'static) {
        *crate::lock(&self.inner.changed) = Some(Arc::new(tell));
    }

    /// How many may run at once.
    pub fn concurrency(&self) -> usize {
        self.inner.concurrency.load(Ordering::Relaxed)
    }

    /// Changes how many may run at once.
    ///
    /// Lowering it does not stop anything already running — a setting that
    /// killed transfers to obey itself would lose bytes to a preference.
    /// The excess drains as those finish.
    /// How many files each named queue runs at once, where fewer than the
    /// whole queue's limit; unnamed queues run up to that limit.
    /// Named queues to start on their own each day, at a minute past local
    /// midnight. Which already started today is kept across a change.
    pub fn set_queue_starts(&self, starts: Vec<(String, u32)>) {
        let mut held = crate::lock(&self.inner.starts);
        held.at = starts
            .into_iter()
            .filter(|(name, at)| !name.trim().is_empty() && *at < 24 * 60)
            .collect();
    }

    /// The queues whose start time has come on `day` (any count of days)
    /// at `minute` past midnight, each once a day: within five minutes of
    /// its time, so a loop that slept a little late still starts it, and
    /// a computer that slept through it does not start it hours after.
    pub fn starts_due(&self, day: i64, minute: u32) -> Vec<String> {
        let mut held = crate::lock(&self.inner.starts);
        let QueueStarts { at: starts, fired } = &mut *held;
        let mut due = Vec::new();
        for (queue, at) in starts.iter() {
            let near = minute >= *at && minute < at + 5;
            if near && fired.get(queue) != Some(&day) {
                fired.insert(queue.clone(), day);
                due.push(queue.clone());
            }
        }
        due
    }

    pub fn set_queue_limits(&self, limits: HashMap<String, usize>) {
        *crate::lock(&self.inner.queue_limits) = limits
            .into_iter()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, n)| (name, n.max(1)))
            .collect();
    }

    pub fn queue_limits(&self) -> HashMap<String, usize> {
        crate::lock(&self.inner.queue_limits).clone()
    }

    pub fn set_concurrency(&self, limit: usize) {
        self.inner
            .concurrency
            .store(limit.max(1), Ordering::Relaxed);
    }

    /// The options the next download to start will use.
    pub async fn options(&self) -> Options {
        self.inner.options.lock().await.clone()
    }

    /// Replaces the options the next download will start with.
    ///
    /// The limiter is deliberately not replaced: it is shared with every
    /// download already running, and swapping the handle would leave those
    /// drawing on a budget nothing else could see. Use `set_speed_limit`.
    pub async fn set_options(&self, options: Options) {
        let mut current = self.inner.options.lock().await;
        let limiter = current.limiter.clone();
        // Nor is the ledger of disk space promised to downloads under way.
        let room = current.room.clone();
        *current = Options {
            limiter,
            room,
            ..options
        };
    }

    /// The shared ceiling, in bytes per second. Zero means none.
    pub async fn speed_limit(&self) -> u64 {
        self.inner.options.lock().await.limiter.rate().await
    }

    /// Applies a ceiling to everything, including transfers already running.
    pub async fn set_speed_limit(&self, bytes_per_second: u64) {
        let limiter = self.inner.options.lock().await.limiter.clone();
        limiter.set_rate(bytes_per_second).await;
    }

    pub fn schedule(&self) -> Schedule {
        crate::lock(&self.inner.schedule).clone()
    }

    /// Whether the queue may run now.
    ///
    /// Asked on every tick, so it reads the window in place rather than
    /// cloning one — the clone was two heap strings and a vector, twenty
    /// times a second, to answer a question about the clock.
    pub(super) fn window_open(&self) -> bool {
        crate::lock(&self.inner.schedule).open()
    }

    pub fn set_schedule(&self, schedule: Schedule) {
        *crate::lock(&self.inner.schedule) = schedule;
    }

    /// Caps how much is downloaded per period; zero bytes is no cap.
    pub fn set_allowance(&self, allowance: crate::quota::Allowance) {
        crate::lock(&self.inner.quota).set(allowance);
    }

    /// Whether the schedule and the download limit both let downloads run.
    pub fn may_run(&self) -> bool {
        self.window_open() && self.allowance_spent().is_none()
    }

    /// Whether any row is held by the schedule or the download limit, and
    /// so waits for one of them to let it go rather than for a person.
    pub async fn held(&self) -> bool {
        self.inner
            .slots
            .lock()
            .await
            .values()
            .any(|s| s.entry.paused_by_schedule && s.entry.state == State::Paused)
    }

    /// How long until downloading goes on, while the allowance is spent.
    pub fn allowance_spent(&self) -> Option<Duration> {
        crate::lock(&self.inner.quota).spent(std::time::SystemTime::now())
    }

    /// Sends downloads through this proxy, giving up on a connection that
    /// sends nothing for `read_timeout`, from now on. Running downloads keep
    /// the client they started with.
    pub async fn set_client(
        &self,
        proxy: &crate::proxy::Proxy,
        read_timeout: Duration,
    ) -> Result<(), String> {
        let client = crate::proxy::client_with_timeout(proxy, read_timeout)?;
        *self.inner.client.lock().await = client;
        Ok(())
    }

    /// One download's own speed limit, in KB/s (zero for none), in force at
    /// once if it is running.
    pub async fn set_own_limit(&self, id: Id, kb_per_second: u64) {
        let limiter = {
            let mut slots = self.inner.slots.lock().await;
            let Some(slot) = slots.get_mut(&id) else {
                return;
            };
            slot.entry_mut().speed_limit = kb_per_second;
            slot.own_limit.clone()
        };
        limiter.set_rate(kb_per_second.saturating_mul(1024)).await;
        self.save().await;
    }
}
