use super::*;

impl App {
    pub(super) fn setup_window_state(self: &Rc<Self>) {
        let saved = self.state.borrow().window;
        self.window.set_default_size(saved.width, saved.height);
        if saved.maximized {
            self.window.maximize();
        }
        for property in [
            "default-width",
            "default-height",
            "maximized",
            "fullscreened",
        ] {
            let weak = Rc::downgrade(self);
            self.window
                .connect_notify_local(Some(property), move |window, _| {
                    let Some(app) = weak.upgrade() else { return };
                    // GTK keeps the normal size here while maximized/fullscreen, and
                    // accounts for decorations. Allocated widget sizes drift on restart.
                    let (width, height) = window.default_size();
                    let mut state = app.state.borrow_mut();
                    if width > 0 {
                        state.window.width = width;
                    }
                    if height > 0 {
                        state.window.height = height;
                    }
                    // A hidden startup has not yet received a window-manager state.
                    if window.is_mapped() && !window.is_fullscreen() {
                        state.window.maximized = window.is_maximized();
                    }
                    // Cache changes in memory. Normal persistence/close/quit saves them
                    // without serializing the library for every pixel of a resize.
                });
        }
    }
}
