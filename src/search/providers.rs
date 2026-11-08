use super::Group;
use crate::{
    model::{Track, YoutubeAccount},
    services::{self, PodcastDirectory, PodcastShow},
    youtube_music,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

#[derive(Debug)]
pub enum Payload {
    Tracks(Vec<Track>),
    Shows(Vec<PodcastShow>),
}
pub struct Reply {
    pub generation: u64,
    pub group: Group,
    pub query: String,
    pub result: Result<Payload, String>,
}
struct Job {
    generation: u64,
    query: String,
    account: Option<YoutubeAccount>,
    cache: PathBuf,
}

pub struct Service {
    workers: BTreeMap<Group, mpsc::Sender<Job>>,
    replies: mpsc::Receiver<Reply>,
    pub epoch: Arc<AtomicU64>,
}

impl Default for Service {
    fn default() -> Self {
        let epoch = Arc::new(AtomicU64::new(0));
        let (replies, rx) = mpsc::channel();
        let mut workers = BTreeMap::new();
        for group in [
            Group::Apple,
            Group::Gpodder,
            Group::Radio,
            Group::Youtube,
            Group::Feed,
        ] {
            let (tx, jobs) = mpsc::channel::<Job>();
            workers.insert(group, tx);
            let epoch = epoch.clone();
            let replies = replies.clone();
            std::thread::spawn(move || {
                while let Ok(mut job) = jobs.recv() {
                    // Keep at most one request in flight and one latest pending request per provider.
                    while let Ok(newer) = jobs.try_recv() {
                        job = newer;
                    }
                    let cancelled = || epoch.load(Ordering::Relaxed) != job.generation;
                    if cancelled() {
                        continue;
                    }
                    let result = match group {
                        Group::Apple => {
                            services::podcast_search(PodcastDirectory::Apple, &job.query)
                                .map(Payload::Shows)
                        }
                        Group::Gpodder => {
                            services::podcast_search(PodcastDirectory::Gpodder, &job.query)
                                .map(Payload::Shows)
                        }
                        Group::Radio => {
                            services::radio_cancellable(&job.query, &job.cache, cancelled)
                                .map(Payload::Tracks)
                        }
                        Group::Youtube => youtube_music::search_cancellable(
                            job.account.as_ref(),
                            &job.query,
                            cancelled,
                        )
                        .map(Payload::Tracks),
                        Group::Feed => {
                            services::podcast(&job.query).map(|(_, tracks)| Payload::Tracks(tracks))
                        }
                        _ => unreachable!(),
                    };
                    if !cancelled() {
                        let _ = replies.send(Reply {
                            generation: job.generation,
                            group,
                            query: job.query,
                            result,
                        });
                    }
                }
            });
        }
        Self {
            workers,
            replies: rx,
            epoch,
        }
    }
}

impl Service {
    pub fn generation(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }
    pub fn cancel(&self) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }
    pub fn search(
        &self,
        group: Group,
        query: String,
        account: Option<YoutubeAccount>,
        cache: PathBuf,
    ) {
        if let Some(worker) = self.workers.get(&group) {
            let _ = worker.send(Job {
                generation: self.generation(),
                query,
                account,
                cache,
            });
        }
    }
    pub fn poll(&self) -> Vec<Reply> {
        self.replies
            .try_iter()
            .filter(|r| r.generation == self.generation())
            .collect()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.cancel();
    }
}
