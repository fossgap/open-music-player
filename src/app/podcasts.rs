use super::*;

#[derive(Default)]
pub(super) struct Controls {
    pending: RefCell<HashMap<String, u64>>,
    generation: Cell<u64>,
}
pub(super) struct FeedReply {
    pub url: String,
    pub generation: u64,
    pub subscribe: bool,
    pub quiet: bool,
    pub result: Result<(Subscription, Vec<Track>), String>,
}
impl App {
    pub(super) fn subscribe(&self, input: &str) {
        let mut url = match services::http_url(input) {
            Ok(url) => url,
            Err(error) => {
                self.notice(&error, true, false);
                return;
            }
        };
        url.set_fragment(None);
        self.request_feed(url.to_string(), true, false);
    }
    fn request_feed(&self, url: String, subscribe: bool, quiet: bool) {
        if self.podcasts.pending.borrow().contains_key(&url) {
            return;
        }
        let generation = self.podcasts.generation.get().wrapping_add(1);
        self.podcasts.generation.set(generation);
        self.podcasts
            .pending
            .borrow_mut()
            .insert(url.clone(), generation);
        if !quiet {
            self.notice(&self.tr("Loading podcast…"), false, true);
        }
        self.refresh();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = services::podcast(&url);
            let _ = tx.send(Message::Feed(FeedReply {
                url,
                generation,
                subscribe,
                quiet,
                result,
            }));
        });
    }
    pub(super) fn refresh_podcasts(&self, quiet: bool) {
        let subscriptions = self.state.borrow().subscriptions.clone();
        for sub in subscriptions {
            self.request_feed(sub.url, false, quiet);
        }
    }
    pub(super) fn podcast_pending(&self, url: &str) -> bool {
        self.podcasts.pending.borrow().contains_key(url)
    }
    pub(super) fn unsubscribe(&self, url: &str) {
        // Discard queued/in-flight replies so refresh cannot re-subscribe a removed feed.
        self.podcasts.pending.borrow_mut().remove(url);
        self.state
            .borrow_mut()
            .subscriptions
            .retain(|s| s.url != url);
        self.refresh();
        self.persist();
        self.notice(
            &self.tr("Unsubscribed. Existing episodes are kept."),
            false,
            false,
        );
    }
    pub(super) fn handle_feed(&self, reply: FeedReply) {
        if self.podcasts.pending.borrow().get(&reply.url) != Some(&reply.generation) {
            return;
        }
        self.podcasts.pending.borrow_mut().remove(&reply.url);
        if !reply.subscribe
            && !self
                .state
                .borrow()
                .subscriptions
                .iter()
                .any(|s| s.url == reply.url)
        {
            return;
        }
        match reply.result {
            Ok((sub, tracks)) => {
                let mut state = self.state.borrow_mut();
                if let Some(saved) = state.subscriptions.iter_mut().find(|s| s.url == reply.url) {
                    *saved = sub;
                } else {
                    state.subscriptions.push(sub);
                }
                state.merge(tracks);
                drop(state);
                if !reply.quiet {
                    self.notice(&self.tr("Podcast updated"), false, false);
                }
                self.refresh();
                self.ensure_artwork();
                self.persist();
            }
            Err(error) => {
                self.notice(&error, true, false);
                self.refresh();
            }
        }
    }
}
