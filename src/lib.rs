//! Core application engine and runtime orchestration.
//!
//! This module contains the top-level [`Engine`] type responsible for
//! initializing and running the Taurino application runtime.
//!
//! The engine owns the Tao event loop and coordinates the shared runtime
//! infrastructure used by windows, WebViews, menus, tray icons and other
//! engine subsystems.
//!
//! # Architecture
//!
//! The runtime is divided into two major layers:
//!
//! - [`Engine`] owns the process-level lifecycle and the event loop.
//! - [`EngineManager`] owns and exposes the individual application managers.
//!
//! The engine itself does not directly implement window, WebView, menu or tray
//! behavior. Instead, these responsibilities are delegated to the managers
//! accessible through [`EngineManager`].
//!
//! # Event loop
//!
//! Tao requires its event loop to be owned and driven by the application
//! thread. Therefore [`Engine`] owns the [`TaurinoLoop`] directly.
//!
//! External events, such as process termination signals, are converted into
//! [`EventLoopMessage`] values and sent through Tao's event-loop proxy. This
//! ensures that lifecycle operations are executed on the event-loop thread
//! rather than directly from signal-handler threads.
//!
//! # Shutdown
//!
//! Supported operating-system termination signals are translated into
//! [`EventLoopMessage::Shutdown`] messages.
//!
//! The actual shutdown procedure is then handled by [`EngineEventHandler`].
//! This keeps all application cleanup serialized through the Tao event loop
//! and avoids performing GUI or manager operations from an arbitrary thread.

use std::{sync::Arc, vec};
mod handler;
mod manager;
mod trayicon;
mod window;
use anyhow::Result;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use anyhow::anyhow;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use taurino_core::webview::Webview;
use taurino_core::{
  Config, EngineLoop, EngineLoopBuilder, EventLoopMessage,
  async_runtime::IPCRuntime,
  menu::MenuManager,
  schema::{
    webview::{WebViewConfig, WebviewUrl},
    window::WindowConfig,
  },
  tools::{
    ArcMut, arc_mut,
    resources::ResourceTable,
    stores::{DeviceRegistry, WebContextStore},
  },
};

use crate::{handler::EngineEventHandler, manager::EngineManager};

/// Top-level owner and runtime coordinator of a Taurino application.
///
/// `Engine` owns the process-level application runtime. Its main purpose is to
/// initialize shared state, create the platform event loop and transfer control
/// to the application's event-processing lifecycle.
///
/// It is intentionally kept separate from [`EngineManager`]:
///
/// - `Engine` owns the runtime and lifecycle.
/// - `EngineManager` provides access to application subsystems and shared state.
///
/// This separation allows subsystem managers to be shared independently through
/// [`Arc`] while keeping ownership of the Tao event loop centralized.
///
/// # Responsibilities
///
/// `Engine` is responsible for:
///
/// - creating the shared WebView context;
/// - creating the global resource table;
/// - initializing the menu manager;
/// - creating the Tao event loop;
/// - installing platform-specific Tao integrations;
/// - installing process termination handling;
/// - creating the shared [`EngineManager`];
/// - creating initial application windows;
/// - forwarding Tao events to [`EngineEventHandler`];
/// - owning the application runtime until the event loop terminates.
///
/// # Lifecycle
///
/// The typical lifecycle is:
///
/// ```text
/// Engine::new()
///      │
///      ├─ initialize WebContextStore
///      ├─ initialize ResourceTable
///      ├─ initialize MenuManager
///      ├─ create Tao EventLoop
///      ├─ install OS signal forwarding
///      └─ create EngineManager
///             │
///             ▼
/// Engine::start()
///      │
///      ├─ configure global handlers
///      ├─ create initial windows
///      └─ enter Tao event loop
///             │
///             ▼
/// EngineEventHandler
///      │
///      ├─ window events
///      ├─ user events
///      └─ shutdown
/// ```
///
/// # Threading
///
/// The Tao event loop remains owned by `Engine` and is executed on the thread
/// from which [`Engine::start`] is called.
///
/// Managers and other runtime resources may internally use synchronized shared
/// ownership, but operations involving Tao windows or event-loop state should
/// remain serialized through the event-loop thread.
///
/// # Shutdown handling
///
/// Process termination signals are not handled directly inside the signal
/// callback. Instead, the callback sends [`EventLoopMessage::Shutdown`] through
/// an [`EventLoopProxy`](taurino_core::tao::event_loop::EventLoopProxy).
///
/// This design prevents GUI cleanup or manager operations from executing on the
/// signal-handling thread.
///
///
///
#[allow(dead_code)]
pub struct Engine {
  device_registry: ArcMut<DeviceRegistry>,
  /// Shared WebView context storage.
  ///
  /// The context store is shared between WebViews created by the engine.
  /// Keeping it at engine scope allows WebViews to reuse browser context,
  /// storage and other WebView runtime state where supported by the
  /// underlying platform.
  webcontext: WebContextStore,

  /// Tao application event loop.
  ///
  /// The event loop represents the central execution loop of the GUI
  /// application and receives native window events as well as internal
  /// [`EventLoopMessage`] values.
  ///
  /// It is owned directly by the engine because Tao associates the event loop
  /// with the application thread and expects it to remain alive for the
  /// lifetime of the GUI runtime.
  event_loop: EngineLoop,

  /// Shared application resource table.
  ///
  /// The resource table stores engine-managed resources that need stable
  /// identifiers and application-wide access.
  ///
  /// The synchronized shared container allows managers and API layers to
  /// reference the same table without transferring ownership.
  table: ArcMut<ResourceTable>,

  /// Shared collection of engine subsystem managers.
  ///
  /// [`EngineManager`] provides access to managers for windows, menus, tray
  /// icons and other application-wide services.
  ///
  /// It is reference counted because individual subsystems and callbacks may
  /// need to retain access to the engine state independently.
  manager: Arc<EngineManager>,
  config: Config,
  ipc_runtime: Arc<IPCRuntime>,
}

impl Engine {
  /// Creates and initializes a new application engine.
  ///
  /// This method prepares all process-level runtime components required
  /// before the application event loop can be started.
  ///
  /// # Initialization sequence
  ///
  /// The following components are initialized:
  ///
  /// 1. shared [`WebContextStore`];
  /// 2. global [`ResourceTable`];
  /// 3. menu manager;
  /// 4. Tao event-loop builder;
  /// 5. platform-specific event-loop integrations;
  /// 6. Tao event loop;
  /// 7. process termination signal handler;
  /// 8. shared [`EngineManager`].
  ///
  /// # Windows integration
  ///
  /// On Windows, the menu subsystem installs a native message hook into Tao's
  /// event-loop builder.
  ///
  /// This allows the menu manager to participate in native Windows message
  /// processing without introducing a second message loop.
  ///
  /// # Process signals
  ///
  /// A process termination handler is installed through `ctrlc`.
  ///
  /// The handler does **not** perform application cleanup directly. Instead,
  /// it sends [`EventLoopMessage::Shutdown`] into the Tao event loop.
  ///
  /// This distinction is important because signal callbacks may execute on a
  /// separate thread while Tao window and lifecycle operations belong to the
  /// event-loop thread.
  ///
  /// # Errors
  ///
  /// Returns an error if one of the engine subsystems cannot be initialized.
  ///
  /// The signal-handler installation currently uses `expect`, meaning that a
  /// failure to install the process handler causes process termination rather
  /// than being returned through this method.
  pub fn new(config: Vec<u8>) -> Result<Self> {
    let mut config = Config::new(config)?;
    config.add_window_config(WindowConfig {
      // center: true,
      center: true,
      enable_drag_drop: true,
      webviews: vec![WebViewConfig {
        use_https_scheme: true,
        drag_drop_enabled: true,
        // Configure this WebView as a child of the native
        // Taurino window.

        // Load an external HTTPS resource.
        url: WebviewUrl::default(),

        // Preserve subsystem defaults for all options that
        // are not explicitly required here.
        ..Default::default()
      }],

      ..Default::default()
    });
    config.add_window_config(WindowConfig {
      // center: true,
      label: "sub:window".to_string(),
      enable_drag_drop: true,
      center: true,
      webviews: vec![WebViewConfig {
        drag_drop_enabled: true,
        // Configure this WebView as a child of the native
        // Taurino window.

        // Load an external HTTPS resource.
        url: WebviewUrl::default(),

        // Preserve subsystem defaults for all options that
        // are not explicitly required here.
        ..Default::default()
      }],

      ..Default::default()
    });

    let device_registry = DeviceRegistry::new()?;
    // ---------------------------------------------------------------------
    // Shared WebView runtime
    // ---------------------------------------------------------------------
    //
    // The context store is created once at engine startup and shared with
    // WebView-related components throughout the application lifetime.
    let webcontext = WebContextStore::new();

    // ---------------------------------------------------------------------
    // Global resource registry
    // ---------------------------------------------------------------------
    //
    // Resources stored here can be referenced from multiple engine
    // subsystems while preserving one canonical resource table.
    let resource_table = arc_mut(ResourceTable::default());

    // ---------------------------------------------------------------------
    // Menu subsystem
    // ---------------------------------------------------------------------
    //
    // The menu manager is initialized before constructing the final engine
    // manager because platform-specific event-loop integration may require
    // access to it.
    let menu_manager = MenuManager::new()?;

    // ---------------------------------------------------------------------
    // Tao event-loop construction
    // ---------------------------------------------------------------------
    //
    // A user-event-capable event loop is required because Taurino injects
    // internal engine messages such as shutdown commands into the native
    // GUI event stream.
    let mut loop_builder = EngineLoopBuilder::with_user_event();

    // ---------------------------------------------------------------------
    // Windows native message integration
    // ---------------------------------------------------------------------
    //
    // Windows menus participate in the Win32 message pump. The menu
    // manager therefore installs its message hook into Tao before the
    // event loop itself is built.
    #[cfg(windows)]
    {
      use taurino_core::native::tao::platform::windows::EventLoopBuilderExtWindows;
      loop_builder.with_msg_hook(MenuManager::install_msg_hook(menu_manager.clone()));
    }

    // Build the final event loop after all platform extensions have been
    // configured.
    let event_loop = loop_builder.build();

    // ---------------------------------------------------------------------
    // Process shutdown bridge
    // ---------------------------------------------------------------------
    //
    // EventLoopProxy is Send-capable and provides the safe boundary between
    // the ctrlc callback thread and Tao's event-loop thread.
    let ctrlc_proxy = event_loop.create_proxy();
    let proxy = event_loop.create_proxy();
    ctrlc::set_handler(move || {
      // Ignore delivery failure here because it normally means that the
      // Tao event loop has already terminated and therefore no shutdown
      // work can be scheduled anymore.
      let _ = ctrlc_proxy.send_event(EventLoopMessage::Shutdown);
    })
    .expect("failed to install signal handler");

    // ---------------------------------------------------------------------
    // Shared manager infrastructure
    // ---------------------------------------------------------------------
    //
    // EngineManager receives the globally shared runtime components and
    // constructs the subsystem managers around them.
    let ipc_runtime = IPCRuntime::new(&config, proxy.clone())?;
    let manager = EngineManager::new(
      ipc_runtime.clone(),
      webcontext.clone(),
      resource_table.clone(),
      menu_manager,
      proxy,
      config.clone(),
      device_registry.clone(),
    )?;

    Ok(Self {
      config,
      device_registry,
      webcontext,
      event_loop,
      manager,
      table: resource_table,
      ipc_runtime,
    })
  }

  /// Returns the shared application resource table.
  ///
  /// The returned value references the same underlying table owned by the
  /// engine rather than creating a new independent resource registry.
  ///
  /// Callers may therefore use the returned handle to access resources
  /// registered by other engine subsystems.
  pub fn resource_table(&self) -> ArcMut<ResourceTable> {
    self.table.clone()
  }

  /// Returns the shared WebView context store.
  ///
  /// The returned store is a clone of the shared context handle and refers
  /// to the same underlying WebView runtime state where supported by
  /// [`WebContextStore`].
  pub fn webcontext(&self) -> WebContextStore {
    self.webcontext.clone()
  }

  /// Starts the application runtime and enters the Tao event loop.
  ///
  /// This method consumes the engine because control is transferred to Tao's
  /// event loop for the remainder of the GUI application's runtime.
  ///
  /// # Startup sequence
  ///
  /// Before entering the event loop, the method:
  ///
  /// 1. installs global window event handling;
  /// 2. installs global WebView event handling;
  /// 3. creates the initial application window;
  /// 4. creates [`EngineEventHandler`];
  /// 5. transfers event processing to Tao.
  ///
  /// # Event dispatch
  ///
  /// Every Tao event is forwarded to [`EngineEventHandler::handle_event`].
  ///
  /// The event handler is responsible for interpreting native window events,
  /// handling Taurino user events and coordinating application shutdown.
  ///
  /// # Return behavior
  ///
  /// Depending on the Tao version and platform implementation, entering the
  /// event loop normally transfers control until application termination.
  pub fn start(self) -> Result<()> {
    // Clone the manager because ownership of `self.manager` will later be
    // transferred into EngineEventHandler.
    let manager = self.manager.clone();

    // ---------------------------------------------------------------------
    // Global window event observer
    // ---------------------------------------------------------------------
    //
    // This callback receives normalized engine-level window events.
    //
    // The current implementation only prints diagnostics. Applications or
    // higher framework layers can replace this with their own dispatcher.
    manager.set_global_window_event_handler(|_manager, window_label, event| {
      println!("Window `{window_label}` emitted: {:?}", event);
    })?;

    // ---------------------------------------------------------------------
    // Global WebView event observer
    // ---------------------------------------------------------------------
    //
    // Similar to window events, WebView events are forwarded through one
    // engine-wide callback.
    manager.set_global_webview_event_handler(|_manager, window_label, webview_label, event| {
      println!(
        "WebView `{webview_label}` in window \
                     `{window_label}` emitted: {:?}",
        event
      );
    })?;

    // ---------------------------------------------------------------------
    // Initial application window
    // ---------------------------------------------------------------------
    //
    // Window creation is performed before entering Tao's event loop because
    // the initial runtime target is already available through `event_loop`.

    for window in self.config.get_windows() {
      if window.create {
        manager.window()?.open_window(window, &self.event_loop)?;
      }
    }

    // ---------------------------------------------------------------------
    // Central runtime event handler
    // ---------------------------------------------------------------------
    //
    // EngineEventHandler owns the shared manager reference and coordinates
    // application lifecycle behavior for incoming Tao events.
    let event_handler = EngineEventHandler::new(self.manager);

    // ---------------------------------------------------------------------
    // Enter GUI event loop
    // ---------------------------------------------------------------------
    //
    // From this point onward, application lifecycle changes should normally
    // be driven by events rather than by direct calls from external threads.
    self.event_loop.run(move |event, target, control_flow| {
      event_handler.handle_event(event, target, control_flow);
    });
  }
}

// =============================================================================
// Error handling helpers
// =============================================================================

/// Executes a fallible block and logs any returned error.
///
/// The block is evaluated inside a closure returning [`Result<()>`].
/// This allows the caller to use the `?` operator inside the supplied block
/// without propagating the resulting error to the surrounding function.
///
/// Errors are forwarded to [`log_err!`] instead.
///
/// # Intended use
///
/// This macro is useful for best-effort operations where failure should be
/// recorded but must not interrupt the surrounding event-processing path.
///
/// # Example
///
/// ```ignore
/// try_or_log_err!({
///     manager.cleanup()?;
///     manager.flush()?;
///     Ok(())
/// });
/// ```
///
/// # Important
///
/// Because errors are consumed by this macro, it should not be used for
/// operations whose failure must influence control flow or be reported to the
/// caller.
#[macro_export]
macro_rules! try_or_log_err {
  ($body:block) => {
    match (move || -> Result<()> { $body })() {
      Ok(_) => {}

      Err(e) => {
        crate::log_err!(e);
      }
    }
  };
}

/// Logs an error when a [`Result`] is `Err`.
///
/// Successful values are intentionally ignored.
///
/// Unlike [`try_or_log_err!`], this macro receives an already evaluated result
/// rather than executing a block.
///
/// # Intended use
///
/// This is primarily useful for cleanup paths and event handling where an
/// individual failure should be recorded without aborting subsequent cleanup.
///
///
#[macro_export]
macro_rules! log_if_err {
  ($result:expr) => {{
    if let Err(error) = $result {
      taurino_core::taurino_error!("{error}");
    }
  }};
}

#[macro_export]
macro_rules! log_err {
  ($error:expr) => {{
    taurino_core::taurino_error!("{}", $error);
  }};
}

/*
#[macro_export]
macro_rules! log_if_err {
  ($result:expr) => {
    if let Err(e) = $result {
      taurino_core::taurino_log!(
        taurino_core::tools::logging::Level::Error,
        "{e}"
      );
    }
  };
}

#[macro_export]
macro_rules! log {
  ($result:expr) => {
    taurino_core::taurino_log!(
      taurino_core::tools::logging::Level::Info,
      "{}",
      $result
    );
  };
}

#[macro_export]
macro_rules! log_err {
  ($result:expr) => {
    taurino_core::taurino_log!(
      taurino_core::tools::logging::Level::Error,
      "{}",
      $result
    );
  };
}

*/
