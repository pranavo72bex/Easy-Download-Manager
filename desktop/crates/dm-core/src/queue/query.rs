use super::*;

impl Queue {
    /// The rows a range is being retried for, now: each with the attempt
    /// it is on (the first try is 1) and how many there may be, 0 for no
    /// end, and what failed, in the engine's words. Over once bytes arrive
    /// again.
    pub async fn retrying(&self) -> HashMap<Id, (u32, u32, String, std::time::Instant)> {
        self.inner
            .slots
            .lock()
            .await
            .iter()
            .filter(|(_, s)| s.entry.state == State::Running)
            .filter_map(|(id, s)| s.retrying.clone().map(|r| (*id, r)))
            .collect()
    }

    /// Every row, in the order they were added.
    pub async fn list(&self) -> Vec<Entry> {
        let slots = self.inner.slots.lock().await;
        let order = self.inner.order.lock().await;
        order
            .iter()
            .filter_map(|id| slots.get(id).map(|s| Entry::clone(&s.entry)))
            .collect()
    }

    /// The rows in order, shared rather than copied: a row unchanged since
    /// the last read is the same allocation (`Arc::ptr_eq`), so a reader
    /// can keep what it worked out from it. What the window reads four
    /// times a second while anything moves.
    pub async fn list_shared(&self) -> Vec<Arc<Entry>> {
        let slots = self.inner.slots.lock().await;
        let order = self.inner.order.lock().await;
        order
            .iter()
            .filter_map(|id| slots.get(id).map(|s| s.entry.clone()))
            .collect()
    }

    pub async fn get(&self, id: Id) -> Option<Entry> {
        self.inner
            .slots
            .lock()
            .await
            .get(&id)
            .map(|s| Entry::clone(&s.entry))
    }
}
