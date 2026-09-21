//! The command that installs one dsh version globally.
//!
//! Built here, in the crate that has no GUI dependency, so both the platform's
//! answer and the quoting it needs are testable on either machine.

use std::path::{Path, PathBuf};

use super::PACKAGE;

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

#[cfg(test)]
mod tests {
    use super::*;

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
