use std::{fmt, path::PathBuf};

use serde_with::skip_serializing_none;
use taurino_core::{
    anyhow,
    serde::{Deserialize, Deserializer, Serialize},
};
use url::Url;

use crate::window::options::{WindowOptions, default_true};

/// An URL to open on a Tauri webview window.
#[derive(PartialEq, Eq, Debug, Clone, Serialize)]
#[serde(crate = "taurino_core::serde")]
#[serde(untagged)]
#[non_exhaustive]
pub enum WebviewUrl {
    /// An external URL. Must use either the `http` or `https` schemes.
    External(Url),
    /// The path portion of an app URL.
    /// For instance, to load `tauri://localhost/users/john`,
    /// you can simply provide `users/john` in this configuration.
    App(PathBuf),
    /// A custom protocol url, for example, `doom://index.html`
    CustomProtocol(Url),
}

impl<'de> Deserialize<'de> for WebviewUrl {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(crate = "taurino_core::serde")]
        #[serde(untagged)]
        enum WebviewUrlDeserializer {
            Url(Url),
            Path(PathBuf),
        }

        match WebviewUrlDeserializer::deserialize(deserializer)? {
            WebviewUrlDeserializer::Url(u) => {
                if u.scheme() == "https" || u.scheme() == "http" {
                    Ok(Self::External(u))
                } else {
                    Ok(Self::CustomProtocol(u))
                }
            }
            WebviewUrlDeserializer::Path(p) => Ok(Self::App(p)),
        }
    }
}

impl fmt::Display for WebviewUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::External(url) | Self::CustomProtocol(url) => {
                write!(f, "{url}")
            }
            Self::App(path) => write!(f, "{}", path.display()),
        }
    }
}

impl Default for WebviewUrl {
    fn default() -> Self {
        Self::App("index.html".into())
    }
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(
    crate = "taurino_core::serde",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub struct WebViewOptions {
    pub label: String,
    /// Whether the webview is a child of the window or not. Defaults to `false`.
    #[serde(default)]
    pub child: bool,
    /// The window webview URL.
    #[serde(default)]
    pub url: WebviewUrl,
    /// The user agent for the webview
    #[serde(alias = "user-agent")]
    pub user_agent: Option<String>,
    /// Whether the drag and drop handlers used internally to generate [`DragDropEvent`]s are enabled on the webview. By default it is enabled.
    ///
    /// Disabling it is required to use HTML5 drag and drop on the frontend on Windows since we replace the drag drop handler of WebView2.
    ///
    /// Note: this setting maps to [`WebviewBuilder::disable_drag_drop_handler`], not [`WindowBuilder::drag_and_drop`].
    ///
    /// [`DragDropEvent`]: https://docs.rs/tauri/latest/tauri/enum.DragDropEvent.html
    /// [`WebviewBuilder::disable_drag_drop_handler`]: https://docs.rs/tauri/latest/tauri/webview/struct.WebviewBuilder.html#method.disable_drag_drop_handler
    /// [`WindowBuilder::drag_and_drop`]: https://docs.rs/tauri/latest/x86_64-pc-windows-msvc/tauri/window/struct.WindowBuilder.html#method.drag_and_drop
    #[serde(default = "default_true", alias = "drag-drop-enabled")]
    pub drag_drop_enabled: bool,
    /// Defines additional browser arguments on Windows.
    ///
    /// ## Warning
    ///
    /// Webview instances with different browser arguments must also have different [data directories](Self::data_directory).
    ///
    /// By default wry passes `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`
    /// so if you set this, you also need to disable these components by yourself if you want.
    #[serde(default, alias = "additional-browser-args")]
    pub additional_browser_args: Option<String>,
    /// Whether or not the webview should be launched in incognito  mode.
    ///
    /// ## Platform-specific:
    ///
    /// - **Android**: Unsupported.
    #[serde(default)]
    pub incognito: bool,
    /// The proxy URL for the WebView for all network requests.
    ///
    /// Must be either a `http://` or a `socks5://` URL.
    ///
    /// ## Platform-specific
    ///
    /// - **macOS**: Requires the `macos-proxy` feature flag and only compiles for macOS 14+.
    #[serde(alias = "proxy-url")]
    pub proxy_url: Option<Url>,
    /// Whether page zooming by hotkeys is enabled
    ///
    /// ## Platform-specific:
    ///
    /// - **Windows**: Controls WebView2's [`IsZoomControlEnabled`](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2settings?view=webview2-winrt-1.0.2420.47#iszoomcontrolenabled) setting.
    /// - **MacOS / Linux**: Injects a polyfill that zooms in and out with `ctrl/command` + `-/=`,
    /// 20% in each step, ranging from 20% to 1000%. Requires `webview:allow-set-webview-zoom` permission
    ///
    /// - **Android / iOS**: Unsupported.
    #[serde(default, alias = "zoom-hotkeys-enabled")]
    pub zoom_hotkeys_enabled: bool,
    /// Whether browser extensions can be installed for the webview process
    ///
    /// ## Platform-specific:
    ///
    /// - **Windows**: Enables the WebView2 environment's [`AreBrowserExtensionsEnabled`](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/winrt/microsoft_web_webview2_core/corewebview2environmentoptions?view=webview2-winrt-1.0.2739.15#arebrowserextensionsenabled)
    /// - **MacOS / Linux / iOS / Android** - Unsupported.
    #[serde(default, alias = "browser-extensions-enabled")]
    pub browser_extensions_enabled: bool,

    /// Sets whether the custom protocols should use `https://<scheme>.localhost` instead of the default `http://<scheme>.localhost` on Windows and Android. Defaults to `false`.
    ///
    /// ## Note
    ///
    /// Using a `https` scheme will NOT allow mixed content when trying to fetch `http` endpoints and therefore will not match the behavior of the `<scheme>://localhost` protocols used on macOS and Linux.
    ///
    /// ## Warning
    ///
    /// Changing this value between releases will change the IndexedDB, cookies and localstorage location and your app will not be able to access the old data.
    #[serde(default, alias = "use-https-scheme")]
    pub use_https_scheme: bool,
    /// Enable web inspector which is usually called browser devtools. Enabled by default.
    ///
    /// This API works in **debug** builds, but requires `devtools` feature flag to enable it in **release** builds.
    ///
    /// ## Platform-specific
    ///
    /// - macOS: This will call private functions on **macOS**.
    /// - Android: Open `chrome://inspect/#devices` in Chrome to get the devtools window. Wry's `WebView` devtools API isn't supported on Android.
    /// - iOS: Open Safari > Develop > [Your Device Name] > [Your WebView] to get the devtools window.
    pub devtools: Option<bool>,

    /// Change the default background throttling behaviour.
    ///
    /// By default, browsers use a suspend policy that will throttle timers and even unload
    /// the whole tab (view) to free resources after roughly 5 minutes when a view became
    /// minimized or hidden. This will pause all tasks until the documents visibility state
    /// changes back from hidden to visible by bringing the view back to the foreground.
    ///
    /// ## Platform-specific
    ///
    /// - **Linux / Windows / Android**: Unsupported. Workarounds like a pending WebLock transaction might suffice.
    /// - **iOS**: Supported since version 17.0+.
    /// - **macOS**: Supported since version 14.0+.
    ///
    /// see <https://github.com/tauri-apps/tauri/issues/5250#issuecomment-2569380578>
    #[serde(default, alias = "background-throttling")]
    pub background_throttling: Option<BackgroundThrottlingPolicy>,
    /// Whether we should disable JavaScript code execution on the webview or not.
    #[serde(default, alias = "javascript-disabled")]
    pub javascript_disabled: bool,
    /// on macOS and iOS there is a link preview on long pressing links, this is enabled by default.
    /// see https://docs.rs/objc2-web-kit/latest/objc2_web_kit/struct.WKWebView.html#method.allowsLinkPreview
    #[serde(default = "default_true", alias = "allow-link-preview")]
    pub allow_link_preview: bool,
    /// Allows disabling the input accessory view on iOS.
    ///
    /// The accessory view is the view that appears above the keyboard when a text input element is focused.
    /// It usually displays a view with "Done", "Next" buttons.
    #[serde(
        default,
        alias = "disable-input-accessory-view",
        alias = "disable_input_accessory_view"
    )]
    pub disable_input_accessory_view: bool,
    /// Set a custom path for the webview's data directory (localStorage, cache, etc.),
    /// **relative to the local data directory (`localDataDir()`), followed by the window label**.
    ///
    /// To set absolute paths, use [`WebviewWindowBuilder::data_directory`](https://docs.rs/tauri/2/tauri/webview/struct.WebviewWindowBuilder.html#method.data_directory)
    ///
    /// This path is not affected by the `app > appDirectoriesOverride` config.
    /// To keep the webview data in an overridden directory, leave this unset (the webview then uses the app local data directory)
    /// or resolve a path from `app.path().app_local_data_dir()` and set it with `WebviewWindowBuilder::data_directory`.
    ///
    /// #### Platform-specific:
    ///
    /// - **Windows**: WebViews with different values for settings like `additionalBrowserArgs`, `browserExtensionsEnabled` or `scrollBarStyle` must have different data directories.
    /// - **macOS / iOS**: Unsupported, use `dataStoreIdentifier` instead.
    /// - **Android**: Unsupported.
    #[serde(default, alias = "data-directory")]
    pub data_directory: Option<PathBuf>,
    /// Initialize the WebView with a custom data store identifier. This can be seen as a replacement for `dataDirectory` which is unavailable in WKWebView.
    ///
    /// See <https://developer.apple.com/documentation/webkit/wkwebsitedatastore/init(foridentifier:)?language=objc>
    ///
    /// The array must contain 16 u8 numbers.
    ///
    /// #### Platform-specific:
    ///
    /// - **iOS**: Supported since version 17.0+.
    /// - **macOS**: Supported since version 14.0+.
    /// - **Windows / Linux / Android**: Unsupported.
    #[serde(default, alias = "data-store-identifier")]
    pub data_store_identifier: Option<[u8; 16]>,

    /// Specifies the native scrollbar style to use with the webview.
    /// CSS styles that modify the scrollbar are applied on top of the native appearance configured here.
    ///
    /// Defaults to `default`, which is the browser default.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**:
    ///   - `fluentOverlay` requires WebView2 Runtime version 125.0.2535.41 or higher,
    ///     and does nothing on older versions.
    ///   - This option must be given the same value for all webviews that target the same data directory.
    /// - **Linux / Android / iOS / macOS**: Unsupported. Only supports `Default` and performs no operation.
    #[serde(default, alias = "scroll-bar-style")]
    pub scroll_bar_style: ScrollBarStyle,

    /// Whether to limit navigations to App-Bound Domains.
    ///
    /// This is required to enable Service Workers in WKWebView, which are otherwise
    /// unavailable. Defaults to `false`.
    ///
    /// When this is set to `true`, the webview can only navigate to the domains listed in the
    /// `WKAppBoundDomains` array of `src-tauri/Info.ios.plist`. Add `localhost` and every
    /// [registrable domain](https://developer.mozilla.org/en-US/docs/Glossary/Registrable_domain)
    /// this webview loads to that array:
    ///
    /// ```xml
    /// <plist>
    /// <dict>
    ///     <key>WKAppBoundDomains</key>
    ///     <array>
    ///         <string>localhost</string>
    ///         <string>aregistrabledomain.example</string>
    ///     </array>
    /// </dict>
    /// </plist>
    /// ```
    ///
    /// `localhost` must be listed if any webview with this option enabled opens a local webpage,
    /// makes any localhost call, or uses the isolation pattern, because Tauri serves the
    /// application webpage, the IPC protocol and the isolation pattern iframe from the
    /// `localhost` domain.
    ///
    /// Requests served through custom URI schemes are allowed as long as they use a registrable
    /// domain listed in the `WKAppBoundDomains` array, including requests to the `localhost`
    /// domain.
    ///
    /// An entire URI scheme can be listed by adding the protocol name followed by a colon, for
    /// example `stream:` for a custom `stream` scheme (see the
    /// [streaming example](https://github.com/tauri-apps/tauri/blob/dev/examples/streaming/main.rs)).
    /// This is not covered by Apple's
    /// [App-Bound Domains announcement](https://webkit.org/blog/10882/app-bound-domains/),
    /// so it may not be accepted during App Store review.
    ///
    /// See <https://webkit.org/blog/10882/app-bound-domains/> and
    /// <https://developer.apple.com/documentation/webkit/wkwebviewconfiguration/limitsnavigationstoappbounddomains>
    /// for the official documentation on App-Bound Domains.
    ///
    /// ## Platform-specific
    ///
    /// - **iOS**: Supported since version 14.0+.
    /// - **Linux / Windows / Android / macOS:** Unsupported.
    #[serde(default, alias = "limit-navigations-to-app-bound-domains")]
    pub limit_navigations_to_app_bound_domains: bool,
    /// The name of the Android activity to create for this window.
    #[serde(default, alias = "activity-name")]
    pub activity_name: Option<String>,
    /// The name of the Android activity that is creating this webview window.
    ///
    /// This is important to determine which stack the activity will belong to.
    #[serde(default, alias = "created-by-activity-name")]
    pub created_by_activity_name: Option<String>,

    /// Sets the identifier of the scene that is requesting the new scene,
    /// establishing a relationship between the two scenes.
    ///
    /// By default the system uses the foreground scene.
    #[serde(default, alias = "requested-by-scene-identifier")]
    pub requested_by_scene_identifier: Option<String>,
    /// Controls the WebView's browser-level general autofill behavior.
    ///
    /// **This option does not disable password or credit card autofill.**
    ///
    /// When set to `false`, the WebView will not automatically populate
    /// general form fields using previously stored data such as addresses
    /// or contact information.
    ///
    /// If not specified, this is `true` by default.
    ///
    /// ## Platform-specific
    ///
    /// - **Windows**: Supported. WebView2's autofill feature (called
    ///   "Suggestions") may not honor `autocomplete="off"` on input
    ///   elements in some cases.
    /// - **Linux / Android / iOS / macOS**: Unsupported and performs no
    ///   operation.
    #[serde(default = "default_true", alias = "general-autofill-enabled")]
    pub general_autofill_enabled: bool,

    /// Enables clipboard access for the page rendered on **Linux** and **Windows**.
    ///
    /// **macOS** doesn't provide such method and is always enabled by default,
    /// but you still need to add menu item accelerators to use shortcuts.
    #[serde(default = "default_true", alias = "enable-clipboard-access")]
    pub enable_clipboard_access: bool,
    /// Controls navigation behavior for this WebView.
    ///
    /// If omitted, no custom navigation handler is installed.
    #[serde(default)]
    pub navigation_policy: Option<NavigationPolicy>,

    /// Controls requests from `window.open`, `target="_blank"`, etc.
    #[serde(default)]
    pub new_window_policy: Option<NewWindowPolicy>,
}

/// Background throttling policy.
#[derive(Debug, PartialEq, Eq, Clone, Deserialize, Serialize)]
#[serde(
    crate = "taurino_core::serde",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum BackgroundThrottlingPolicy {
    /// A policy where background throttling is disabled
    Disabled,
    /// A policy where a web view that's not in a window fully suspends tasks. This is usually the default behavior in case no policy is set.
    Suspend,
    /// A policy where a web view that's not in a window limits processing, but does not fully suspend tasks.
    Throttle,
}

/// The scrollbar style to use in the webview.
///
/// ## Platform-specific
///
/// - **Windows**: This option must be given the same value for all webviews that target the same data directory.
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
#[serde(
    crate = "taurino_core::serde",
    rename_all = "camelCase",
    deny_unknown_fields
)]
#[non_exhaustive]
pub enum ScrollBarStyle {
    #[default]
    /// The platform's native scrollbar, as rendered by the webview by default.
    ///
    /// This is the only supported value outside of Windows.
    Default,

    /// Fluent UI style overlay scrollbars. **Windows Only**
    ///
    /// Requires WebView2 Runtime version 125.0.2535.41 or higher, does nothing on older versions,
    /// see <https://learn.microsoft.com/en-us/microsoft-edge/webview2/release-notes/?tabs=dotnetcsharp#10253541>
    FluentOverlay,
}

impl Default for WebViewOptions {
    fn default() -> Self {
        Self {
            enable_clipboard_access: false,
            label: "root".to_string(),
            child: false,
            url: WebviewUrl::default(),
            user_agent: None,
            drag_drop_enabled: true,
            additional_browser_args: None,
            incognito: false,
            proxy_url: None,
            zoom_hotkeys_enabled: false,
            browser_extensions_enabled: false,
            use_https_scheme: false,
            devtools: None,
            background_throttling: None,
            javascript_disabled: false,
            allow_link_preview: true,
            disable_input_accessory_view: false,
            data_directory: None,
            data_store_identifier: None,
            scroll_bar_style: ScrollBarStyle::default(),
            limit_navigations_to_app_bound_domains: false,
            activity_name: None,
            created_by_activity_name: None,
            requested_by_scene_identifier: None,
            general_autofill_enabled: true,
            navigation_policy: None,
            new_window_policy: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(crate = "taurino_core::serde", rename_all = "camelCase", tag = "type")]
pub enum NavigationPolicy {
    /// Allow every navigation.
    AllowAll,

    /// Deny every navigation.
    DenyAll,

    /// Only allow navigation to the same origin as the initial WebView URL.
    SameOrigin,

    /// Allow navigation only when one of the configured rules matches.
    AllowList { rules: Vec<NavigationRule> },

    /// Deny navigation when one of the configured rules matches.
    BlockList { rules: Vec<NavigationRule> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    crate = "taurino_core::serde",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub struct NavigationRule {
    /// Optional URL scheme, e.g. "https".
    #[serde(default)]
    pub scheme: Option<String>,

    /// Hostname to match, e.g. "example.com".
    pub host: String,

    /// Optional port to match.
    #[serde(default)]
    pub port: Option<u16>,

    /// Whether subdomains should be accepted.
    ///
    /// Example:
    /// host = "example.com"
    /// include_subdomains = true
    ///
    /// Matches:
    /// - example.com
    /// - api.example.com
    /// - auth.api.example.com
    #[serde(default)]
    pub include_subdomains: bool,
}

impl NavigationPolicy {
    pub fn allows(&self, initial_url: Option<&Url>, target_url: &Url) -> bool {
        match self {
            Self::AllowAll => true,

            Self::DenyAll => false,

            Self::SameOrigin => {
                let Some(initial_url) = initial_url else {
                    return false;
                };

                same_origin(initial_url, target_url)
            }

            Self::AllowList { rules } => rules.iter().any(|rule| rule.matches(target_url)),

            Self::BlockList { rules } => !rules.iter().any(|rule| rule.matches(target_url)),
        }
    }
}

impl NavigationRule {
    pub fn matches(&self, url: &Url) -> bool {
        if let Some(expected_scheme) = &self.scheme {
            if !url.scheme().eq_ignore_ascii_case(expected_scheme) {
                return false;
            }
        }

        let Some(actual_host) = url.host_str() else {
            return false;
        };

        if !self.matches_host(actual_host) {
            return false;
        }

        if let Some(expected_port) = self.port {
            if url.port_or_known_default() != Some(expected_port) {
                return false;
            }
        }

        true
    }

    fn matches_host(&self, actual_host: &str) -> bool {
        if actual_host.eq_ignore_ascii_case(&self.host) {
            return true;
        }

        if !self.include_subdomains {
            return false;
        }

        let actual_host = actual_host.to_ascii_lowercase();
        let expected_host = self.host.to_ascii_lowercase();

        actual_host.ends_with(&format!(".{expected_host}"))
    }
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

// ============================================================================
// New window configuration
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(crate = "taurino_core::serde", rename_all = "camelCase", tag = "type")]
pub enum NewWindowPolicy {
    /// Let the WebView runtime handle the request normally.
    Allow,

    /// Reject every new-window request.
    Deny,

    /// Evaluate requests against configured rules.
    Rules {
        rules: Vec<NewWindowRule>,

        #[serde(default)]
        fallback: NewWindowAction,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    crate = "taurino_core::serde",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub struct NewWindowRule {
    /// Match condition.
    pub request: NewWindowRequestMatcher,

    /// Response returned when the request matches.
    pub response: NewWindowAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    crate = "taurino_core::serde",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub struct NewWindowRequestMatcher {
    /// Optional URL scheme, e.g. "https".
    #[serde(default)]
    pub scheme: Option<String>,

    /// Optional host, e.g. "example.com".
    #[serde(default)]
    pub host: Option<String>,

    /// Include subdomains when matching `host`.
    #[serde(default)]
    pub include_subdomains: bool,

    /// Optional port.
    #[serde(default)]
    pub port: Option<u16>,

    /// Optional path prefix.
    #[serde(default)]
    pub path_prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(crate = "taurino_core::serde", rename_all = "camelCase", tag = "type")]
pub enum NewWindowAction {
    /// Allow WRY to create the requested window.
    Allow,

    /// Reject the request.
    #[default]
    Deny,

    /// Create a new engine-controlled window using the supplied configuration.
    Create { window: Box<WindowOptions> },
}

// ============================================================================
// Matching
// ============================================================================

impl NewWindowRequestMatcher {
    pub fn matches(&self, url: &Url) -> bool {
        if let Some(expected_scheme) = &self.scheme {
            if !url.scheme().eq_ignore_ascii_case(expected_scheme) {
                return false;
            }
        }

        if let Some(expected_host) = &self.host {
            let Some(actual_host) = url.host_str() else {
                return false;
            };

            if !host_matches(actual_host, expected_host, self.include_subdomains) {
                return false;
            }
        }

        if let Some(expected_port) = self.port {
            if url.port_or_known_default() != Some(expected_port) {
                return false;
            }
        }

        if let Some(path_prefix) = &self.path_prefix {
            if !url.path().starts_with(path_prefix) {
                return false;
            }
        }

        true
    }
}

fn host_matches(actual: &str, expected: &str, include_subdomains: bool) -> bool {
    if actual.eq_ignore_ascii_case(expected) {
        return true;
    }

    if !include_subdomains {
        return false;
    }

    let actual = actual.to_ascii_lowercase();
    let expected = expected.to_ascii_lowercase();

    actual.ends_with(&format!(".{expected}"))
}

// ============================================================================
// Policy evaluation
// ============================================================================

impl NewWindowPolicy {
    pub fn evaluate(&self, url: &Url) -> NewWindowAction {
        match self {
            Self::Allow => NewWindowAction::Allow,

            Self::Deny => NewWindowAction::Deny,

            Self::Rules { rules, fallback } => {
                for rule in rules {
                    if rule.request.matches(url) {
                        return rule.response.clone();
                    }
                }

                fallback.clone()
            }
        }
    }
}
