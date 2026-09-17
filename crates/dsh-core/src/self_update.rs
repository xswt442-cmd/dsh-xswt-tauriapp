//! The shell's own updates: what this application's GitHub releases say.
//!
//! Deliberately separate from [`crate::updates`], which is about the **dsh
//! runtime**. Two different products from two different sources — dsh is
//! installed from npm, this application is released from its own GitHub
//! repository — so mixing them would mean one version comparison answering two
//! unrelated questions.
//!
//! The check is advisory and never fatal: a rate-limited API, an offline
//! machine or a release with nothing installable all end in "no prompt", not in
//! a failed launch.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The repository this application is released from.
pub const REPO: &str = "xswt442-cmd/dsh-xswt-tauriapp";
/// Default endpoint for the newest release.
pub const DEFAULT_RELEASES_API: &str =
    "https://api.github.com/repos/xswt442-cmd/dsh-xswt-tauriapp/releases/latest";
/// Where a human goes when the release carries no installer for this machine.
pub const RELEASES_PAGE: &str = "https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/latest";
/// GitHub answers 403 to any request without one of these.
const USER_AGENT: &str = "dsh-xswt-tauriapp";
/// How long the API may take.
const API_TIMEOUT: Duration = Duration::from_secs(15);
/// How long an installer download may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// One file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Asset {
    /// File name, as attached.
    pub name: String,
    /// Where to fetch it.
    pub url: String,
}

/// The newest release, as this application understands it.
#[derive(Debug, Clone, Serialize)]
pub struct Release {
    /// Version with any `v` prefix removed, so it compares as semver.
    pub version: String,
    /// The tag exactly as published (`v0.0.7`).
    pub tag: String,
    /// The release page, for a human.
    pub page: String,
    /// Everything attached.
    pub assets: Vec<Asset>,
}

/// A newer release, and what this machine would do about it.
#[derive(Debug, Clone, Serialize)]
pub struct Available {
    /// The newer version.
    pub version: String,
    /// The release page.
    pub page: String,
    /// The installer to hand to the operating system, when one fits.
    ///
    /// `None` means the release carries nothing this machine can install — the
    /// page is then the only honest answer, and the UI says so.
    pub installer: Option<Asset>,
    /// The release's `SHA256SUMS`, which the download is checked against.
    pub checksums: Option<Asset>,
}

/// Strip the decoration a tag carries so it compares as a version.
///
/// GitHub tags here look like `v0.0.7`, and `semver` rejects the leading `v`.
/// Getting this wrong is not cosmetic: the comparison in [`crate::updates`]
/// falls back to string inequality for unparseable input, so `v0.0.7` against
/// an installed `0.0.7` would read as "newer" and prompt for the release the
/// user is already running.
pub fn normalise_version(raw: &str) -> String {
    raw.trim()
        .trim_start_matches(['v', 'V', '='])
        .trim()
        .to_string()
}

/// The endpoint to ask, overridable for tests and mirrors.
pub fn releases_api() -> String {
    std::env::var("DSH_SHELL_RELEASES_API")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASES_API.to_string())
}

/// The file names this machine can install, most preferred first.
///
/// Only installers that hand over to the operating system's own installer are
/// listed. A bare executable would run without the dependencies, shortcuts and
/// uninstaller the packages carry — and on Linux it would simply not start,
/// since this application links the system WebKitGTK.
pub fn installer_suffixes(os: &str, arch: &str) -> &'static [&'static str] {
    match (os, arch) {
        ("windows", "x86_64") => &["_x64-setup.exe"],
        ("macos", "aarch64") => &["_aarch64.dmg"],
        ("macos", "x86_64") => &["_x64.dmg"],
        ("linux", "x86_64") => &["_amd64.deb"],
        // No arm64 Linux bundle is built, and an unknown platform is no guess.
        _ => &[],
    }
}

/// The installer in `assets` that fits `os`/`arch`, if any.
pub fn pick_installer(assets: &[Asset], os: &str, arch: &str) -> Option<Asset> {
    installer_suffixes(os, arch).iter().find_map(|suffix| {
        assets
            .iter()
            .find(|asset| asset.name.ends_with(suffix))
            .cloned()
    })
}

/// The `SHA256SUMS` attachment, if the release carries one.
pub fn pick_checksums(assets: &[Asset]) -> Option<Asset> {
    assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")
        .cloned()
}

/// Whether `release` is newer than `current`, and what would be done about it.
///
/// Both sides must parse as semver, and that is the point: [`crate::updates`]
/// falls back to "a different string is a newer version" when parsing fails,
/// which is the right guess for a version feed and the wrong one for an
/// updater. Here an unorderable tag means "do not offer anything".
pub fn available(current: &str, release: &Release) -> Option<Available> {
    let candidate = semver::Version::parse(&normalise_version(&release.version)).ok()?;
    let installed = semver::Version::parse(&normalise_version(current)).ok()?;
    if candidate <= installed {
        return None;
    }
    Some(Available {
        version: release.version.clone(),
        page: release.page.clone(),
        installer: pick_installer(
            &release.assets,
            std::env::consts::OS,
            std::env::consts::ARCH,
        ),
        checksums: pick_checksums(&release.assets),
    })
}

/// The shape of `GET /repos/{owner}/{repo}/releases/latest`, reduced.
#[derive(Debug, Deserialize)]
struct LatestRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<LatestAsset>,
}

#[derive(Debug, Deserialize)]
struct LatestAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
}

/// Parse the API's answer. Pure, so the field names are pinned by a test.
pub fn parse_latest(body: &str) -> Result<Release, String> {
    let parsed: LatestRelease =
        serde_json::from_str(body).map_err(|error| format!("无法解析发布信息：{error}"))?;
    if parsed.tag_name.is_empty() {
        return Err("发布信息里没有 tag_name".to_string());
    }
    let version = normalise_version(&parsed.tag_name);
    let page = if parsed.html_url.is_empty() {
        RELEASES_PAGE.to_string()
    } else {
        parsed.html_url
    };
    let assets = parsed
        .assets
        .into_iter()
        .filter(|asset| !asset.name.is_empty() && !asset.browser_download_url.is_empty())
        .map(|asset| Asset {
            name: asset.name,
            url: asset.browser_download_url,
        })
        .collect();
    Ok(Release {
        version,
        tag: parsed.tag_name,
        page,
        assets,
    })
}

/// Ask GitHub for the newest release.
pub fn fetch_latest() -> Result<Release, String> {
    let response = ureq::AgentBuilder::new()
        .timeout(API_TIMEOUT)
        .build()
        .get(&releases_api())
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call();
    let body = match response {
        Ok(response) => response.into_string().unwrap_or_default(),
        // A 404 means no release yet, which is not an error worth surfacing.
        Err(ureq::Error::Status(404, _)) => return Err("还没有发布任何版本".to_string()),
        Err(ureq::Error::Status(code, response)) => {
            let detail = response.into_string().unwrap_or_default();
            return Err(format!("发布接口返回 {code}：{}", detail.trim()));
        }
        Err(error) => return Err(format!("无法访问发布接口：{error}")),
    };
    parse_latest(&body)
}

/// Check, then decide, in one step.
pub fn check(current: &str) -> Result<Option<Available>, String> {
    Ok(available(current, &fetch_latest()?))
}

/// Parse `sha256sum` output: one `<hex>  <name>` per line.
pub fn parse_sha256sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            // `sha256sum -b` marks binary mode with a `*` before the name.
            Some((
                name.trim_start_matches('*').to_string(),
                hash.to_ascii_lowercase(),
            ))
        })
        .collect()
}

/// The SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Fetch a URL's body, with the user agent GitHub requires.
fn fetch_bytes(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let response = ureq::AgentBuilder::new()
        .timeout(timeout)
        .build()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => format!("下载失败：HTTP {code}"),
            other => format!("下载失败：{other}"),
        })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取下载内容失败：{error}"))?;
    Ok(bytes)
}

/// Download the installer into `dir`, verified against the release's checksums.
///
/// Verification is required rather than best-effort. This file is about to be
/// handed to the operating system, and TLS only secures the transport — the
/// checksum is what catches a truncated or swapped payload at the far end. A
/// release without `SHA256SUMS` is refused for that reason.
pub fn download_installer(
    installer: &Asset,
    checksums: Option<&Asset>,
    dir: &Path,
) -> Result<PathBuf, String> {
    let sums = checksums
        .ok_or_else(|| "这个版本没有提供 SHA256SUMS，已拒绝下载未经校验的安装包。".to_string())?;
    let bytes = fetch_bytes(&installer.url, DOWNLOAD_TIMEOUT)?;

    let listing = fetch_bytes(&sums.url, API_TIMEOUT)?;
    let listing = String::from_utf8_lossy(&listing);
    let expected = parse_sha256sums(&listing)
        .into_iter()
        .find(|(name, _)| name == &installer.name)
        .map(|(_, hash)| hash)
        .ok_or_else(|| format!("SHA256SUMS 里没有 {}。", installer.name))?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "{} 校验失败：期望 {expected}，实际 {actual}。已丢弃。",
            installer.name
        ));
    }

    std::fs::create_dir_all(dir).map_err(|error| format!("创建下载目录失败：{error}"))?;
    let path = dir.join(&installer.name);
    std::fs::write(&path, &bytes)
        .map_err(|error| format!("写入 {} 失败：{error}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            url: format!("https://example.invalid/{name}"),
        }
    }

    /// A release shaped like the API's answer for this repository.
    fn release(tag: &str, names: &[&str]) -> Release {
        Release {
            version: normalise_version(tag),
            tag: tag.to_string(),
            page: "https://example.invalid/release".to_string(),
            assets: names.iter().map(|name| asset(name)).collect(),
        }
    }

    #[test]
    fn a_tag_loses_its_decoration() {
        assert_eq!(normalise_version("v0.0.7"), "0.0.7");
        assert_eq!(normalise_version("V0.0.7"), "0.0.7");
        assert_eq!(normalise_version(" 0.0.7 "), "0.0.7");
        assert_eq!(normalise_version("0.0.7"), "0.0.7");
    }

    #[test]
    fn the_installed_release_is_never_offered_back() {
        // The trap this module exists to avoid: the tag carries a `v` and the
        // installed version does not, and the shared comparator treats two
        // different strings as "newer" when either fails to parse.
        let latest = release("v0.0.7", &["dsh-xswt-tauriapp_0.0.7_amd64.deb"]);
        assert!(available("0.0.7", &latest).is_none());
        assert!(available("v0.0.7", &latest).is_none());
        // And an older release is not offered over a newer install either.
        assert!(available("0.0.9", &latest).is_none());
    }

    #[test]
    fn a_newer_release_is_offered_with_what_fits() {
        let latest = release(
            "v0.0.8",
            &[
                "dsh-xswt-tauriapp_0.0.8_amd64.deb",
                "dsh-xswt-tauriapp_0.0.8_x64-setup.exe",
                "SHA256SUMS",
            ],
        );
        let offered = available("0.0.7", &latest).expect("0.0.8 is newer than 0.0.7");
        assert_eq!(offered.version, "0.0.8");
        assert_eq!(
            offered.checksums.as_ref().map(|asset| asset.name.as_str()),
            Some("SHA256SUMS")
        );
    }

    #[test]
    fn an_unorderable_tag_offers_nothing() {
        // "nightly" is not a version this application can reason about, so the
        // only safe answer is silence rather than a prompt on every launch.
        let latest = release("nightly", &["dsh-xswt-tauriapp_0.0.8_amd64.deb"]);
        assert!(available("0.0.7", &latest).is_none());
        let latest = release("v0.0.8", &[]);
        assert!(available("not-a-version", &latest).is_none());
    }

    #[test]
    fn every_platform_gets_the_installer_that_fits_it() {
        let names = [
            "dsh-xswt-tauriapp_0.0.8_x64-setup.exe",
            "dsh-xswt-tauriapp_0.0.8_x64.dmg",
            "dsh-xswt-tauriapp_0.0.8_aarch64.dmg",
            "dsh-xswt-tauriapp_0.0.8_amd64.deb",
            "dsh-xswt-tauriapp-0.0.8-1.x86_64.rpm",
            "dsh-xswt-tauriapp_0.0.8_amd64.AppImage",
            "SHA256SUMS",
        ];
        let assets: Vec<Asset> = names.iter().map(|name| asset(name)).collect();

        let pick = |os, arch| pick_installer(&assets, os, arch).map(|asset| asset.name);
        assert_eq!(
            pick("windows", "x86_64").as_deref(),
            Some("dsh-xswt-tauriapp_0.0.8_x64-setup.exe")
        );
        assert_eq!(
            pick("macos", "aarch64").as_deref(),
            Some("dsh-xswt-tauriapp_0.0.8_aarch64.dmg")
        );
        assert_eq!(
            pick("macos", "x86_64").as_deref(),
            Some("dsh-xswt-tauriapp_0.0.8_x64.dmg")
        );
        // The deb, not the AppImage: it carries the dependencies, the shortcut
        // and the uninstaller, and it is 3 MB rather than 80 MB.
        assert_eq!(
            pick("linux", "x86_64").as_deref(),
            Some("dsh-xswt-tauriapp_0.0.8_amd64.deb")
        );
        // Platforms we do not build for get nothing rather than a wrong file.
        assert_eq!(pick("linux", "aarch64"), None);
        assert_eq!(pick("freebsd", "x86_64"), None);
        // The checksums are never an installer.
        assert!(pick_installer(&assets, "windows", "x86_64") != pick_checksums(&assets));
    }

    #[test]
    fn the_api_answer_is_parsed_from_the_fields_github_actually_sends() {
        let body = r#"{
          "tag_name": "v0.0.8",
          "html_url": "https://github.com/xswt442-cmd/dsh-xswt-tauriapp/releases/tag/v0.0.8",
          "assets": [
            {"name": "dsh-xswt-tauriapp_0.0.8_amd64.deb",
             "browser_download_url": "https://example.invalid/deb"},
            {"name": "SHA256SUMS", "browser_download_url": "https://example.invalid/sums"},
            {"name": "", "browser_download_url": "https://example.invalid/nameless"}
          ]
        }"#;
        let release = parse_latest(body).expect("parses");
        assert_eq!(release.version, "0.0.8");
        assert_eq!(release.tag, "v0.0.8");
        assert!(release.page.ends_with("/v0.0.8"));
        // The nameless attachment is dropped rather than becoming a broken offer.
        assert_eq!(release.assets.len(), 2);
    }

    #[test]
    fn a_malformed_answer_is_an_error_and_not_a_panic() {
        assert!(parse_latest("").is_err());
        assert!(parse_latest("{}").is_err(), "no tag_name");
        assert!(parse_latest("not json at all").is_err());
    }

    #[test]
    fn checksums_are_read_the_way_sha256sum_writes_them() {
        let text = "abc123  dsh-xswt-tauriapp_0.0.8_amd64.deb\n\
                    DEF456 *dsh-xswt-tauriapp_0.0.8_x64-setup.exe\n\
                    \n\
                    garbage-without-a-second-column\n";
        let parsed = parse_sha256sums(text);
        assert_eq!(
            parsed,
            vec![
                (
                    "dsh-xswt-tauriapp_0.0.8_amd64.deb".to_string(),
                    "abc123".to_string()
                ),
                (
                    "dsh-xswt-tauriapp_0.0.8_x64-setup.exe".to_string(),
                    "def456".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_hash_is_computed_the_way_the_release_publishes_it() {
        // The published value for an empty file, which is the one case with a
        // number everybody agrees on.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(sha256_hex(b"abc").len(), 64);
    }
}
