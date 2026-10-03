#[cfg(windows)]
use std::sync::atomic::Ordering;
use std::{path::PathBuf, sync::mpsc::Sender};

use taurino_core::dpi::{self, Theme};
#[cfg(windows)]
use taurino_window::window::Window;
use taurino_window::{webview::inner_size, window::map_theme_from_tao};

/// An event from a window.
#[derive(Debug, Clone)]
pub enum WindowEvent {
    /// The size of the window has changed. Contains the client area's new dimensions.
    Resized(dpi::PhysicalSize<u32>),
    /// The position of the window has changed. Contains the window's new position.
    Moved(dpi::PhysicalPosition<i32>),
    /// The window has been requested to close.
    CloseRequested {
        /// A signal sender. If a `true` value is emitted, the window won't be closed.
        signal_tx: Sender<bool>,
    },
    /// The window has been destroyed.
    Destroyed,
    /// The window gained or lost focus.
    ///
    /// The parameter is true if the window has gained focus, and false if it has lost focus.
    Focused(bool),
    /// The window's scale factor has changed.
    ///
    /// The following user actions can cause DPI changes:
    ///
    /// - Changing the display's resolution.
    /// - Changing the display's scale factor (e.g. in Control Panel on Windows).
    /// - Moving the window to a display with a different scale factor.
    ScaleFactorChanged {
        /// The new scale factor.
        scale_factor: f64,
        /// The window inner size.
        new_inner_size: dpi::PhysicalSize<u32>,
    },
    /// An event associated with the drag and drop action.
    DragDrop(DragDropEvent),
    /// The system window theme has changed.
    ///
    /// Applications might wish to react to this to change the theme of the content of the window when the system changes the window theme.
    ThemeChanged(Theme),
    /*
    /// Emitted when the application has been suspended.
    ///
    /// ## Platform-specific
    ///
    /// - **Android**: This is triggered by `onPause` method of the Activity.
    /// - **iOS**: This is triggered by `applicationWillResignActive` method of the UIApplicationDelegate.
    /// - **Linux / macOS / Windows**: Unsupported.
    #[cfg(mobile)]
    #[cfg_attr(docsrs, doc(cfg(any(target_os = "android", target_os = "ios"))))]
    Suspended,
    */

    /*
    /// Emitted when the application has been resumed.
    ///
    /// ## Platform-specific
    ///
    /// - **Android**: This is triggered by `onResume` method of the Activity. The first onResume() is ignored to match the iOS implementation, since that is called on activity creation.
    /// - **iOS**: This is triggered by `applicationWillEnterForeground` method of the UIApplicationDelegate.
    /// - **Linux / macOS / Windows**: Unsupported.
    #[cfg(mobile)]
    #[cfg_attr(docsrs, doc(cfg(any(target_os = "android", target_os = "ios"))))]
    Resumed,
    */
}

/// An event from a window.
#[derive(Debug, Clone)]
pub enum WebviewEvent {
    /// An event associated with the drag and drop action.
    DragDrop(DragDropEvent),
}

/// The drag drop event payload.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DragDropEvent {
    /// A drag operation has entered the webview.
    Enter {
        /// List of paths that are being dragged onto the webview.
        paths: Vec<PathBuf>,
        /// The position of the mouse cursor.
        position: dpi::PhysicalPosition<f64>,
    },
    /// A drag operation is moving over the webview.
    Over {
        /// The position of the mouse cursor.
        position: dpi::PhysicalPosition<f64>,
    },
    /// The file(s) have been dropped onto the webview.
    Drop {
        /// List of paths that are being dropped onto the window.
        paths: Vec<PathBuf>,
        /// The position of the mouse cursor.
        position: dpi::PhysicalPosition<f64>,
    },
    /// The drag operation has been cancelled or left the window.
    Leave,
}

pub struct WindowEventWrapper(pub Option<WindowEvent>);

impl WindowEventWrapper {
    pub fn map_from_tao(event: &taurino_core::tao::event::WindowEvent<'_>, #[cfg(windows)] window: &Window) -> Self {
        let event = match event {
            taurino_core::tao::event::WindowEvent::Resized(size) => WindowEvent::Resized(*size),
            taurino_core::tao::event::WindowEvent::Moved(position) => WindowEvent::Moved(*position),
            taurino_core::tao::event::WindowEvent::Destroyed => WindowEvent::Destroyed,
            taurino_core::tao::event::WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
            } => WindowEvent::ScaleFactorChanged {
                scale_factor: *scale_factor,
                new_inner_size: **new_inner_size,
            },
            taurino_core::tao::event::WindowEvent::Focused(focused) => {
                #[cfg(not(windows))]
                return Self(Some(WindowEvent::Focused(*focused)));
                // on multiwebview mode, if there's no focused webview, it means we're receiving a direct window focus change
                // (without receiving a webview focus, such as when clicking the taskbar app icon or using Alt + Tab)
                // in this case we must send the focus change event here
                #[cfg(windows)]
                if window.has_children.load(Ordering::Relaxed) {
                    use taurino_window::config::FocusState;

                    if !*focused {
                        // Blur events are handled in the webview side (add_LostFocus)
                        return Self(None);
                    }

                    let mut focused_webview = window.focused_webview.lock().unwrap();
                    if let FocusState::Blured {
                        last_focused_webview_label,
                    } = &*focused_webview
                    {
                        let should_focus_webview = last_focused_webview_label
                            .as_deref()
                            .and_then(|last_focused_webview_label| window.webview_by_label(last_focused_webview_label));
                        *focused_webview = FocusState::WindowFocused;
                        if let Some(should_focus_webview) = should_focus_webview {
                            drop(focused_webview);
                            let _ = should_focus_webview.focus();
                        }
                        WindowEvent::Focused(true)
                    } else {
                        // Already focused
                        return Self(None);
                    }
                } else if window.webviews().is_empty() {
                    // Raw tao window without webviews, forward the event
                    WindowEvent::Focused(*focused)
                } else {
                    // when not on multiwebview mode, wry will set focus to the webview,
                    // and we will handle focus change events on the webview (add_GotFocus and add_LostFocus)
                    return Self(None);
                }
            }
            taurino_core::tao::event::WindowEvent::ThemeChanged(theme) => {
                WindowEvent::ThemeChanged(map_theme_from_tao(theme.clone()))
            }
            /*             #[cfg(mobile)]
            taurino_core::tao::event::WindowEvent::Suspended => WindowEvent::Suspended,
            #[cfg(mobile)]
            taurino_core::tao::event::WindowEvent::Resumed => WindowEvent::Resumed,
             */
            _ => return Self(None),
        };
        Self(Some(event))
    }

    pub fn parse(window: &Window, event: &taurino_core::tao::event::WindowEvent<'_>) -> Self {
        match event {
            // resized event from tao doesn't include a reliable size on macOS
            // because wry replaces the NSView
            taurino_core::tao::event::WindowEvent::Resized(_) => {
                if let Some(w) = &window.inner() {
                    let size = inner_size(w, &window.webviews(), window.has_children.load(Ordering::Relaxed));
                    Self(Some(WindowEvent::Resized(size)))
                } else {
                    Self(None)
                }
            }
            e => Self::map_from_tao(
                e,
                #[cfg(windows)]
                window,
            ),
        }
    }
}
