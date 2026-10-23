use super::*;

impl Queue {
    /// The rows a range is being retried for, now: each with the attempt
    /// it is on (the first try is 1) and how many there may be, 0 for no
    /// end, and what failed, in the engine's words. Over once bytes arrive
    /// again.
    pub async fn retrying(&self) -> HashMap<Id, (u32, u32, String)> {
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
            .filter_map(|id| slots.get(id).map(|s| s.entry.clone()))
            .collect()
    }

    pub async fn get(&self, id: Id) -> Option<Entry> {
        self.inner
            .slots
            .lock()
            .await
            .get(&id)
            .map(|s| s.entry.clone())
    }
}
