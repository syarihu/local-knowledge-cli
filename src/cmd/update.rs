use std::path::PathBuf;

use crate::util::{DEFAULT_REPO, VERSION, get_db_path, home_dir, now_iso, open_db_with_migrate};

pub fn cmd_update(skip_verify: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = home_dir().join(".config").join("lk");
    let config_path = config_dir.join("config.json");

    let repo = if config_path.exists() {
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
        config["repo"].as_str().unwrap_or(DEFAULT_REPO).to_string()
    } else {
        DEFAULT_REPO.to_string()
    };

    let homebrew = is_homebrew_install();

    let dest = if homebrew {
        // Homebrew installation: use brew upgrade
        println!("Homebrew installation detected. Running brew upgrade...");
        let status = std::process::Command::new("brew")
            .args(["upgrade", "syarihu/tap/lk"])
            .status()?;
        if !status.success() {
            return Err(
                "brew upgrade failed. Try: brew update && brew upgrade syarihu/tap/lk".into(),
            );
        }
        // Use the current exe path (symlink resolves to new version after upgrade)
        std::env::current_exe()
            .ok()
            .and_then(|p| p.canonicalize().ok())
            .unwrap_or_else(|| PathBuf::from("lk"))
    } else {
        // Manual installation: download from GitHub releases
        let target = detect_target()?;
        let asset_name = format!("lk-{target}.tar.gz");
        let checksum_name = "checksums.txt";

        println!("Checking for updates...");

        let latest_tag = fetch_latest_tag(&repo)?;
        println!("Latest version: {latest_tag}");

        let base_url = format!("https://github.com/{repo}/releases/download/{latest_tag}");

        // Use tempfile crate for secure temporary directory
        let tmpdir = tempfile::tempdir()?;
        let tmppath = tmpdir.path();

        // Download binary archive
        let download_url = format!("{base_url}/{asset_name}");
        println!("Downloading {download_url}...");

        let archive_path = tmppath.join(&asset_name);
        let dl = std::process::Command::new("curl")
            .args(["-fSL", &download_url, "-o"])
            .arg(&archive_path)
            .output()?;

        if !dl.status.success() {
            return Err(format!("Download failed: {}", String::from_utf8_lossy(&dl.stderr)).into());
        }

        // Download and verify checksum
        let checksum_url = format!("{base_url}/{checksum_name}");
        let checksum_path = tmppath.join(checksum_name);
        let dl_checksum = std::process::Command::new("curl")
            .args(["-fsSL", &checksum_url, "-o"])
            .arg(&checksum_path)
            .output()?;

        if dl_checksum.status.success() {
            verify_checksum(&archive_path, &checksum_path, &asset_name)?;
            println!("Checksum verified.");
        } else if skip_verify {
            eprintln!(
                "Warning: checksums.txt not found in release, skipping verification (--skip-verify)."
            );
        } else {
            return Err("Checksum file not found in release. Use --skip-verify to bypass (not recommended).".into());
        }

        // Extract
        let extract = std::process::Command::new("tar")
            .args(["xzf"])
            .arg(&archive_path)
            .arg("-C")
            .arg(tmppath)
            .output()?;

        if !extract.status.success() {
            return Err("Failed to extract archive".into());
        }

        // Install binary
        let bin_dir = home_dir().join(".local").join("bin");
        std::fs::create_dir_all(&bin_dir)?;
        let d = bin_dir.join("lk");
        std::fs::remove_file(&d).ok();
        std::fs::copy(tmppath.join("lk"), &d)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755))?;
        }

        // tmpdir is automatically cleaned up when dropped
        d
    };

    // === Shared post-update logic ===

    // Install embedded Claude commands
    install_embedded_commands()?;

    // Get the version from the newly installed binary
    let new_version = std::process::Command::new(&dest)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().strip_prefix("lk ").map(|v| v.to_string()))
        .unwrap_or_else(|| VERSION.to_string());

    // Update config
    std::fs::create_dir_all(&config_dir)?;
    let config_json = serde_json::json!({
        "install_dir": "",
        "installed_at": now_iso(),
        "version": new_version,
        "repo": repo,
    });
    std::fs::write(&config_path, serde_json::to_string_pretty(&config_json)?)?;

    // Run DB migration if inside a project with a knowledge DB
    let db_path = get_db_path();
    if db_path.exists() {
        let _ = open_db_with_migrate(); // run migration + auto-sync if needed
    }

    println!("\nUpdate complete! (lk {new_version})");
    Ok(())
}

/// Verify SHA256 checksum of downloaded file against checksums.txt
fn verify_checksum(
    file_path: &std::path::Path,
    checksums_path: &std::path::Path,
    expected_filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};

    let checksums_content = std::fs::read_to_string(checksums_path)?;
    let expected_hash = checksums_content
        .lines()
        .find_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[1] == expected_filename {
                Some(parts[0].to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| format!("Checksum for {expected_filename} not found in checksums.txt"))?;

    let file_data = std::fs::read(file_path)?;
    let actual_hash = hex::encode(Sha256::digest(&file_data));

    if actual_hash != expected_hash {
        return Err(format!(
            "Checksum mismatch!\n  Expected: {expected_hash}\n  Actual:   {actual_hash}"
        )
        .into());
    }

    Ok(())
}

/// Install Claude commands embedded in the binary.
/// Commands are compiled into the binary so they can't be tampered with via MITM.
pub fn install_embedded_commands() -> Result<(), Box<dyn std::error::Error>> {
    let commands_dir = home_dir().join(".claude").join("commands");
    std::fs::create_dir_all(&commands_dir)?;

    // Clean up legacy ~ prefixed command files
    for (filename, _) in EMBEDDED_COMMANDS {
        let legacy = format!("~{filename}");
        let legacy_path = commands_dir.join(&legacy);
        if legacy_path.exists() {
            std::fs::remove_file(&legacy_path)?;
            println!("  Removed legacy: {legacy}");
        }
    }

    for (filename, content) in EMBEDDED_COMMANDS {
        std::fs::write(commands_dir.join(filename), content)?;
        println!("  Updated: {filename}");
    }
    Ok(())
}

/// Fetch the latest release tag from GitHub.
/// Tries `gh` CLI first (already authenticated), falls back to curl redirect.
fn fetch_latest_tag(repo: &str) -> Result<String, Box<dyn std::error::Error>> {
    // Try gh CLI first
    if let Ok(output) = std::process::Command::new("gh")
        .args([
            "release", "view", "--repo", repo, "--json", "tagName", "-q", ".tagName",
        ])
        .output()
        && output.status.success()
    {
        let tag = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !tag.is_empty() {
            println!("Latest version: {tag}");
            return Ok(tag);
        }
    }

    // Fallback: curl effective URL (follows redirects, extracts final URL)
    let output = std::process::Command::new("curl")
        .args([
            "-sL",
            "-o",
            "/dev/null",
            "-w",
            "%{url_effective}",
            &format!("https://github.com/{repo}/releases/latest"),
        ])
        .output()?;

    let effective_url = String::from_utf8_lossy(&output.stdout).to_string();
    let tag = effective_url
        .trim()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string();

    if tag.is_empty() {
        return Err("Could not determine latest version".into());
    }

    println!("Latest version: {tag}");
    Ok(tag)
}

const EMBEDDED_COMMANDS: &[(&str, &str)] = &[
    (
        "lk-knowledge-search.md",
        include_str!("../../commands/lk-knowledge-search.md"),
    ),
    (
        "lk-knowledge-add-db.md",
        include_str!("../../commands/lk-knowledge-add-db.md"),
    ),
    (
        "lk-knowledge-export.md",
        include_str!("../../commands/lk-knowledge-export.md"),
    ),
    (
        "lk-knowledge-sync.md",
        include_str!("../../commands/lk-knowledge-sync.md"),
    ),
    (
        "lk-knowledge-write-md.md",
        include_str!("../../commands/lk-knowledge-write-md.md"),
    ),
    (
        "lk-knowledge-discover.md",
        include_str!("../../commands/lk-knowledge-discover.md"),
    ),
    (
        "lk-knowledge-refresh.md",
        include_str!("../../commands/lk-knowledge-refresh.md"),
    ),
    (
        "lk-knowledge-from-branch.md",
        include_str!("../../commands/lk-knowledge-from-branch.md"),
    ),
];

fn detect_target() -> Result<String, Box<dyn std::error::Error>> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu".to_string()),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu".to_string()),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc".to_string()),
        _ => Err(format!("Unsupported platform: {os}-{arch}").into()),
    }
}

/// Detect if the currently running binary was installed via Homebrew.
fn is_homebrew_install() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| {
            let s = p.to_string_lossy();
            s.contains("/homebrew/") || s.contains("/Cellar/") || s.contains("/linuxbrew/")
        })
        .unwrap_or(false)
}
