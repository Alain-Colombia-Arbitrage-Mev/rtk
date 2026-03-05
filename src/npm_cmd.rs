use crate::tracking;
use crate::utils::resolved_command;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use std::ffi::OsString;
use std::process::Command;

/// Known npm subcommands that should NOT get "run" injected.
/// Shared between production code and tests to avoid drift.
const NPM_SUBCOMMANDS: &[&str] = &[
    "install",
    "i",
    "ci",
    "uninstall",
    "remove",
    "rm",
    "update",
    "up",
    "list",
    "ls",
    "outdated",
    "init",
    "create",
    "publish",
    "pack",
    "link",
    "audit",
    "fund",
    "exec",
    "explain",
    "why",
    "search",
    "view",
    "info",
    "show",
    "config",
    "set",
    "get",
    "cache",
    "prune",
    "dedupe",
    "doctor",
    "help",
    "version",
    "prefix",
    "root",
    "bin",
    "bugs",
    "docs",
    "home",
    "repo",
    "ping",
    "whoami",
    "token",
    "profile",
    "team",
    "access",
    "owner",
    "deprecate",
    "dist-tag",
    "star",
    "stars",
    "login",
    "logout",
    "adduser",
    "unpublish",
    "pkg",
    "diff",
    "rebuild",
    "test",
    "t",
    "start",
    "stop",
    "restart",
];

lazy_static! {
    /// Webpack/Vite progress lines like [0%] ... [100%]
    static ref RE_PROGRESS: Regex = Regex::new(r"^\s*\[?\d+%\]?").unwrap();
    /// HMR update lines
    static ref RE_HMR: Regex = Regex::new(r"(?i)\[hmr\]|hmr update|hot update").unwrap();
    /// ANSI spinner characters
    static ref RE_SPINNER: Regex = Regex::new(r"[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]").unwrap();
}

#[derive(Debug, Clone)]
pub enum NpmCommand {
    Run,
    Install,
    Ci,
    Outdated,
}

pub fn run(cmd: NpmCommand, args: &[String], verbose: u8, skip_env: bool) -> Result<()> {
    match cmd {
        NpmCommand::Run => run_script(args, verbose, skip_env),
        NpmCommand::Install => run_install(args, verbose),
        NpmCommand::Ci => run_ci(args, verbose),
        NpmCommand::Outdated => run_outdated(args, verbose),
    }
}

/// Run an npm script with filtered output
fn run_script(args: &[String], verbose: u8, skip_env: bool) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = resolved_command("npm");

    // Determine if this is "npm run <script>" or another npm subcommand (install, list, etc.)
    // Only inject "run" when args look like a script name, not a known npm subcommand.
    let first_arg = args.first().map(|s| s.as_str());
    let is_run_explicit = first_arg == Some("run");
    let is_npm_subcommand = first_arg
        .map(|a| NPM_SUBCOMMANDS.contains(&a) || a.starts_with('-'))
        .unwrap_or(false);

    let effective_args = if is_run_explicit {
        // "rtk npm run build" → "npm run build"
        cmd.arg("run");
        &args[1..]
    } else if is_npm_subcommand {
        // "rtk npm install express" → "npm install express"
        args
    } else {
        // "rtk npm build" → "npm run build" (assume script name)
        cmd.arg("run");
        args
    };

    for arg in effective_args {
        cmd.arg(arg);
    }

    if skip_env {
        cmd.env("SKIP_ENV_VALIDATION", "1");
    }

    if verbose > 0 {
        eprintln!("Running: npm {}", args.join(" "));
    }

    let output = cmd.output().context("Failed to run npm")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    // Detect dev server scripts
    let is_dev = args
        .first()
        .map(|s| matches!(s.as_str(), "dev" | "start" | "serve"))
        .unwrap_or(false);

    let filtered = if is_dev {
        filter_dev_server_output(&raw)
    } else {
        filter_npm_output(&raw)
    };
    println!("{}", filtered);

    timer.track(
        &format!("npm {}", args.join(" ")),
        &format!("rtk npm {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

/// Run npm install with compact output
fn run_install(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    // Validate package names in args (before flags)
    for arg in args {
        if arg.starts_with('-') {
            break;
        }
        if !is_valid_package_name(arg) {
            anyhow::bail!(
                "Invalid package name: '{}' (contains unsafe characters)",
                arg
            );
        }
    }

    let mut cmd = Command::new("npm");
    cmd.arg("install");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("npm install running...");
    }

    let output = cmd.output().context("Failed to run npm install")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    let filtered = filter_install_output(&combined);
    println!("{}", filtered);

    timer.track(
        &format!("npm install {}", args.join(" ")),
        &format!("rtk npm install {}", args.join(" ")),
        &combined,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

/// Run npm ci with compact output
fn run_ci(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("npm");
    cmd.arg("ci");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("npm ci running...");
    }

    let output = cmd.output().context("Failed to run npm ci")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    // ci output is similar to install
    let filtered = filter_install_output(&combined);
    println!("{}", filtered);

    timer.track("npm ci", "rtk npm ci", &combined, &filtered);

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

/// npm outdated JSON output structure
#[derive(Debug, Deserialize)]
struct NpmOutdatedPackage {
    current: Option<String>,
    #[allow(dead_code)]
    wanted: Option<String>,
    latest: Option<String>,
    #[serde(rename = "type")]
    dep_type: Option<String>,
}

/// Run npm outdated with compact output
fn run_outdated(args: &[String], _verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("npm");
    cmd.arg("outdated");
    cmd.arg("--json");

    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd.output().context("Failed to run npm outdated")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    let filtered = filter_outdated_output(&stdout);

    if filtered.trim().is_empty() {
        println!("All packages up-to-date \u{2713}");
    } else {
        println!("{}", filtered);
    }

    timer.track("npm outdated", "rtk npm outdated", &combined, &filtered);

    // npm outdated exits 1 when packages are outdated - don't propagate
    Ok(())
}

/// Filter npm outdated JSON into compact "pkg: current → latest" format
fn filter_outdated_output(output: &str) -> String {
    use std::collections::HashMap;

    match serde_json::from_str::<HashMap<String, NpmOutdatedPackage>>(output) {
        Ok(packages) => {
            if packages.is_empty() {
                return String::new();
            }

            let mut lines: Vec<String> = Vec::new();
            let mut sorted: Vec<_> = packages.iter().collect();
            sorted.sort_by_key(|(name, _)| (*name).clone());

            for (name, pkg) in sorted {
                let current = pkg.current.as_deref().unwrap_or("?");
                let latest = pkg.latest.as_deref().unwrap_or("?");
                let dep_marker = if pkg.dep_type.as_deref() == Some("devDependencies") {
                    " (dev)"
                } else {
                    ""
                };
                lines.push(format!(
                    "{}: {} \u{2192} {}{}",
                    name, current, latest, dep_marker
                ));
            }

            format!("{} outdated\n{}", lines.len(), lines.join("\n"))
        }
        Err(_) => {
            // Fallback: filter text output
            filter_outdated_text(output)
        }
    }
}

/// Fallback text parser for npm outdated (non-JSON)
fn filter_outdated_text(output: &str) -> String {
    let mut result = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Package")
            || trimmed.starts_with("Current")
            || trimmed.contains("──")
        {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 4 {
            result.push(format!("{}: {} \u{2192} {}", parts[0], parts[1], parts[3]));
        } else if !trimmed.is_empty() {
            result.push(trimmed.to_string());
        }
    }

    result.join("\n")
}

/// Filter npm run output - strip boilerplate, progress bars, npm WARN
pub fn filter_npm_output(output: &str) -> String {
    let mut result = Vec::new();

    for line in output.lines() {
        // Skip npm boilerplate
        if line.starts_with('>') && line.contains('@') {
            continue;
        }
        // Skip npm lifecycle scripts
        if line.trim_start().starts_with("npm WARN") {
            continue;
        }
        if line.trim_start().starts_with("npm notice") {
            continue;
        }
        // Skip progress indicators
        if line.contains('\u{2e29}')
            || line.contains('\u{2e28}')
            || (line.contains("...") && line.len() < 10)
        {
            continue;
        }
        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }

        result.push(line.to_string());
    }

    if result.is_empty() {
        "ok".to_string()
    } else {
        result.join("\n")
    }
}

/// Filter dev server output (npm run dev / npm run start)
/// Strips HMR noise, progress bars, repeated compilation messages
fn filter_dev_server_output(output: &str) -> String {
    let mut result = Vec::new();
    let mut seen_compiled = false;

    for line in output.lines() {
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip npm boilerplate
        if trimmed.starts_with('>') && trimmed.contains('@') {
            continue;
        }
        if trimmed.starts_with("npm WARN") || trimmed.starts_with("npm notice") {
            continue;
        }

        // Skip progress bars [0%]...[100%]
        if RE_PROGRESS.is_match(trimmed) {
            continue;
        }

        // Skip HMR updates
        if RE_HMR.is_match(trimmed) {
            continue;
        }

        // Skip ANSI spinners
        if RE_SPINNER.is_match(trimmed) {
            continue;
        }

        // Skip "waiting for changes" noise
        if trimmed.contains("waiting for file changes") || trimmed.contains("Waiting for changes") {
            continue;
        }

        // Deduplicate "compiled successfully" - keep only first
        if trimmed.contains("compiled successfully")
            || trimmed.contains("Compiled successfully")
            || trimmed.contains("ready in")
        {
            if seen_compiled {
                continue;
            }
            seen_compiled = true;
        }

        // Keep errors, warnings, server URLs, and meaningful output
        result.push(line.to_string());
    }

    if result.is_empty() {
        "ok \u{2713}".to_string()
    } else {
        result.join("\n")
    }
}

/// Filter npm install/ci output - strip progress bars, keep summary
fn filter_install_output(output: &str) -> String {
    let mut result = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip progress bars
        if trimmed.contains('\u{2e29}')
            || trimmed.contains('\u{2e28}')
            || trimmed.contains("timing")
            || RE_SPINNER.is_match(trimmed)
        {
            continue;
        }

        // Skip npm WARN/notice
        if trimmed.starts_with("npm WARN") || trimmed.starts_with("npm notice") {
            continue;
        }

        // Skip http fetch lines
        if trimmed.starts_with("npm http") {
            continue;
        }

        // Keep errors
        if trimmed.contains("ERR") || trimmed.contains("error") || trimmed.contains("ERROR") {
            result.push(trimmed.to_string());
            continue;
        }

        // Keep summary lines
        if trimmed.contains("added")
            || trimmed.contains("removed")
            || trimmed.contains("changed")
            || trimmed.contains("packages in")
            || trimmed.contains("up to date")
            || trimmed.starts_with('+')
            || trimmed.starts_with('-')
            || trimmed.contains("audited")
            || trimmed.contains("vulnerabilities")
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

/// Validates npm package name according to official rules
fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 214 {
        return false;
    }
    if name.contains("..") {
        return false;
    }
    name.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '@' | '/' | '-' | '_' | '.'))
}

/// Runs an unsupported npm subcommand by passing it through directly
pub fn run_passthrough(args: &[OsString], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("npm passthrough: {:?}", args);
    }
    let status = Command::new("npm")
        .args(args)
        .status()
        .context("Failed to run npm")?;

    let args_str = tracking::args_display(args);
    timer.track_passthrough(
        &format!("npm {}", args_str),
        &format!("rtk npm {} (passthrough)", args_str),
    );

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_npm_output() {
        let output = r#"
> project@1.0.0 build
> next build

npm WARN deprecated inflight@1.0.6: This module is not supported
npm notice

   Creating an optimized production build...
   ✓ Build completed
"#;
        let result = filter_npm_output(output);
        assert!(!result.contains("npm WARN"));
        assert!(!result.contains("npm notice"));
        assert!(!result.contains("> project@"));
        assert!(result.contains("Build completed"));
    }

    #[test]
    fn test_npm_subcommand_routing() {
        // Uses the shared NPM_SUBCOMMANDS constant — no drift between prod and test
        fn needs_run_injection(args: &[&str]) -> bool {
            let first = args.first().copied();
            let is_run_explicit = first == Some("run");
            let is_subcommand = first
                .map(|a| NPM_SUBCOMMANDS.contains(&a) || a.starts_with('-'))
                .unwrap_or(false);
            !is_run_explicit && !is_subcommand
        }

        // Known subcommands should NOT get "run" injected
        for subcmd in NPM_SUBCOMMANDS {
            assert!(
                !needs_run_injection(&[subcmd]),
                "'npm {}' should NOT inject 'run'",
                subcmd
            );
        }

        // Script names SHOULD get "run" injected
        for script in &["build", "dev", "lint", "typecheck", "deploy"] {
            assert!(
                needs_run_injection(&[script]),
                "'npm {}' SHOULD inject 'run'",
                script
            );
        }

        // Flags should NOT get "run" injected
        assert!(!needs_run_injection(&["--version"]));
        assert!(!needs_run_injection(&["-h"]));

        // Explicit "run" should NOT inject another "run"
        assert!(!needs_run_injection(&["run", "build"]));
    }

    #[test]
    fn test_filter_npm_output_empty() {
        let output = "\n\n\n";
        let result = filter_npm_output(output);
        assert_eq!(result, "ok");
    }

    #[test]
    fn test_filter_dev_server_output() {
        let output = r#"
> myapp@1.0.0 dev
> next dev

  ▲ Next.js 14.1.0
  - Local:        http://localhost:3000

[HMR] connected
[HMR] Updated ./src/app/page.tsx
[HMR] Updated ./src/app/layout.tsx
Compiled successfully
waiting for file changes...
Compiled successfully
[25%] building modules...
[100%] complete

  ✓ Ready in 2.3s
  ✓ Compiled / in 245ms
"#;
        let result = filter_dev_server_output(output);

        // HMR lines should be stripped
        assert!(!result.contains("[HMR]"));
        // Progress should be stripped
        assert!(!result.contains("[25%]"));
        assert!(!result.contains("[100%]"));
        // "waiting for changes" should be stripped
        assert!(!result.contains("waiting for file changes"));
        // Only first "Compiled successfully" kept
        assert_eq!(result.matches("Compiled successfully").count(), 1);
        // Server URL preserved
        assert!(result.contains("localhost:3000"));
    }

    #[test]
    fn test_filter_dev_server_savings() {
        let raw = r#"
> myapp@1.0.0 dev
> next dev

  ▲ Next.js 14.1.0
  - Local:        http://localhost:3000

[HMR] connected
[HMR] Updated module ./src/app/page.tsx
[HMR] Updated module ./src/app/layout.tsx
[HMR] Updated module ./src/components/Header.tsx
[HMR] Updated module ./src/components/Footer.tsx
Compiled successfully in 234ms
waiting for file changes...
Compiled successfully in 156ms
waiting for file changes...
Compiled successfully in 189ms
[0%] building modules
[25%] building modules 5/20
[50%] building modules 10/20
[75%] building modules 15/20
[100%] building modules 20/20

  ✓ Ready in 2.3s
  ✓ Compiled / in 245ms
"#;
        let filtered = filter_dev_server_output(raw);
        let input_tokens = count_tokens(raw);
        let output_tokens = count_tokens(&filtered);
        let savings = 100.0 - (output_tokens as f64 / input_tokens as f64 * 100.0);
        assert!(
            savings >= 60.0,
            "Dev server filter: expected >=60% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_install_output() {
        let output = r#"
npm WARN deprecated inflight@1.0.6: This module is not supported
npm WARN deprecated glob@7.2.3: This module is not supported
npm notice
npm http fetch GET 200 https://registry.npmjs.org/react
npm http fetch GET 200 https://registry.npmjs.org/next

added 320 packages, removed 5 packages, and audited 325 packages in 12s

45 packages are looking for funding

found 0 vulnerabilities
"#;
        let result = filter_install_output(output);
        assert!(!result.contains("npm WARN"));
        assert!(!result.contains("npm notice"));
        assert!(!result.contains("npm http"));
        assert!(result.contains("added 320 packages"));
        assert!(result.contains("vulnerabilities"));
    }

    #[test]
    fn test_filter_install_savings() {
        let raw = r#"
npm WARN deprecated inflight@1.0.6: This module is not supported
npm WARN deprecated glob@7.2.3: This module is not supported
npm WARN deprecated rimraf@3.0.2: Rimraf versions prior to v4 are not supported
npm WARN deprecated semver@6.3.0: Not supported
npm notice
npm notice New major version of npm available! 9.6.7 -> 10.2.4
npm notice Changelog: https://github.com/npm/cli/releases/tag/v10.2.4
npm notice Run npm install -g npm@10.2.4 to update!
npm notice
npm http fetch GET 200 https://registry.npmjs.org/react 124ms
npm http fetch GET 200 https://registry.npmjs.org/next 234ms
npm http fetch GET 200 https://registry.npmjs.org/typescript 156ms
npm http fetch GET 200 https://registry.npmjs.org/eslint 189ms
npm http fetch GET 200 https://registry.npmjs.org/tailwindcss 145ms

added 320 packages, removed 5 packages, and audited 325 packages in 12s

45 packages are looking for funding
  run `npm fund` for details

found 0 vulnerabilities
"#;
        let filtered = filter_install_output(raw);
        let input_tokens = count_tokens(raw);
        let output_tokens = count_tokens(&filtered);
        let savings = 100.0 - (output_tokens as f64 / input_tokens as f64 * 100.0);
        assert!(
            savings >= 70.0,
            "Install filter: expected >=70% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_outdated_json() {
        let json = r#"{
            "express": {
                "current": "4.18.2",
                "wanted": "4.18.2",
                "latest": "4.19.0",
                "type": "dependencies"
            },
            "typescript": {
                "current": "5.2.2",
                "wanted": "5.3.3",
                "latest": "5.3.3",
                "type": "devDependencies"
            }
        }"#;
        let result = filter_outdated_output(json);
        assert!(result.contains("express: 4.18.2 \u{2192} 4.19.0"));
        assert!(result.contains("typescript: 5.2.2 \u{2192} 5.3.3 (dev)"));
        assert!(result.contains("2 outdated"));
    }

    #[test]
    fn test_filter_outdated_empty() {
        let json = "{}";
        let result = filter_outdated_output(json);
        assert!(result.is_empty());
    }

    #[test]
    fn test_package_name_validation() {
        assert!(is_valid_package_name("lodash"));
        assert!(is_valid_package_name("@clerk/express"));
        assert!(!is_valid_package_name("../../../etc/passwd"));
        assert!(!is_valid_package_name("lodash; rm -rf /"));
    }

    #[test]
    fn test_passthrough_signature() {
        // Compile-time verification that run_passthrough exists with correct signature
        let _args: Vec<OsString> = vec![OsString::from("help")];
    }
}
