//! Replaces the clipboard. Run only in an isolated Wayland session:
//! COMPOSITOR_TEST_CLIPBOARD=wayland cargo run --example wayland_clipboard_check
#[path = "../tests/support/clipboard_images.rs"]
mod clipboard_images;

use compositor::native_clipboard::wayland::WaylandClipboard;
use quickgui::{
    Application, ClipboardImage, ClipboardImageFormat, ClipboardItem, ClipboardProvider,
    IntoElement, View, ViewContext, WindowOptions, div, text,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

struct Probe;
impl View for Probe {
    fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().child(text("Isolated Wayland clipboard verification"))
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        std::env::var("COMPOSITOR_TEST_CLIPBOARD").as_deref(),
        Ok("wayland")
    );
    assert!(std::env::var_os("DISPLAY").is_none());
    let mut app = Application::new().into_runner()?;
    let clipboard =
        WaylandClipboard::new(app.owned_display_handle())?.ok_or("Wayland is required")?;
    app.set_clipboard_provider(clipboard.clone());
    let _window = app.open_window(WindowOptions::default(), Probe)?;
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        // Let the compositor focus the test surface and deliver its selection.
        std::thread::sleep(Duration::from_secs(1));
        let result = check(clipboard);
        let _ = tx.send(result.map_err(|error| error.to_string()));
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        app.pump(Some(Duration::from_millis(10)))?;
        if let Ok(result) = rx.try_recv() {
            worker
                .join()
                .map_err(|_| "Clipboard test worker panicked")?;
            result?;
            println!(
                "Standard Wayland clipboard: text, image, 20 MiB, repeated ownership, and clear passed"
            );
            break;
        }
        if worker.is_finished() {
            worker
                .join()
                .map_err(|_| "Clipboard test worker panicked")?;
            rx.recv()??;
            break;
        }
        if Instant::now() > deadline {
            return Err("Clipboard verification timed out".into());
        }
    }
    Ok(())
}
fn check(clipboard: WaylandClipboard) -> Result<(), Box<dyn std::error::Error>> {
    for index in 0..12 {
        let value = format!("clipboard text {index}: café 🖌");
        clipboard.write(ClipboardItem::new_string(value.as_str())?)?;
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(clipboard.read()?.and_then(|item| item.text()), Some(value));
        assert!(clipboard.read_image()?.is_none());
    }
    for (format, bytes) in [
        (ClipboardImageFormat::Png, clipboard_images::profiled_png()),
        (
            ClipboardImageFormat::Tiff,
            clipboard_images::oriented_tiff(),
        ),
        (ClipboardImageFormat::Png, vec![37; 20 * 1024 * 1024]),
    ] {
        clipboard.write(ClipboardItem::new_image(ClipboardImage::new(
            format,
            bytes.clone(),
        )?)?)?;
        std::thread::sleep(Duration::from_millis(50));
        let received = clipboard.read_image()?.ok_or("No image returned")?;
        assert_eq!(received, bytes);
        if format == ClipboardImageFormat::Tiff {
            assert_eq!(
                compositor::image_io::read_encoded(&received)?.dimensions(),
                (3, 7)
            );
        } else if bytes.len() < 10_000 {
            let pixels = compositor::image_io::read_encoded(&received)?;
            assert!((187..=189).contains(&pixels[(0, 0)][0]));
            assert_eq!(pixels[(0, 0)][3], 123);
        }
        assert!(clipboard.read()?.is_none());
    }
    clipboard.write(ClipboardItem::default())?;
    std::thread::sleep(Duration::from_millis(50));
    assert!(clipboard.read_image()?.is_none());
    Ok(())
}
