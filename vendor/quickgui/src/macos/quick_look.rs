use objc2::runtime::AnyClass;

use super::*;

// QLPreviewPanel lives in the Quartz umbrella framework. Linking it here makes the class
// available to the Objective-C runtime lookup below without adding a crate dependency.
#[link(name = "Quartz", kind = "framework")]
unsafe extern "C" {}

/// Retained preview item exposed to `QLPreviewPanel` through the `QLPreviewItem` protocol.
///
/// QuickGUI copies the already-validated path and display name into Foundation objects, so the
/// panel never reaches back into runtime state while it is open.
struct PreviewItemIvars {
    url: Retained<NSURL>,
    title: Option<Retained<NSString>>,
}

declare_class!(
    struct QuickGuiPreviewItem;

    unsafe impl ClassType for QuickGuiPreviewItem {
        type Super = NSObject;
        type Mutability = InteriorMutable;
        const NAME: &'static str = "QuickGuiPreviewItem";
    }

    impl DeclaredClass for QuickGuiPreviewItem {
        type Ivars = PreviewItemIvars;
    }

    unsafe impl QuickGuiPreviewItem {
        #[method_id(previewItemURL)]
        fn preview_item_url(&self) -> Retained<NSURL> {
            self.ivars().url.clone()
        }

        #[method_id(previewItemTitle)]
        fn preview_item_title(&self) -> Option<Retained<NSString>> {
            self.ivars().title.clone()
        }
    }
);

impl QuickGuiPreviewItem {
    fn new(url: Retained<NSURL>, title: Option<Retained<NSString>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PreviewItemIvars { url, title });
        unsafe { msg_send_id![super(this), init] }
    }
}

struct PreviewSourceIvars {
    item: Retained<QuickGuiPreviewItem>,
}

declare_class!(
    struct QuickGuiPreviewSource;

    unsafe impl ClassType for QuickGuiPreviewSource {
        type Super = NSObject;
        type Mutability = InteriorMutable;
        const NAME: &'static str = "QuickGuiPreviewSource";
    }

    impl DeclaredClass for QuickGuiPreviewSource {
        type Ivars = PreviewSourceIvars;
    }

    unsafe impl QuickGuiPreviewSource {
        #[method(numberOfPreviewItemsInPreviewPanel:)]
        fn number_of_items(&self, _panel: &AnyObject) -> isize {
            1
        }

        #[method_id(previewPanel:previewItemAtIndex:)]
        fn item_at_index(
            &self,
            _panel: &AnyObject,
            _index: isize,
        ) -> Retained<QuickGuiPreviewItem> {
            self.ivars().item.clone()
        }
    }
);

impl QuickGuiPreviewSource {
    fn new(item: Retained<QuickGuiPreviewItem>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PreviewSourceIvars { item });
        unsafe { msg_send_id![super(this), init] }
    }
}

thread_local! {
    /// The data source the shared preview panel is currently reading.
    ///
    /// AppKit holds the data source weakly, so exactly one core-owned object is retained here for
    /// as long as the panel is open and released when the preview is closed.
    static ACTIVE_PREVIEW_SOURCE: RefCell<Option<Retained<QuickGuiPreviewSource>>> =
        const { RefCell::new(None) };
}

fn preview_panel_class() -> Result<&'static AnyClass, PlatformError> {
    AnyClass::get("QLPreviewPanel").ok_or_else(|| {
        PlatformError::Platform("Quick Look is not available in this process".into())
    })
}

/// Show the shared Quick Look panel for one already-validated filesystem path.
pub(crate) fn preview_file(path: &Path, display_name: Option<&str>) -> Result<(), PlatformError> {
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        PlatformError::Platform("Quick Look must be driven from the AppKit main thread".into())
    })?;
    let _ = mtm;
    let class = preview_panel_class()?;
    let url =
        native_file_url(path, false).map_err(|error| PlatformError::Platform(error.into()))?;
    let item = QuickGuiPreviewItem::new(url, display_name.map(NSString::from_str));
    let source = QuickGuiPreviewSource::new(item);

    let panel: Option<Retained<AnyObject>> = unsafe { msg_send_id![class, sharedPreviewPanel] };
    let panel = panel.ok_or_else(|| {
        PlatformError::Platform("Quick Look did not provide its shared panel".into())
    })?;
    unsafe {
        let source_ref: &AnyObject = &source;
        let _: () = msg_send![&panel, setDataSource: source_ref];
        let _: () = msg_send![&panel, reloadData];
        let _: () = msg_send![&panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    }
    ACTIVE_PREVIEW_SOURCE.with(|active| *active.borrow_mut() = Some(source));
    Ok(())
}

/// Close the shared Quick Look panel and release the core-owned data source.
pub(crate) fn close_file_preview() -> Result<(), PlatformError> {
    MainThreadMarker::new().ok_or_else(|| {
        PlatformError::Platform("Quick Look must be driven from the AppKit main thread".into())
    })?;
    let class = preview_panel_class()?;
    let exists: bool = unsafe { msg_send![class, sharedPreviewPanelExists] };
    if exists {
        let panel: Option<Retained<AnyObject>> = unsafe { msg_send_id![class, sharedPreviewPanel] };
        if let Some(panel) = panel {
            unsafe {
                let _: () = msg_send![&panel, setDataSource: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![&panel, orderOut: std::ptr::null::<AnyObject>()];
            }
        }
    }
    ACTIVE_PREVIEW_SOURCE.with(|active| active.borrow_mut().take());
    Ok(())
}
