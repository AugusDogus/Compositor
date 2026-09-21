//! Opt-in because these checks replace the selected desktop's clipboard.
//! Run only on an isolated Xvfb or nested Wayland compositor.
use compositor::{image_io, native_clipboard};

#[path = "support/clipboard_images.rs"]
mod clipboard_images;
use clipboard_images::{oriented_tiff, profiled_png};

#[test]
#[ignore = "replaces the clipboard; needs COMPOSITOR_TEST_CLIPBOARD=x11 or wayland on an isolated display"]
fn native_transfers_preserve_profiles_orientation_and_large_payloads() {
    let backend =
        std::env::var("COMPOSITOR_TEST_CLIPBOARD").expect("Choose an isolated clipboard backend");
    assert!(matches!(backend.as_str(), "x11" | "wayland"));
    let x11 = if backend == "x11" {
        assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
        Some(x11_clipboard::Clipboard::new().unwrap())
    } else {
        assert!(std::env::var_os("DISPLAY").is_none());
        None
    };
    for (mime, bytes) in [
        ("image/png", profiled_png()),
        ("image/tiff", oriented_tiff()),
        // Larger than X11's maximum single ChangeProperty request: exercise INCR.
        ("image/png", vec![37; 20 * 1024 * 1024]),
        ("text/plain", b"clipboard text".to_vec()),
    ] {
        if let Some(clipboard) = &x11 {
            let target = clipboard.setter.get_atom(mime).unwrap();
            clipboard
                .store(clipboard.setter.atoms.clipboard, target, bytes.clone())
                .unwrap();
        } else {
            use wl_clipboard_rs::copy::{MimeType, Options, Source};
            Options::new()
                .copy(
                    Source::Bytes(bytes.clone().into_boxed_slice()),
                    MimeType::Specific(mime.into()),
                )
                .unwrap();
        }
        let received = native_clipboard::read_image().unwrap();
        if mime == "text/plain" {
            assert!(received.is_none());
            continue;
        }
        let received = received.unwrap();
        assert_eq!(received.len(), bytes.len());
        assert!(received == bytes, "Encoded clipboard data changed");
        if mime == "image/tiff" {
            assert_eq!(
                image_io::read_encoded(&received).unwrap().dimensions(),
                (3, 7)
            );
        } else if received.len() < 10_000 {
            let pixels = image_io::read_encoded(&received).unwrap();
            assert!((187..=189).contains(&pixels[(0, 0)][0]));
            assert_eq!(pixels[(0, 0)][3], 123);
        }
    }
}
