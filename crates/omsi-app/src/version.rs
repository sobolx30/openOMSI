//! This build's version, the project's page, and a link opened in the system's browser.
//!
//! Nothing here uses the network: `open_url` hands a link to the player's own browser when they
//! press a button (the crash dialog's "Report on GitHub", Settings → the project's page).

/// This edition's repository on GitHub: where the crash dialog opens a prefilled issue and Settings → About links to.
pub const REPO_URL: &str = "https://github.com/sobolx30/openOMSI-Sobol3D-Edition";

/// This build's version.
pub fn current_version() -> &'static str {
    crate::startup::VERSION
}

/// Open a web page in the system's browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "android")]
    crate::android::open_url(url);
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_version_is_a_number_with_dots() {
        let v = super::current_version();
        assert!(!v.is_empty() && v.chars().next().unwrap().is_ascii_digit(), "{v}");
    }
}
