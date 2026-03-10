use crate::tracking;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use std::ffi::OsString;
use std::process::Command;

lazy_static! {
    static ref RE_YARN_PROGRESS: Regex =
        Regex::new(r"(?i)^\[(\d+/\d+)\]|Resolving|Fetching|Linking").unwrap();
}

#[derive(Debug, Clone)]
pub enum YarnCommand {
    Install,
    Outdated,
    List,
}

pub fn run(cmd: YarnCommand, args: &[String], verbose: u8) -> Result<()> {
    match cmd {
        YarnCommand::Install => run_install(args, verbose),
        YarnCommand::Outdated => run_outdated(args, verbose),
        YarnCommand::List => run_list(args, verbose),
    }
}

fn run_install(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("yarn");
    cmd.arg("install");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: yarn install {}", args.join(" "));
    }

    let output = cmd.output().context("Failed to run yarn install")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_yarn_install(&raw);
    println!("{}", filtered);

    timer.track(
        &format!("yarn install {}", args.join(" ")),
        &format!("rtk yarn install {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}

/// yarn outdated JSON structure
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct YarnOutdatedEntry {
    #[serde(default)]
    current: String,
    #[serde(default)]
    wanted: String,
    #[serde(default)]
    latest: String,
    #[serde(default)]
    package: String,
    #[serde(rename = "type", default)]
    dep_type: String,
}

fn run_outdated(args: &[String], _verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("yarn");
    cmd.arg("outdated");
    cmd.arg("--json");
    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd.output().context("Failed to run yarn outdated")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_yarn_outdated(&stdout);

    if filtered.trim().is_empty() {
        println!("All packages up-to-date \u{2713}");
    } else {
        println!("{}", filtered);
    }

    timer.track("yarn outdated", "rtk yarn outdated", &raw, &filtered);

    // yarn outdated exits 1 when packages are outdated
    Ok(())
}

fn run_list(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("yarn");
    cmd.arg("list");
    cmd.arg("--depth=0");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: yarn list --depth=0");
    }

    let output = cmd.output().context("Failed to run yarn list")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_yarn_list(&stdout);
    println!("{}", filtered);

    timer.track("yarn list", "rtk yarn list", &raw, &filtered);

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}

/// Runs an unsupported yarn subcommand by passing it through directly
pub fn run_passthrough(args: &[OsString], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("yarn passthrough: {:?}", args);
    }
    let status = Command::new("yarn")
        .args(args)
        .status()
        .context("Failed to run yarn")?;

    let args_str = tracking::args_display(args);
    timer.track_passthrough(
        &format!("yarn {}", args_str),
        &format!("rtk yarn {} (passthrough)", args_str),
    );

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Filter yarn install output - strip progress, keep summary
fn filter_yarn_install(output: &str) -> String {
    let mut result = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip progress lines
        if RE_YARN_PROGRESS.is_match(trimmed) {
            continue;
        }

        // Skip info/warning noise
        if trimmed.starts_with("info") && !trimmed.contains("error") {
            continue;
        }
        if trimmed.starts_with("warning") && trimmed.contains("already exists") {
            continue;
        }

        // Keep errors
        if trimmed.contains("error") || trimmed.contains("Error") || trimmed.contains("ERR") {
            result.push(trimmed.to_string());
            continue;
        }

        // Keep summary lines
        if trimmed.starts_with("Done in")
            || trimmed.starts_with("success")
            || trimmed.contains("added")
            || trimmed.contains("removed")
            || trimmed.contains("packages in")
            || trimmed.starts_with("✨")
        {
            result.push(trimmed.to_string());
        }
    }

    if result.is_empty() {
        "ok \u{2713}".to_string()
    } else {
        result.join("\n")
    }
}

/// Filter yarn outdated output - parse NDJSON or table format
fn filter_yarn_outdated(output: &str) -> String {
    // Try parsing NDJSON (yarn v1 --json gives one JSON per line)
    let mut entries: Vec<(String, String, String, String)> = Vec::new();

    for line in output.lines() {
        if let Ok(entry) = serde_json::from_str::<YarnOutdatedEntry>(line) {
            if !entry.package.is_empty() {
                entries.push((entry.package, entry.current, entry.latest, entry.dep_type));
            }
        }
    }

    if !entries.is_empty() {
        let mut lines = Vec::new();
        lines.push(format!("{} outdated", entries.len()));
        for (name, current, latest, dtype) in &entries {
            let marker = if dtype == "devDependencies" {
                " (dev)"
            } else {
                ""
            };
            lines.push(format!(
                "{}: {} \u{2192} {}{}",
                name, current, latest, marker
            ));
        }
        return lines.join("\n");
    }

    // Fallback: parse text table
    filter_yarn_outdated_text(output)
}

/// Fallback text parser for yarn outdated
fn filter_yarn_outdated_text(output: &str) -> String {
    let mut result = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Package")
            || trimmed.starts_with("Current")
            || trimmed.contains("───")
            || trimmed.starts_with("info")
            || trimmed.starts_with("Done")
        {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 4 {
            result.push(format!("{}: {} \u{2192} {}", parts[0], parts[1], parts[3]));
        }
    }

    if result.is_empty() {
        String::new()
    } else {
        format!("{} outdated\n{}", result.len(), result.join("\n"))
    }
}

/// Filter yarn list output - compact dependency tree
fn filter_yarn_list(output: &str) -> String {
    let mut packages = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("yarn list")
            || trimmed.starts_with("Done in")
            || trimmed.starts_with("info")
            || trimmed.starts_with("warning")
        {
            continue;
        }

        // Parse "├─ package@version" or "└─ package@version"
        let clean = trimmed
            .trim_start_matches(|c: char| "├└│─ ".contains(c))
            .trim();
        if !clean.is_empty() && clean.contains('@') {
            packages.push(clean.to_string());
        }
    }

    if packages.is_empty() {
        "yarn list: 0 packages".to_string()
    } else {
        let count = packages.len();
        let shown: Vec<_> = packages.iter().take(30).cloned().collect();
        let mut result = format!("yarn list: {} packages\n{}", count, shown.join("\n"));
        if count > 30 {
            result.push_str(&format!("\n... +{} more", count - 30));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_yarn_install() {
        let output = r#"yarn install v1.22.19
[1/4] Resolving packages...
[2/4] Fetching packages...
[3/4] Linking dependencies...
[4/4] Building fresh packages...
info fsevents@2.3.3: The platform "linux" is incompatible with this module.
info "fsevents@2.3.3" is an optional dependency and failed compatibility check.
success Saved lockfile.
Done in 8.23s.
"#;
        let result = filter_yarn_install(output);
        assert!(!result.contains("Resolving"));
        assert!(!result.contains("[1/4]"));
        assert!(result.contains("Done in") || result.contains("success"));
    }

    #[test]
    fn test_filter_yarn_install_savings() {
        let raw = r#"yarn install v1.22.19
[1/4] Resolving packages...
[2/4] Fetching packages...
info There appears to be trouble with your network connection.
[3/4] Linking dependencies...
warning "eslint > @eslint/eslintrc > globals@13.24.0" has unmet peer dependency
warning "typescript > some-dep" already exists and conflicts
info fsevents@2.3.3: The platform "linux" is incompatible
info "fsevents@2.3.3" is an optional dependency
[4/4] Building fresh packages...
success Saved lockfile.
Done in 12.45s.
"#;
        let filtered = filter_yarn_install(raw);
        let savings = 100.0 - (count_tokens(&filtered) as f64 / count_tokens(raw) as f64 * 100.0);
        assert!(
            savings >= 60.0,
            "Expected >=60% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_yarn_outdated_text() {
        let output = r#"Package  Current  Wanted  Latest  Package Type
lodash   4.17.20  4.17.21 4.17.21 dependencies
react    17.0.2   18.2.0  18.2.0  dependencies
Done in 0.5s.
"#;
        let result = filter_yarn_outdated_text(output);
        assert!(result.contains("lodash: 4.17.20 \u{2192} 4.17.21"));
        assert!(result.contains("react: 17.0.2 \u{2192} 18.2.0"));
        assert!(result.contains("2 outdated"));
    }

    #[test]
    fn test_filter_yarn_list() {
        let output = r#"yarn list v1.22.19
├─ lodash@4.17.21
├─ react@18.2.0
├─ react-dom@18.2.0
└─ typescript@5.3.3
Done in 0.12s.
"#;
        let result = filter_yarn_list(output);
        assert!(result.contains("4 packages"));
        assert!(result.contains("lodash@4.17.21"));
        assert!(result.contains("typescript@5.3.3"));
    }

    #[test]
    fn test_filter_yarn_install_empty() {
        let result = filter_yarn_install("");
        assert_eq!(result, "ok \u{2713}");
    }
}
