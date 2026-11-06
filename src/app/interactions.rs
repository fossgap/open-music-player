use super::*;

impl App {
    pub(super) fn set_buffering(&self, percent: i32) {
        self.buffering_percent.set(percent);
        if percent < 100 {
            if self.buffering_since.get().is_none() {
                self.buffering_since.set(Some(Instant::now()));
            }
        } else {
            self.buffering_since.set(None);
            if self.buffering_notice.replace(false) {
                object::<gtk::Box>(&self.ui, "notice").set_visible(false);
            }
        }
    }

    pub(super) fn update_buffering_notice(&self) {
        if !self.audio.borrow().loaded {
            self.set_buffering(100);
            return;
        }
        // Short rebuffering bursts during seeks must not resize the browse view.
        if self
            .buffering_since
            .get()
            .is_some_and(|since| since.elapsed() >= Duration::from_millis(500))
            && (self.buffering_notice.get() || !object::<gtk::Box>(&self.ui, "notice").is_visible())
        {
            self.notice(
                &format!(
                    "{} {}%",
                    self.tr("Buffering…"),
                    self.buffering_percent.get()
                ),
                false,
                true,
            );
            self.buffering_notice.set(true);
        }
    }

    pub(super) fn setup_wheel_controls(self: &Rc<Self>) {
        for (id, volume) in [("volume_box", true), ("seek", false)] {
            let controller = gtk::EventControllerScroll::new(
                gtk::EventControllerScrollFlags::BOTH_AXES
                    | gtk::EventControllerScrollFlags::DISCRETE,
            );
            controller.set_name(Some(if volume { "volume-wheel" } else { "seek-wheel" }));
            controller.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = Rc::downgrade(self);
            controller.connect_scroll(move |_, dx, dy| {
                let Some(app) = weak.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if volume {
                    let value = app.state.borrow().volume;
                    if let Some(target) = wheel_target(value, 1.0, dx, dy, 0.05) {
                        app.set_volume(target);
                    }
                } else {
                    let seek: gtk::Scale = object(&app.ui, "seek");
                    if !seek.is_sensitive() || !app.audio.borrow().loaded {
                        return glib::Propagation::Proceed;
                    }
                    if let Some(target) =
                        wheel_target(seek.value(), seek.adjustment().upper(), dx, dy, 5.0)
                    {
                        app.seek(target);
                        seek.set_value(target);
                    }
                }
                glib::Propagation::Stop
            });
            object::<gtk::Widget>(&self.ui, id).add_controller(controller);
        }
    }
}
fn wheel_target(current: f64, maximum: f64, dx: f64, dy: f64, step: f64) -> Option<f64> {
    if !dx.is_finite() || !dy.is_finite() || !current.is_finite() || !maximum.is_finite() {
        return None;
    }
    let delta = if dy.abs() >= dx.abs() { -dy } else { dx };
    if delta == 0.0 {
        return None;
    }
    Some((current + delta * step).clamp(0.0, maximum.max(0.0)))
}
#[cfg(test)]
mod tests {
    use super::wheel_target;
    #[test]
    fn wheel_directions_limits_and_invalid_input() {
        assert_eq!(wheel_target(0.5, 1.0, 0.0, -1.0, 0.05), Some(0.55));
        assert_eq!(wheel_target(20.0, 30.0, 1.0, 0.0, 5.0), Some(25.0));
        assert_eq!(wheel_target(29.0, 30.0, 0.0, -1.0, 5.0), Some(30.0));
        assert_eq!(wheel_target(1.0, 30.0, 0.0, 1.0, 5.0), Some(0.0));
        assert_eq!(wheel_target(1.0, 30.0, 0.0, f64::NAN, 5.0), None);
    }
}
