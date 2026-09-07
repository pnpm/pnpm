/// A fixed stripe set bounds lock memory while serializing writers for the
/// same logical resource. Hash collisions only reduce concurrency.
///
/// This guards concurrency **within one instance**. Across replicas sharing
/// one hosted store the same race is settled by the conditional write every
/// shared record is written under (S3 `If-Match` / `ETag`), which is what
/// makes the lock an optimization there rather than the guarantee.
pub(crate) struct StripedLocks {
    stripes: Box<[tokio::sync::Mutex<()>]>,
}

impl StripedLocks {
    /// Number of stripes. 64 keeps false sharing between distinct resources
    /// rare while staying tiny in memory.
    const STRIPES: usize = 64;

    pub(crate) fn new() -> Self {
        let stripes = (0..Self::STRIPES).map(|_| tokio::sync::Mutex::new(())).collect();
        Self { stripes }
    }

    /// Lock the stripe owning `name`, held until the returned guard is dropped.
    pub(crate) async fn lock(&self, name: &str) -> tokio::sync::MutexGuard<'_, ()> {
        self.stripes[self.stripe_index(name)].lock().await
    }

    /// Lock the stripes owning every name in `names`, held until the
    /// returned guards are dropped. Stripes are locked in ascending
    /// index order (duplicates collapsed), so two overlapping
    /// batch publishes — or a batch publish racing a single-package
    /// publish — can't deadlock on lock order.
    pub(crate) async fn lock_many(&self, names: &[&str]) -> Vec<tokio::sync::MutexGuard<'_, ()>> {
        let mut indices: Vec<usize> = names.iter().map(|name| self.stripe_index(name)).collect();
        indices.sort_unstable();
        indices.dedup();
        let mut guards = Vec::with_capacity(indices.len());
        for index in indices {
            guards.push(self.stripes[index].lock().await);
        }
        guards
    }

    pub(crate) fn stripe_index(&self, name: &str) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(name, &mut hasher);
        std::hash::Hasher::finish(&hasher) as usize % self.stripes.len()
    }
}
