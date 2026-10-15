use std::{collections::HashMap, hash::Hash, sync::Mutex};
use tokio::sync::watch;

#[cfg(test)]
mod tests;

/// Coalesces concurrent in-flight calls for the same key so only one execution
/// runs at a time.
pub(crate) struct Singleflight<Key, Value> {
    in_flight: Mutex<HashMap<Key, watch::Receiver<Option<Value>>>>,
}

impl<Key, Value> Default for Singleflight<Key, Value>
where
    Key: Eq + Hash,
{
    fn default() -> Self {
        Self { in_flight: Mutex::new(HashMap::new()) }
    }
}

enum FlightRole<Value> {
    Leader(watch::Sender<Option<Value>>),
    Follower(watch::Receiver<Option<Value>>),
}

struct LeaderGuard<'a, Key: Eq + Hash, Value> {
    singleflight: &'a Singleflight<Key, Value>,
    key: &'a Key,
    completed: bool,
}

impl<Key: Eq + Hash, Value> Drop for LeaderGuard<'_, Key, Value> {
    fn drop(&mut self) {
        if !self.completed {
            let mut map = self.singleflight.in_flight.lock().unwrap();
            map.remove(self.key);
        }
    }
}

impl<Key, Value> Singleflight<Key, Value>
where
    Key: Clone + Eq + Hash + Send + Sync,
    Value: Clone + Send + Sync,
{
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn acquire_role(&self, key: &Key) -> FlightRole<Value> {
        let mut map = self.in_flight.lock().unwrap();
        if let Some(existing) = map.get(key) {
            FlightRole::Follower(existing.clone())
        } else {
            let (sender, receiver) = watch::channel(None);
            map.insert(key.clone(), receiver);
            FlightRole::Leader(sender)
        }
    }

    async fn run_leader<FutureOutput>(
        &self,
        key: &Key,
        sender: watch::Sender<Option<Value>>,
        future: FutureOutput,
    ) -> Value
    where
        FutureOutput: std::future::Future<Output = Value> + Send,
    {
        let mut guard = LeaderGuard { singleflight: self, key, completed: false };
        let result = future.await;
        guard.completed = true;

        {
            let mut map = self.in_flight.lock().unwrap();
            map.remove(key);
        }

        let _ = sender.send(Some(result.clone()));
        result
    }

    async fn await_follower(mut receiver: watch::Receiver<Option<Value>>) -> Option<Value> {
        while receiver.changed().await.is_ok() {
            let borrowed = receiver.borrow();
            if let Some(ref val) = *borrowed {
                return Some(val.clone());
            }
        }
        None
    }

    /// Execute `make_future` if no other caller is currently executing for `key`.
    /// If an execution is already in flight for `key`, wait for it and return
    /// its result instead of executing again.
    pub(crate) async fn work<Factory, FutureOutput>(&self, key: Key, make_future: Factory) -> Value
    where
        Factory: FnOnce() -> FutureOutput + Send,
        FutureOutput: std::future::Future<Output = Value> + Send,
    {
        let mut make_future_opt = Some(make_future);
        loop {
            match self.acquire_role(&key) {
                FlightRole::Leader(sender) => {
                    let future = make_future_opt
                        .take()
                        .expect("make_future only executed when becoming leader")(
                    );
                    return self.run_leader(&key, sender, future).await;
                }
                FlightRole::Follower(receiver) => {
                    if let Some(result) = Self::await_follower(receiver).await {
                        return result;
                    }
                }
            }
        }
    }
}
