//! pilot's browser identity: Chrome on the operating system pilot runs on.
//!
//! Moli's default identity is Chrome on Windows. Bot defenses compare the
//! operating system a browser claims with its TCP/IP stack's fingerprint,
//! which names the real one, so pilot claims the system it runs on. Like
//! Chrome, the User-Agent carries only the major version
//! (`Chrome/156.0.0.0`); the full version is in the client hints.

use moli_browser_profile::{
    BrowserIdentityProfile, BrowserUserAgentMetadataOverride, DEFAULT_ACCEPT_LANGUAGE,
    DEFAULT_USER_AGENT,
};

/// Reported when SystemVersion.plist cannot be read.
const FALLBACK_MACOS_VERSION: &str = "15.0.0";

/// The operating system pilot reports, with the version Chrome gives in
/// `Sec-CH-UA-Platform-Version`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostOs {
    MacOs { version: String },
    Linux { kernel: String },
    Windows,
}

impl HostOs {
    /// The system this process runs on.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            let version = std::fs::read_to_string(
                "/System/Library/CoreServices/SystemVersion.plist",
            )
            .ok()
            .and_then(|plist| macos_version_from_plist(&plist))
            .unwrap_or_else(|| {
                tracing::warn!(
                    "⚠️ could not read the macOS version; pilot reports {FALLBACK_MACOS_VERSION}"
                );
                FALLBACK_MACOS_VERSION.to_owned()
            });
            Self::MacOs { version }
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .ok()
                .and_then(|release| kernel_version(&release))
                .unwrap_or_default();
            Self::Linux { kernel }
        }
    }
}

/// Chrome's identity on `host`, on an ARM CPU when `arm`.
pub fn chrome_identity(host: &HostOs, arm: bool) -> BrowserIdentityProfile {
    let moli = BrowserIdentityProfile::new(DEFAULT_USER_AGENT, DEFAULT_ACCEPT_LANGUAGE);
    let (system, navigator_platform, platform, platform_version) = match host {
        // Chrome on Apple silicon still says Intel and MacIntel.
        HostOs::MacOs { version } => (
            "Macintosh; Intel Mac OS X 10_15_7",
            "MacIntel",
            "macOS",
            version.clone(),
        ),
        HostOs::Linux { kernel } if arm => (
            "X11; Linux aarch64",
            "Linux aarch64",
            "Linux",
            kernel.clone(),
        ),
        HostOs::Linux { kernel } => ("X11; Linux x86_64", "Linux x86_64", "Linux", kernel.clone()),
        HostOs::Windows => (
            "Windows NT 10.0; Win64; x64",
            moli.navigator_platform(),
            moli.platform(),
            moli.platform_version().to_owned(),
        ),
    };
    let user_agent = format!(
        "Mozilla/5.0 ({system}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
        moli.major_version()
    );
    BrowserIdentityProfile::from_devtools_override(
        &moli,
        user_agent,
        None,
        Some(navigator_platform.to_owned()),
        Some(BrowserUserAgentMetadataOverride {
            brands: None,
            full_version_list: None,
            full_version: Some(moli.full_version().to_owned()),
            platform: platform.to_owned(),
            platform_version,
            architecture: if arm { "arm" } else { "x86" }.to_owned(),
            model: String::new(),
            mobile: false,
            bitness: Some("64".to_owned()),
            wow64: Some(false),
            form_factors: None,
        }),
    )
}

/// `ProductVersion` from macOS's SystemVersion.plist, as three components.
fn macos_version_from_plist(plist: &str) -> Option<String> {
    let (_, after_key) = plist.split_once("<key>ProductVersion</key>")?;
    let (_, after_tag) = after_key.split_once("<string>")?;
    let (version, _) = after_tag.split_once("</string>")?;
    three_components(version.trim())
}

/// The numeric part of a Linux kernel release, such as `6.8.0`.
fn kernel_version(release: &str) -> Option<String> {
    let numeric: String = release
        .trim()
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    three_components(numeric.trim_end_matches('.'))
}

/// `major[.minor[.patch]]` padded to three components.
fn three_components(version: &str) -> Option<String> {
    let mut parts: Vec<&str> = version.split('.').collect();
    let numeric = parts
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|character| character.is_ascii_digit()));
    if !numeric || parts.len() > 3 {
        return None;
    }
    parts.resize(3, "0");
    Some(parts.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome_major() -> String {
        BrowserIdentityProfile::new(DEFAULT_USER_AGENT, DEFAULT_ACCEPT_LANGUAGE)
            .major_version()
            .to_owned()
    }

    fn chrome_full_version() -> String {
        BrowserIdentityProfile::new(DEFAULT_USER_AGENT, DEFAULT_ACCEPT_LANGUAGE)
            .full_version()
            .to_owned()
    }

    #[test]
    fn on_a_mac_pilot_is_chrome_on_macos() {
        let identity = chrome_identity(
            &HostOs::MacOs {
                version: "26.2.0".to_owned(),
            },
            true,
        );
        assert_eq!(
            identity.user_agent(),
            format!(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
                chrome_major()
            )
        );
        assert_eq!(identity.navigator_platform(), "MacIntel");
        assert_eq!(identity.platform(), "macOS");
        assert_eq!(identity.platform_version(), "26.2.0");
        assert_eq!(identity.architecture(), "arm");
        assert_eq!(identity.bitness(), "64");
        assert!(!identity.wow64());
        assert!(!identity.mobile());
        assert_eq!(identity.full_version(), chrome_full_version());
        assert!(
            identity
                .sec_ch_ua_value()
                .is_some_and(|brands| brands.contains("Google Chrome")),
            "client hints still name Chrome's brands"
        );
    }

    #[test]
    fn on_linux_pilot_is_chrome_on_linux() {
        let identity = chrome_identity(
            &HostOs::Linux {
                kernel: "6.8.0".to_owned(),
            },
            false,
        );
        assert!(
            identity
                .user_agent()
                .starts_with("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36"),
            "{}",
            identity.user_agent()
        );
        assert_eq!(identity.navigator_platform(), "Linux x86_64");
        assert_eq!(identity.platform(), "Linux");
        assert_eq!(identity.platform_version(), "6.8.0");
        assert_eq!(identity.architecture(), "x86");
        let arm = chrome_identity(
            &HostOs::Linux {
                kernel: "6.8.0".to_owned(),
            },
            true,
        );
        assert!(arm.user_agent().contains("(X11; Linux aarch64)"));
        assert_eq!(arm.navigator_platform(), "Linux aarch64");
        assert_eq!(arm.architecture(), "arm");
    }

    #[test]
    fn on_windows_pilot_keeps_moli_windows_identity_with_chromes_reduced_user_agent() {
        let identity = chrome_identity(&HostOs::Windows, false);
        assert_eq!(
            identity.user_agent(),
            format!(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
                chrome_major()
            )
        );
        assert_eq!(identity.navigator_platform(), "Win32");
        assert_eq!(identity.platform(), "Windows");
        assert_eq!(identity.platform_version(), "19.0.0");
        assert_eq!(identity.architecture(), "x86");
        assert_eq!(identity.full_version(), chrome_full_version());
    }

    #[test]
    fn the_macos_version_is_read_from_system_version_plist() {
        let plist = "<plist version=\"1.0\">\n<dict>\n\t<key>ProductName</key>\n\t<string>macOS</string>\n\t<key>ProductVersion</key>\n\t<string>26.2</string>\n</dict>\n</plist>\n";
        assert_eq!(macos_version_from_plist(plist).as_deref(), Some("26.2.0"));
        let patched = plist.replace("26.2", "15.6.1");
        assert_eq!(
            macos_version_from_plist(&patched).as_deref(),
            Some("15.6.1")
        );
        assert_eq!(macos_version_from_plist("<plist/>"), None);
    }

    #[test]
    fn the_kernel_version_keeps_only_its_numbers() {
        assert_eq!(kernel_version("6.8.0-45-generic").as_deref(), Some("6.8.0"));
        assert_eq!(kernel_version("6.10\n").as_deref(), Some("6.10.0"));
        assert_eq!(kernel_version("unknown"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_mac_reports_its_own_macos_version() {
        let HostOs::MacOs { version } = HostOs::current() else {
            panic!("a Mac is macOS");
        };
        assert_eq!(version.split('.').count(), 3, "{version}");
        assert!(
            version.split('.').all(|part| part.parse::<u32>().is_ok()),
            "{version}"
        );
    }
}
