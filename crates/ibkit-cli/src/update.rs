//! `ibkit update`: replace this binary and its JVM agent with the latest
//! GitHub release, through the release's own installer.

use anyhow::{Context, Result, bail};
use ibkit_gateway::platform::AGENT_LIBRARY;
use semver::Version;
use std::path::{Path, PathBuf};

const REPOSITORY: &str = "racetozero/ibkit";
const HOMEBREW_UPGRADE: &str = "brew upgrade racetozero/tap/ibkit";

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
}

/// Update and return what happened, for the caller to print.
pub async fn run() -> Result<String> {
    if cfg!(debug_assertions) {
        bail!(
            "You ran `ibkit update` from a debug build. Install an ibkit release (see the README) to use this command, or rebuild from source."
        );
    }
    let binary = std::fs::canonicalize(std::env::current_exe()?)
        .context("Cannot find this ibkit binary. Run ibkit from its installed path.")?;
    if homebrew_owns(&binary) {
        bail!(
            "You ran `ibkit update` on a binary that Homebrew installed. Run `{HOMEBREW_UPGRADE}` to update it."
        );
    }
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("ibkit/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let release: Release = client
        .get(format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .context("Cannot read the latest ibkit release from GitHub. Check the network connection and try again.")?
        .json()
        .await?;
    let latest = parse_tag(&release.tag_name)?;
    if latest <= current {
        return Ok(format!("ibkit v{current} is up to date."));
    }
    let installer_name = if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    };
    let url = format!("https://raw.githubusercontent.com/{REPOSITORY}/v{latest}/{installer_name}");
    let script = client
        .get(&url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("Cannot download the installer from {url}. Try again later."))?
        .bytes()
        .await?;
    let staging = tempfile::tempdir()?;
    let installer = staging.path().join(installer_name);
    std::fs::write(&installer, &script)?;
    let (staged_binary, staged_agent) = install_staged(&installer, &latest, staging.path()).await?;
    let agent = agent_target(&binary);
    replace_pair(&staged_binary, &staged_agent, &agent)?;
    Ok(format!(
        "Updated ibkit from v{current} to v{latest}. Restart running `ibkit gateway` controllers so that Gateway loads the new agent."
    ))
}

/// Homebrew keeps a formula's files in `Cellar/ibkit/<version>/`.
fn homebrew_owns(binary: &Path) -> bool {
    binary
        .ancestors()
        .nth(3)
        .is_some_and(|rack| rack.ends_with("Cellar/ibkit"))
}

fn parse_tag(tag: &str) -> Result<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).with_context(|| {
        format!("The latest GitHub release has the tag {tag:?}, which is not vX.Y.Z. Install a release by hand from https://github.com/{REPOSITORY}/releases.")
    })
}

/// Where this binary's agent goes: the one that `find_agent` uses for it,
/// next to the binary or in `../lib`.
fn agent_target(binary: &Path) -> PathBuf {
    let directory = binary.parent().unwrap_or(Path::new("."));
    let beside = directory.join(AGENT_LIBRARY);
    if cfg!(windows) || beside.is_file() {
        beside
    } else {
        directory.join("../lib").join(AGENT_LIBRARY)
    }
}

/// Run the release installer into `staging` (`bin/` and, on Unix, `lib/`).
async fn install_staged(
    installer: &Path,
    version: &Version,
    staging: &Path,
) -> Result<(PathBuf, PathBuf)> {
    let bin = staging.join("bin");
    let mut command = if cfg!(windows) {
        let mut command = tokio::process::Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(installer);
        command
    } else {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.arg(installer);
        command
    };
    let output = command
        .env("IBKIT_VERSION", version.to_string())
        .env("IBKIT_INSTALL_DIR", &bin)
        .env("IBKIT_NO_MODIFY_PATH", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .context("Cannot start the ibkit installer. Check that /bin/sh (or PowerShell on Windows) is available.")?;
    if !output.status.success() {
        bail!(
            "The ibkit v{version} installer failed ({}): {}. Your installed ibkit is unchanged.",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let binary = bin.join(if cfg!(windows) { "ibkit.exe" } else { "ibkit" });
    let agent = agent_target(&binary);
    for path in [&binary, &agent] {
        if !path.is_file() {
            bail!(
                "The ibkit v{version} installer did not create {}. Your installed ibkit is unchanged; install the release by hand from https://github.com/{REPOSITORY}/releases.",
                path.display()
            );
        }
    }
    Ok((binary, agent))
}

/// Replace the agent, then the binary; restore the old agent when the
/// binary cannot be replaced, so the pair always matches. The old agent is
/// renamed rather than overwritten: Windows allows that for a loaded DLL,
/// and a running Gateway keeps the file it loaded.
fn replace_pair(staged_binary: &Path, staged_agent: &Path, agent: &Path) -> Result<()> {
    let directory = agent.parent().context("The agent path has no directory.")?;
    std::fs::create_dir_all(directory)?;
    // A unique name: on Windows an old agent that a Gateway still has loaded
    // cannot be removed, and stays until that Gateway exits.
    let old = agent.with_extension(format!("old-{}", std::process::id()));
    let new = agent.with_extension("new");
    std::fs::copy(staged_agent, &new).with_context(|| {
        format!(
            "Cannot write {}. Correct the permissions of {}, then run ibkit update again.",
            new.display(),
            directory.display()
        )
    })?;
    let had_agent = agent.exists();
    if had_agent {
        std::fs::rename(agent, &old)?;
    }
    std::fs::rename(&new, agent)?;
    if let Err(error) = self_replace::self_replace(staged_binary) {
        let _ = std::fs::remove_file(agent);
        if had_agent {
            let _ = std::fs::rename(&old, agent);
        }
        return Err(error).context("Cannot replace the ibkit binary; the previous version stays installed. Correct the permissions of its directory, then run ibkit update again.");
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tags_are_versions() {
        assert_eq!(parse_tag("v1.2.3").unwrap(), Version::new(1, 2, 3));
        assert_eq!(parse_tag("1.2.3").unwrap(), Version::new(1, 2, 3));
        assert!(parse_tag("release-1").is_err());
    }

    #[test]
    fn only_a_homebrew_cellar_counts_as_homebrew() {
        assert!(homebrew_owns(Path::new(
            "/opt/homebrew/Cellar/ibkit/1.2.3/bin/ibkit"
        )));
        assert!(!homebrew_owns(Path::new("/home/me/.local/bin/ibkit")));
        assert!(!homebrew_owns(Path::new(
            "/opt/homebrew/Cellar/other/1.0/bin/ibkit"
        )));
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn a_debug_build_refuses_before_any_network_access() {
        let error = run().await.unwrap_err().to_string();
        assert!(error.contains("debug build"), "{error}");
    }

    /// The installer gets the version and a staging directory, and the
    /// binary and agent it writes there are found.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_installer_runs_into_a_staging_directory() {
        let staging = tempfile::tempdir().unwrap();
        let installer = staging.path().join("install.sh");
        std::fs::write(
            &installer,
            format!(
                "set -eu\nmkdir -p \"$IBKIT_INSTALL_DIR\" \"$IBKIT_INSTALL_DIR/../lib\"\nprintf '%s' \"$IBKIT_VERSION\" > \"$IBKIT_INSTALL_DIR/ibkit\"\nprintf agent > \"$IBKIT_INSTALL_DIR/../lib/{AGENT_LIBRARY}\"\n"
            ),
        )
        .unwrap();
        let (binary, agent) = install_staged(&installer, &Version::new(1, 2, 3), staging.path())
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(binary).unwrap(), "1.2.3");
        assert_eq!(std::fs::read_to_string(agent).unwrap(), "agent");
    }
}
