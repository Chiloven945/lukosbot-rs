use std::{
    collections::hash_map::DefaultHasher,
    future::Future,
    hash::{Hash, Hasher},
};
use tokio::sync::mpsc;

type Job = Box<dyn FnOnce() -> tokio::task::JoinHandle<()> + Send + 'static>;

pub struct StripedExecutor {
    lanes: Vec<mpsc::UnboundedSender<Job>>,
}

impl StripedExecutor {
    pub fn new(stripes: usize) -> Self {
        let mut lanes = Vec::with_capacity(stripes);
        for _ in 0..stripes {
            let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
            tokio::spawn(async move {
                while let Some(job) = rx.recv().await {
                    let jh = job();
                    let _ = jh.await;
                }
            });
            lanes.push(tx);
        }
        Self { lanes }
    }

    pub fn submit<K: Hash>(&self, key: &K, fut: impl Future<Output = ()> + Send + 'static) {
        let idx = {
            let mut h = DefaultHasher::new();
            key.hash(&mut h);
            (h.finish() as usize) % self.lanes.len()
        };
        let _ = self.lanes[idx].send(Box::new(move || tokio::spawn(fut)));
    }
}
