//! Which dsh versions exist, how they split across release channels, and which
//! ones the user asked not to be reminded about.
//!
//! dsh publishes on three tracks that do not share a single monotonic line:
//! plain releases, `-rc.N` candidates and `-alpha.N` previews. The registry's
//! `dist-tags` only expose one name per track (`latest`, `next`, `alpha`) and
//! `latest` is itself usually a release candidate, so the channels are derived
//! from the version strings rather than from the tags.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Where released versions are read from.
pub const DEFAULT_REGISTRY_URL: &str = "https://registry.npmjs.org/@deepseek-ai/dsh";
/// How many versions of each channel the report carries.
pub const VERSIONS_PER_CHANNEL: usize = 6;

/// The npm package this shell wraps.
pub const PACKAGE: &str = "@deepseek-ai/dsh";

/// Channel identifiers, in the order they are shown.
pub const CHANNELS: [(&str, &str); 3] = [("stable", "正式版"), ("rc", "RC"), ("alpha", "Alpha")];

/// The registry document, reduced to the parts this shell reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RegistryDoc {
    /// Every published version, keyed by version string.
    #[serde(default)]
    pub versions: BTreeMap<String, serde_json::Value>,
    /// npm dist-tags (`latest`, `next`, `alpha`, …).
    #[serde(default, rename = "dist-tags")]
    pub dist_tags: BTreeMap<String, String>,
    /// Publish timestamps, keyed by version string.
    #[serde(default)]
    pub time: BTreeMap<String, String>,
}

/// One published version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    /// Version string, e.g. `0.1.5-rc.2`.
    pub version: String,
    /// Channel this version belongs to: `stable`, `rc` or `alpha`.
    pub channel: String,
    /// Publish timestamp, when the registry reports one.
    pub published: Option<String>,
}

/// One release channel, as displayed in a column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelListing {
    /// `stable` | `rc` | `alpha`.
    pub id: String,
    /// Human label for the column heading.
    pub label: String,
    /// The newest version on this channel — offered as the channel's upgrade.
    pub latest: Option<VersionEntry>,
    /// Newest-first versions on this channel, capped at [`VERSIONS_PER_CHANNEL`].
    pub versions: Vec<VersionEntry>,
    /// Every `dist-tags` name that currently points into this channel, sorted.
    /// Collected per channel rather than per version because a tag may lag
    /// behind the newest release on its own track.
    pub dist_tags: Vec<String>,
    /// Whether `latest` is newer than the installed version.
    pub newer: bool,
}

/// Everything the update dialog needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateReport {
    /// The installed version.
    pub current: String,
    /// The three channels, in display order.
    pub channels: Vec<ChannelListing>,
    /// The newest version overall that is newer than `current`.
    pub candidate: Option<VersionEntry>,
    /// Whether `candidate` is on the user's do-not-remind list.
    pub candidate_dismissed: bool,
}

impl UpdateReport {
    /// Whether the launch popup should appear: a newer version exists and the
    /// user has not opted out of this exact version.
    pub fn should_prompt(&self) -> bool {
        self.candidate.is_some() && !self.candidate_dismissed
    }
}

/// The channel a version string belongs to.
pub fn channel_of(version: &str) -> &'static str {
    match semver::Version::parse(version) {
        Ok(parsed) => match parsed.pre.as_str() {
            "" => "stable",
            pre if pre.starts_with("rc") => "rc",
            pre if pre.starts_with("alpha") => "alpha",
            _ => "other",
        },
        Err(_) => "other",
    }
}

/// How stable a channel is, higher being more settled.
///
/// Used to keep the automatic prompt from offering a *less* stable track than
/// the one already installed: an rc user should hear about stable and rc
/// releases, never be nudged onto alpha by a popup.
pub fn stability_rank(channel: &str) -> u8 {
    match channel {
        "stable" => 3,
        "rc" => 2,
        "alpha" => 1,
        _ => 0,
    }
}

/// Parse a version for ordering; unparseable strings sort last.
fn order_key(version: &str) -> Option<semver::Version> {
    semver::Version::parse(version).ok()
}

/// `true` when `candidate` is a strictly newer release than `current`.
///
/// Falls back to string comparison when either side does not parse, so an
/// exotic version never silently disables the update check.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (order_key(candidate), order_key(current)) {
        (Some(a), Some(b)) => a > b,
        _ => candidate != current,
    }
}

/// Build the report for the three channels from a registry document.
pub fn build_report(current: &str, doc: &RegistryDoc, dismissed: &DismissStore) -> UpdateReport {
    let mut grouped: BTreeMap<&str, Vec<VersionEntry>> = BTreeMap::new();
    for version in doc.versions.keys() {
        let channel = channel_of(version);
        if channel == "other" {
            continue;
        }
        grouped.entry(channel).or_default().push(VersionEntry {
            version: version.clone(),
            channel: channel.to_string(),
            published: doc.time.get(version).cloned(),
        });
    }

    let mut channels = Vec::with_capacity(CHANNELS.len());

    for (id, label) in CHANNELS {
        let mut versions = grouped.remove(id).unwrap_or_default();
        // Newest first; versions that fail to parse sink to the bottom rather
        // than being dropped.
        versions.sort_by(
            |a, b| match (order_key(&a.version), order_key(&b.version)) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.version.cmp(&a.version),
            },
        );

        let latest = versions.first().cloned();

        let dist_tags = doc
            .dist_tags
            .iter()
            .filter(|(_, target)| channel_of(target) == id)
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();

        let newer = latest
            .as_ref()
            .is_some_and(|l| is_newer(&l.version, current));
        versions.truncate(VERSIONS_PER_CHANNEL);
        channels.push(ChannelListing {
            id: id.to_string(),
            label: label.to_string(),
            latest,
            versions,
            dist_tags,
            newer,
        });
    }

    // The candidate is what the launch popup offers. Only channels at least as
    // stable as the installed one are considered, so an rc user is never
    // nudged onto alpha by an automatic prompt — alpha stays a deliberate
    // choice from the dialog's third column.
    let installed_rank = stability_rank(channel_of(current));
    let candidate = channels
        .iter()
        .filter(|listing| stability_rank(&listing.id) >= installed_rank)
        .filter_map(|listing| listing.latest.clone())
        .filter(|entry| is_newer(&entry.version, current))
        .max_by(
            |a, b| match (order_key(&a.version), order_key(&b.version)) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => a.version.cmp(&b.version),
            },
        );

    let candidate_dismissed = candidate
        .as_ref()
        .is_some_and(|entry| dismissed.is_dismissed(&entry.version));

    UpdateReport {
        current: current.to_string(),
        channels,
        candidate,
        candidate_dismissed,
    }
}

/// The registry URL, overridable for tests and mirrors.
pub fn registry_url() -> String {
    std::env::var("DSH_TAURI_REGISTRY")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_string())
}

/// Fetch the published versions. Blocking.
pub fn fetch_registry(url: &str) -> Result<RegistryDoc, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();
    match agent.get(url).call() {
        Ok(response) => response
            .into_json::<RegistryDoc>()
            .map_err(|error| format!("解析 registry 响应失败：{error}")),
        Err(ureq::Error::Status(code, _)) => Err(format!("registry 返回 HTTP {code}")),
        Err(error) => Err(format!("无法访问 registry：{error}")),
    }
}

/// Fetch and classify in one step.
pub fn check(current: &str, dismissed: &DismissStore) -> Result<UpdateReport, String> {
    let doc = fetch_registry(&registry_url())?;
    Ok(build_report(current, &doc, dismissed))
}

/// The argv that installs one version globally, or why it is not a version.
///
/// The listing is where these strings come from, but the value that reaches this
/// function has been through a page and back, so it is checked rather than
/// trusted. It ends up as an npm argument, and only a version belongs there — a
/// rejected one costs a dialog line instead of an npm run against a spec nobody
/// chose.
pub fn install_argv(version: &str) -> Result<Vec<String>, String> {
    semver::Version::parse(version)
        .map_err(|error| format!("不是有效的版本号 {version:?}：{error}"))?;
    Ok(vec![
        "install".to_string(),
        "-g".to_string(),
        format!("{PACKAGE}@{version}"),
    ])
}

/// How to run npm for an install, once the platform has had its say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCommand {
    /// The program to execute.
    pub program: PathBuf,
    /// Its argv.
    pub args: Vec<String>,
}

/// Characters cmd.exe treats as syntax even inside a token.
const CMD_METACHARS: [char; 10] = [' ', '"', '&', '|', '<', '>', '^', '(', ')', '%'];

/// Quote one argv token for a `cmd.exe /c` command line.
///
/// cmd groups only with double quotes, so a token that needs quoting is wrapped
/// and its embedded quotes are doubled. `%` is in the list because cmd expands
/// it even inside quotes; the tokens here are a path and a validated version, so
/// the rule is stated for the one place it matters rather than because either is
/// expected to hit it.
pub fn cmd_quote(arg: &str) -> String {
    if !arg.contains(CMD_METACHARS) {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('"', "\"\""))
}

/// Build a `cmd.exe /c` command line from argv.
pub fn cmd_command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| cmd_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command that installs `version` through `npm` on `os`.
///
/// Windows cannot run npm's own launcher without help. npm installs `npm` (a
/// POSIX shell script), `npm.cmd` and `npm.ps1` side by side, and the first is
/// not a PE image at all: handing that to `CreateProcess` is the `os error 193`
/// / "%1 不是有效的 Win32 应用程序" the update button used to report, and picking
/// it out of the directory listing was the whole of that bug — hence the order
/// in [`crate::server::NPM_EXE_NAMES`]. A `.cmd` *can* be started directly, but
/// only through the implicit route `CreateProcess` takes, which leaves the
/// interpreter's switches and the quoting of the line to whoever wrote the
/// launcher. Going through `cmd.exe` explicitly is what makes `/d` (no AutoRun),
/// `/s`, and the token-by-token quoting below ours to decide.
///
/// `os` is a parameter rather than `cfg!` so both answers are testable on either
/// machine — the same reason [`crate::self_update::installer_suffixes`] takes it.
pub fn install_command(npm: &Path, version: &str, os: &str) -> Result<InstallCommand, String> {
    let args = install_argv(version)?;
    if os != "windows" {
        return Ok(InstallCommand {
            program: npm.to_path_buf(),
            args,
        });
    }
    let mut line: Vec<String> = Vec::with_capacity(args.len() + 1);
    line.push(npm.display().to_string());
    line.extend(args);
    Ok(InstallCommand {
        program: PathBuf::from("cmd.exe"),
        args: vec![
            "/d".to_string(),
            "/s".to_string(),
            "/c".to_string(),
            cmd_command_line(&line),
        ],
    })
}

/// Versions the user asked not to be reminded about, persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DismissStore {
    /// Dismissed version strings.
    #[serde(default)]
    pub dismissed: Vec<String>,
    /// Where the list is persisted. Absent for in-memory use.
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl DismissStore {
    /// An in-memory store, for tests and for a failed load.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load from `path`, falling back to an empty list when the file is
    /// missing or unreadable. A corrupt file must never block startup.
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut store = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Self>(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        store.path = Some(path);
        store
    }

    /// Persist the list, creating parent directories as needed.
    pub fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| format!("创建目录失败：{error}"))?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|error| format!("序列化失败：{error}"))?;
        std::fs::write(path, text).map_err(|error| format!("写入失败：{error}"))
    }

    /// Whether `version` is on the list.
    pub fn is_dismissed(&self, version: &str) -> bool {
        self.dismissed.iter().any(|entry| entry == version)
    }

    /// Add `version` and persist.
    pub fn dismiss(&mut self, version: &str) -> Result<(), String> {
        if !self.is_dismissed(version) {
            self.dismissed.push(version.to_string());
        }
        self.save()
    }

    /// Forget everything, so the next launch prompts again.
    pub fn clear(&mut self) -> Result<(), String> {
        self.dismissed.clear();
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(versions: &[(&str, &str)]) -> RegistryDoc {
        let mut doc = RegistryDoc::default();
        for (version, published) in versions {
            doc.versions
                .insert((*version).to_string(), serde_json::json!({}));
            doc.time
                .insert((*version).to_string(), (*published).to_string());
        }
        doc.dist_tags.insert("latest".into(), "0.1.5-rc.2".into());
        doc.dist_tags.insert("alpha".into(), "0.1.6-alpha.1".into());
        doc
    }

    #[test]
    fn channels_come_from_the_version_string_not_the_tag() {
        assert_eq!(channel_of("0.1.5"), "stable");
        assert_eq!(channel_of("0.1.5-rc.1"), "rc");
        assert_eq!(channel_of("0.1.6-alpha.1"), "alpha");
        assert_eq!(channel_of("0.1.5-beta.1"), "other");
        assert_eq!(channel_of("not-a-version"), "other");
        // The registry's own `latest` is a release candidate here, which is
        // exactly why the tag cannot be used as the channel definition.
        assert_eq!(channel_of("0.1.5-rc.1"), "rc");
    }

    #[test]
    fn newer_orders_prereleases_within_a_line() {
        assert!(is_newer("0.1.5-rc.2", "0.1.5-rc.1"));
        assert!(!is_newer("0.1.5-rc.1", "0.1.5-rc.2"));
        // A new patch beats an older line's candidate.
        assert!(is_newer("0.1.6-alpha.1", "0.1.5-rc.1"));
        // A plain release beats its own candidates.
        assert!(is_newer("0.1.5", "0.1.5-rc.2"));
    }

    #[test]
    fn report_splits_three_channels_newest_first() {
        let registry = doc(&[
            ("0.1.5-rc.1", "2026-09-01T00:00:00Z"),
            ("0.1.5-rc.2", "2026-09-02T00:00:00Z"),
            ("0.1.6-alpha.1", "2026-09-03T00:00:00Z"),
            ("0.1.4", "2026-08-01T00:00:00Z"),
        ]);
        let report = build_report("0.1.5-rc.1", &registry, &DismissStore::in_memory());

        let ids: Vec<&str> = report.channels.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["stable", "rc", "alpha"]);

        let stable = &report.channels[0];
        assert_eq!(stable.latest.as_ref().unwrap().version, "0.1.4");
        assert!(
            !stable.newer,
            "0.1.4 is older than the installed 0.1.5-rc.1"
        );
        assert!(
            stable.dist_tags.is_empty(),
            "no tag points at the stable track"
        );

        let rc = &report.channels[1];
        assert_eq!(rc.latest.as_ref().unwrap().version, "0.1.5-rc.2");
        assert_eq!(rc.versions[0].version, "0.1.5-rc.2");
        assert_eq!(rc.versions[1].version, "0.1.5-rc.1");
        assert!(rc.newer);
        // `latest` is the tag pointing at a release candidate — the reason the
        // channel cannot be defined by the tag name.
        assert_eq!(rc.dist_tags, vec!["latest".to_string()]);

        let alpha = &report.channels[2];
        assert_eq!(alpha.latest.as_ref().unwrap().version, "0.1.6-alpha.1");
        assert_eq!(alpha.dist_tags, vec!["alpha".to_string()]);

        // The candidate stays on the installed stability track or above: an
        // rc user is offered the newer rc (0.1.5-rc.2), not the newer but less
        // settled alpha (0.1.6-alpha.1).
        assert_eq!(report.candidate.as_ref().unwrap().version, "0.1.5-rc.2");
        assert!(report.should_prompt());
    }

    #[test]
    fn candidate_never_suggests_a_less_stable_channel() {
        let registry = doc(&[
            ("0.1.5-rc.2", "2026-09-02T00:00:00Z"),
            ("0.1.6-alpha.1", "2026-09-03T00:00:00Z"),
        ]);

        // On rc: rc and stable qualify, alpha does not.
        let from_rc = build_report("0.1.5-rc.1", &registry, &DismissStore::in_memory());
        assert_eq!(from_rc.candidate.as_ref().unwrap().version, "0.1.5-rc.2");

        // On alpha: every channel qualifies, so the newest wins.
        let from_alpha = build_report("0.1.5-alpha.2", &registry, &DismissStore::in_memory());
        assert_eq!(
            from_alpha.candidate.as_ref().unwrap().version,
            "0.1.6-alpha.1"
        );

        // On stable: only stable qualifies; here it has nothing newer.
        let from_stable = build_report("0.1.4", &registry, &DismissStore::in_memory());
        assert!(
            from_stable.candidate.is_none(),
            "must not push a prerelease onto a stable install"
        );
        assert!(!from_stable.should_prompt());
    }

    #[test]
    fn candidate_is_none_when_everything_is_already_installed() {
        let registry = doc(&[
            ("0.1.5-rc.1", "2026-09-01T00:00:00Z"),
            ("0.1.4", "2026-08-01T00:00:00Z"),
        ]);
        let report = build_report("0.1.5-rc.1", &registry, &DismissStore::in_memory());
        assert!(report.candidate.is_none());
        assert!(!report.should_prompt());
    }

    #[test]
    fn dismissing_a_version_suppresses_the_prompt_for_it_only() {
        let registry = doc(&[("0.1.5-rc.2", "2026-09-02T00:00:00Z")]);
        let mut store = DismissStore::in_memory();
        store.dismiss("0.1.5-rc.2").unwrap();

        let report = build_report("0.1.5-rc.1", &registry, &store);
        assert_eq!(report.candidate.as_ref().unwrap().version, "0.1.5-rc.2");
        assert!(report.candidate_dismissed);
        assert!(
            !report.should_prompt(),
            "same version must not prompt twice"
        );

        // A newer version is not covered by the earlier dismissal.
        let mut registry = registry;
        registry
            .versions
            .insert("0.1.5-rc.3".into(), serde_json::json!({}));
        let report = build_report("0.1.5-rc.1", &registry, &store);
        assert_eq!(report.candidate.as_ref().unwrap().version, "0.1.5-rc.3");
        assert!(!report.candidate_dismissed);
        assert!(report.should_prompt());
    }

    #[test]
    fn dismiss_store_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("updates.json");

        let mut store = DismissStore::load(&path);
        assert!(store.dismissed.is_empty(), "missing file loads as empty");
        store.dismiss("0.1.6-alpha.1").unwrap();
        store.dismiss("0.1.6-alpha.1").unwrap();

        let reloaded = DismissStore::load(&path);
        assert_eq!(reloaded.dismissed, vec!["0.1.6-alpha.1".to_string()]);
        assert!(reloaded.is_dismissed("0.1.6-alpha.1"));
        assert!(!reloaded.is_dismissed("0.1.6-alpha.2"));

        let mut cleared = reloaded.clone();
        cleared.clear().unwrap();
        assert!(DismissStore::load(&path).dismissed.is_empty());
    }

    #[test]
    fn corrupt_dismiss_file_does_not_break_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("updates.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(DismissStore::load(&path).dismissed.is_empty());
    }

    #[test]
    fn install_command_targets_the_requested_version() {
        assert_eq!(
            install_argv("0.1.6-alpha.1").expect("a version"),
            vec!["install", "-g", "@deepseek-ai/dsh@0.1.6-alpha.1"]
        );
    }

    #[test]
    fn a_value_that_is_not_a_version_never_becomes_an_npm_argument() {
        // The page sends this back, so it is checked rather than trusted.
        for rejected in [
            "",
            "latest",
            "0.1.6; rm -rf /",
            "--force",
            "@scope/other@1.0.0",
        ] {
            assert!(
                install_argv(rejected).is_err(),
                "{rejected:?} should not become an install target"
            );
        }
    }

    #[test]
    fn an_install_on_windows_goes_through_the_command_interpreter() {
        let command = install_command(
            Path::new(r"C:\Program Files\nodejs\npm.cmd"),
            "0.1.6-alpha.1",
            "windows",
        )
        .expect("a command");
        assert_eq!(command.program, PathBuf::from("cmd.exe"));
        assert_eq!(
            command.args,
            vec![
                "/d".to_string(),
                "/s".to_string(),
                "/c".to_string(),
                r#""C:\Program Files\nodejs\npm.cmd" install -g @deepseek-ai/dsh@0.1.6-alpha.1"#
                    .to_string(),
            ]
        );
    }

    #[test]
    fn an_install_elsewhere_runs_npm_directly() {
        let command = install_command(Path::new("/usr/local/bin/npm"), "0.1.6-alpha.1", "linux")
            .expect("a command");
        assert_eq!(command.program, PathBuf::from("/usr/local/bin/npm"));
        assert_eq!(
            command.args,
            vec!["install", "-g", "@deepseek-ai/dsh@0.1.6-alpha.1"]
        );
    }

    #[test]
    fn only_a_token_cmd_would_reparse_is_quoted() {
        assert_eq!(cmd_quote("install"), "install");
        assert_eq!(cmd_quote("a b"), "\"a b\"");
        assert_eq!(cmd_quote("a\"b"), "\"a\"\"b\"");
        assert_eq!(cmd_quote("100%"), "\"100%\"");
        assert_eq!(
            cmd_command_line(&["a".to_string(), "b c".to_string()]),
            "a \"b c\""
        );
    }

    #[test]
    fn a_bad_version_is_refused_by_the_command_builder_too() {
        assert!(install_command(Path::new("/usr/bin/npm"), "latest", "linux").is_err());
        assert!(install_command(Path::new("npm.cmd"), "0.1.6; rm -rf /", "windows").is_err());
    }
}
