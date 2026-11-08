use gtk::{gdk_pixbuf::Pixbuf, gio, glib};

pub(super) const NAME: &str = "io.github.openmusic.Player";
pub(super) type Pixmap = (i32, i32, Vec<u8>);

pub(super) fn pixmaps() -> Result<Vec<Pixmap>, glib::Error> {
    let svg = glib::Bytes::from_static(include_bytes!(
        "../../resources/io.github.openmusic.Player.svg"
    ));
    [16, 22, 24, 32, 48, 64, 128]
        .into_iter()
        .map(|size| {
            let stream = gio::MemoryInputStream::from_bytes(&svg);
            let image =
                Pixbuf::from_stream_at_scale(&stream, size, size, true, None::<&gio::Cancellable>)?;
            Ok(to_pixmap(&image))
        })
        .collect()
}

fn to_pixmap(image: &Pixbuf) -> Pixmap {
    let (width, height) = (image.width(), image.height());
    let channels = image.n_channels() as usize;
    let stride = image.rowstride() as usize;
    let pixels = image.read_pixel_bytes();
    let mut argb = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        // The last row need not include padding. StatusNotifierItem requires
        // packed ARGB32 in network byte order, rather than GdkPixbuf's RGBA.
        let row = &pixels[y * stride..y * stride + width as usize * channels];
        for pixel in row.chunks_exact(channels) {
            let alpha = if image.has_alpha() { pixel[3] } else { 255 };
            argb.extend_from_slice(&[alpha, pixel[0], pixel[1], pixel[2]]);
        }
    }
    (width, height, argb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gdk_pixbuf::Colorspace;

    #[test]
    fn tray_pixels_use_network_argb_and_skip_row_padding() {
        let bytes = glib::Bytes::from_owned(vec![12, 34, 56, 78, 0, 0, 0, 0, 90, 123, 145, 167]);
        let image = Pixbuf::from_bytes(&bytes, Colorspace::Rgb, true, 8, 1, 2, 8);
        assert_eq!(
            to_pixmap(&image),
            (1, 2, vec![78, 12, 34, 56, 167, 90, 123, 145])
        );
        let bytes = glib::Bytes::from_owned(vec![12, 34, 56, 0, 90, 123, 145]);
        let image = Pixbuf::from_bytes(&bytes, Colorspace::Rgb, false, 8, 1, 2, 4);
        assert_eq!(
            to_pixmap(&image),
            (1, 2, vec![255, 12, 34, 56, 255, 90, 123, 145])
        );
    }

    #[test]
    fn tray_icon_renders_the_app_brand_without_an_icon_theme() {
        let images = pixmaps().expect("The embedded application SVG must render");
        assert_eq!(
            images.iter().map(|(w, _, _)| *w).collect::<Vec<_>>(),
            [16, 22, 24, 32, 48, 64, 128]
        );
        for (width, height, pixels) in &images {
            assert_eq!(width, height);
            assert_eq!(pixels.len(), (width * height * 4) as usize);
            assert!(pixels.chunks_exact(4).any(|p| p[0] == 0));
            assert!(pixels.chunks_exact(4).any(|p| p[0] == 255));
        }
        let (_, _, pixels) = images.iter().find(|(width, _, _)| *width == 64).unwrap();
        let foreground = (32 * 64 + 27) * 4;
        assert_eq!(&pixels[foreground..foreground + 4], [255, 194, 223, 163]);
        let background = (32 * 64 + 32) * 4;
        assert_eq!(&pixels[background..background + 4], [255, 41, 56, 44]);
    }
}
