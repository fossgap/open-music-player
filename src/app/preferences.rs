use super::*;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const APPEARANCE: &str = "org.freedesktop.appearance";

impl App {
    pub(super) fn enter_background(&self) {
        if self.background_hold.borrow().is_none() {
            *self.background_hold.borrow_mut() = Some(self.application.hold());
        }
        self.window.hide();
        object::<gtk::Window>(&self.mini_ui, "mini_window").hide();
    }

    pub(super) fn apply_theme(&self) {
        let mode = self.state.borrow().theme;
        self.theme.borrow_mut().select(mode);
        if mode == model::Theme::System {
            self.watermark.clear();
            self.window.remove_css_class("artwork-watermark");
        }
        self.update_music_theme();
        for id in ["spectrum", "empty_art", "detail_backdrop"] {
            object::<gtk::DrawingArea>(&self.ui, id).queue_draw();
        }
    }

    pub(super) fn preference_settings(self: &Rc<Self>, ui: &gtk::Builder) {
        for (id, label) in [
            ("theme", "Theme"),
            ("start_in_background", "Start in background"),
            ("background", "Keep running when the window closes"),
        ] {
            object::<gtk::Widget>(ui, id)
                .update_property(&[gtk::accessible::Property::Label(&self.tr(label))]);
        }
        let theme: gtk::DropDown = object(ui, "theme");
        theme.set_model(Some(&gtk::StringList::new(&[
            &self.tr("System"),
            &self.tr("Sage"),
        ])));
        theme.set_selected(u32::from(self.state.borrow().theme == model::Theme::Sage));
        let weak = Rc::downgrade(self);
        theme.connect_selected_notify(move |picker| {
            if let Some(app) = weak.upgrade() {
                app.state.borrow_mut().theme = if picker.selected() == 1 {
                    model::Theme::Sage
                } else {
                    model::Theme::System
                };
                app.apply_theme();
                app.persist();
            }
        });
        let start: gtk::Switch = object(ui, "start_in_background");
        start.set_active(self.state.borrow().start_in_background);
        let weak = Rc::downgrade(self);
        start.connect_active_notify(move |switch| {
            if let Some(app) = weak.upgrade() {
                app.state.borrow_mut().start_in_background = switch.is_active();
                app.persist();
            }
        });
    }

    pub(super) fn setup_system_appearance(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Ok(connection) = gio::bus_get_future(gio::BusType::Session).await else {
                return;
            };
            let observed = Rc::new(Cell::new(false));
            let changed = observed.clone();
            let subscriber = weak.clone();
            let subscription = connection.subscribe_to_signal(
                Some(PORTAL),
                Some(SETTINGS),
                Some("SettingChanged"),
                Some(PORTAL_PATH),
                Some(APPEARANCE),
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if let Some((_, key, value)) =
                        signal.parameters.get::<(String, String, glib::Variant)>()
                        && key == "color-scheme"
                        && let Some(value) = value.get::<u32>()
                    {
                        changed.set(true);
                        if let Some(app) = subscriber.upgrade() {
                            app.theme.borrow_mut().system_scheme(value);
                        }
                    }
                },
            );
            let Some(app) = weak.upgrade() else { return };
            app.theme.borrow_mut().portal = Some(subscription);
            drop(app);
            // ReadAll works with both portal versions; never block GTK waiting for a portal.
            let result = connection
                .call_future(
                    Some(PORTAL),
                    PORTAL_PATH,
                    SETTINGS,
                    "ReadAll",
                    Some(&(vec![APPEARANCE],).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    2000,
                )
                .await;
            if !observed.get()
                && let (Some(app), Ok(reply)) = (weak.upgrade(), result)
            {
                type Namespaces = HashMap<String, HashMap<String, glib::Variant>>;
                if let Some((values,)) = reply.get::<(Namespaces,)>()
                    && let Some(value) = values
                        .get(APPEARANCE)
                        .and_then(|settings| settings.get("color-scheme"))
                        .and_then(|value| value.get::<u32>())
                {
                    app.theme.borrow_mut().system_scheme(value);
                }
            }
        });
    }
}
