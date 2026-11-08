//! A drawing area whose content is a retained GTK render tree.
//! Uses GTK 4.8 nodes, including the software renderer's fallback implementations.
use gtk::{glib, prelude::*, subclass::prelude::*};
use std::cell::RefCell;

type Draw = Box<dyn Fn(&gtk::Snapshot, i32, i32)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct RenderArea {
        pub draw: RefCell<Option<Draw>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RenderArea {
        const NAME: &'static str = "OapRenderArea";
        type Type = super::RenderArea;
        type ParentType = gtk::DrawingArea;
    }

    impl ObjectImpl for RenderArea {}
    impl DrawingAreaImpl for RenderArea {}
    impl WidgetImpl for RenderArea {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let area = self.obj();
            if area.width() > 0
                && area.height() > 0
                && let Some(draw) = self.draw.borrow().as_ref()
            {
                draw(snapshot, area.width(), area.height());
            }
        }
    }
}

glib::wrapper! {
    pub struct RenderArea(ObjectSubclass<imp::RenderArea>)
        @extends gtk::DrawingArea, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for RenderArea {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}

impl RenderArea {
    pub fn set_snapshot_func(&self, draw: impl Fn(&gtk::Snapshot, i32, i32) + 'static) {
        *self.imp().draw.borrow_mut() = Some(Box::new(draw));
        self.queue_draw();
    }
}
