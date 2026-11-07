use super::*;
use gtk::gdk_pixbuf::{InterpType, Pixbuf};
use std::collections::VecDeque;

#[derive(Clone)]
pub(super) struct Cover {
    texture: gdk::Texture,
    pub backdrop: Option<Pixbuf>,
    watermark: Option<Pixbuf>,
    pub color: (f64, f64, f64),
}

struct Binding {
    picture: glib::WeakRef<gtk::Picture>,
    path: String,
}

#[derive(Default)]
struct Cache {
    images: HashMap<String, (Cover, u64)>,
    bindings: HashMap<usize, Binding>,
    pending: HashSet<String>,
    queue: VecDeque<String>,
    active: usize,
    clock: u64,
    changed: bool,
    pinned: Option<String>,
}

/// Decode a few covers at a time without blocking GTK or rebinding the list.
#[derive(Clone, Default)]
pub(super) struct Images(Rc<RefCell<Cache>>);

impl Images {
    pub fn set(&self, picture: &gtk::Picture, path: Option<&str>) {
        let id = picture.as_ptr() as usize;
        let path = path.filter(|p| !p.starts_with("http://") && !p.starts_with("https://"));
        let mut cache = self.0.borrow_mut();
        let Some(path) = path else {
            cache.bindings.remove(&id);
            drop(cache);
            picture.set_paintable(None::<&gdk::Paintable>);
            return;
        };
        if cache
            .bindings
            .get(&id)
            .is_some_and(|b| b.path == path && b.picture.upgrade().as_ref() == Some(picture))
        {
            return;
        }
        cache.bindings.insert(
            id,
            Binding {
                picture: picture.downgrade(),
                path: path.into(),
            },
        );
        cache.clock += 1;
        let clock = cache.clock;
        if let Some((cover, used)) = cache.images.get_mut(path) {
            *used = clock;
            let texture = cover.texture.clone();
            drop(cache);
            picture.set_paintable(Some(&texture));
            return;
        }
        if cache.pending.insert(path.into()) {
            cache.queue.push_back(path.into());
        }
        drop(cache);
        picture.set_paintable(None::<&gdk::Paintable>);
        self.start_next();
    }

    fn start_next(&self) {
        loop {
            let mut cache = self.0.borrow_mut();
            cache.bindings.retain(|_, b| b.picture.upgrade().is_some());
            if cache.active >= 4 {
                return;
            }
            let Some(path) = cache.queue.pop_front() else {
                return;
            };
            if !cache.bindings.values().any(|b| b.path == path) {
                cache.pending.remove(&path);
                continue;
            }
            cache.active += 1;
            drop(cache);
            let images = self.clone();
            glib::spawn_future_local(async move {
                let pixbuf = async {
                    let stream = gio::File::for_path(&path)
                        .read_future(glib::Priority::DEFAULT_IDLE)
                        .await
                        .ok()?;
                    Pixbuf::from_stream_at_scale_future(&stream, 400, 400, true)
                        .await
                        .ok()
                }
                .await;
                images.complete(path, pixbuf);
            });
        }
    }

    fn complete(&self, path: String, pixbuf: Option<Pixbuf>) {
        let mut cache = self.0.borrow_mut();
        cache.active -= 1;
        cache.pending.remove(&path);
        if let Some(pixbuf) = pixbuf {
            let color = pixbuf
                .scale_simple(1, 1, InterpType::Bilinear)
                .map(|p| {
                    let bytes = p.read_pixel_bytes();
                    let bytes = bytes.as_ref();
                    (
                        f64::from(bytes[0]) / 255.0,
                        f64::from(bytes[1]) / 255.0,
                        f64::from(bytes[2]) / 255.0,
                    )
                })
                .unwrap_or((0.17, 0.23, 0.17));
            let cover = Cover {
                texture: gdk::Texture::for_pixbuf(&pixbuf),
                backdrop: pixbuf.scale_simple(16, 16, InterpType::Bilinear),
                // A small cached image softens fine detail when enlarged as a watermark.
                // Preserve aspect ratio; decoding and resizing never happen during drawing.
                watermark: {
                    let scale = 64.0 / f64::from(pixbuf.width().max(pixbuf.height()));
                    pixbuf.scale_simple(
                        (f64::from(pixbuf.width()) * scale).round().max(1.0) as i32,
                        (f64::from(pixbuf.height()) * scale).round().max(1.0) as i32,
                        InterpType::Bilinear,
                    )
                },
                color,
            };
            if cache.images.len() >= 256 {
                if let Some(oldest) = cache
                    .images
                    .iter()
                    .filter(|(key, _)| Some(*key) != cache.pinned.as_ref())
                    .min_by_key(|(_, (_, used))| used)
                    .map(|(key, _)| key.clone())
                {
                    cache.images.remove(&oldest);
                }
            }
            cache.clock += 1;
            let clock = cache.clock;
            let pictures: Vec<_> = cache
                .bindings
                .values()
                .filter(|b| b.path == path)
                .filter_map(|b| b.picture.upgrade())
                .collect();
            let texture = cover.texture.clone();
            cache.images.insert(path, (cover, clock));
            cache.changed = true;
            drop(cache);
            for picture in pictures {
                picture.set_paintable(Some(&texture));
            }
        } else {
            // Allow a later explicit refresh to retry an unreadable file.
            cache.bindings.retain(|_, b| b.path != path);
            drop(cache);
        }
        self.start_next();
    }

    pub fn backdrop(&self, path: Option<&str>) -> Option<Cover> {
        let mut cache = self.0.borrow_mut();
        cache.pinned = path.map(str::to_string);
        path.and_then(|p| cache.images.get(p))
            .map(|(cover, _)| cover.clone())
    }

    pub fn take_changed(&self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().changed)
    }
}

impl App {
    pub(super) fn update_backdrop(&self) {
        let state = self.state.borrow();
        let cover = self
            .images
            .backdrop(state.current_track().and_then(|t| t.artwork.as_deref()));
        *self.backdrop_art.borrow_mut() = cover.as_ref().and_then(|c| c.backdrop.clone());
        self.watermark
            .set_artwork(cover.as_ref().and_then(|c| c.watermark.clone()));
        self.backdrop
            .set(cover.map(|c| c.color).unwrap_or((0.17, 0.23, 0.17)));
        object::<gtk::DrawingArea>(&self.ui, "detail_backdrop").queue_draw();
        self.update_visualization_art();
    }
}
