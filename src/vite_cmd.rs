use crate::tracking;
use crate::utils::{package_manager_exec, strip_ansi};
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    // Match Vite build output lines like: dist/assets/index-abc123.js  45.23 kB │ gzip: 14.56 kB
    static ref RE_BUNDLE_LINE: Regex =
        Regex::new(r"^\s*(.+?)\s+([\d.]+)\s*(kB|KB|MB|B)\s*(?:│\s*gzip:\s*([\d.]+)\s*(kB|KB|MB|B))?")
            .unwrap();
    static ref RE_BUILD_TIME: Regex =
        Regex::new(r"(?i)built\s+in\s+([\d.]+\s*(?:ms|s|m))").unwrap();
    static ref RE_BUILD_ERROR: Regex =
        Regex::new(r"(?i)(error|ERROR|✗|failed|FAIL)").unwrap();
    static ref RE_VITE_READY: Regex =
        Regex::new(r"(?i)(ready\s+in|Local:|Network:)").unwrap();
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    // Detect if this is a "build" or "dev" command
    let is_build = args.is_empty()
        || args.iter().any(|a| a == "build")
        || (!args
            .iter()
            .any(|a| a == "dev" || a == "preview" || a == "serve"));

    if is_build {
        run_build(args, verbose)
    } else {
        run_passthrough(args, verbose)
    }
}

fn run_build(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = package_manager_exec("vite");

    // Ensure "build" subcommand if not present
    if !args.iter().any(|a| a == "build") {
        cmd.arg("build");
    }

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: vite build {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run vite build. Is Vite installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_vite_build(&strip_ansi(&raw));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "vite_build", exit_code) {
        if !filtered.is_empty() {
            println!("{}\n{}", filtered, hint);
        } else {
            println!("{}", hint);
        }
    } else if !filtered.is_empty() {
        println!("{}", filtered);
    }

    timer.track(
        &format!("vite build {}", args.join(" ")),
        &format!("rtk vite build {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

fn run_passthrough(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = package_manager_exec("vite");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: vite {}", args.join(" "));
    }

    // For dev/preview, use status() to stream output
    let status = cmd
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Failed to run vite. Is Vite installed?")?;

    let args_str = args.join(" ");
    timer.track_passthrough(
        &format!("vite {}", args_str),
        &format!("rtk vite {} (passthrough)", args_str),
    );

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

pub fn filter_vite_build(output: &str) -> String {
    let mut bundles: Vec<(String, f64, Option<f64>)> = Vec::new(); // (name, size_kb, gzip_kb)
    let mut build_time: Option<String> = None;
    let mut errors: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Capture errors
        if RE_BUILD_ERROR.is_match(trimmed)
            && !trimmed.contains("gzip")
            && !trimmed.contains("dist/")
        {
            errors.push(trimmed.to_string());
            continue;
        }

        // Capture build time
        if let Some(caps) = RE_BUILD_TIME.captures(trimmed) {
            build_time = Some(
                caps.get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
            );
            continue;
        }

        // Capture bundle lines
        if let Some(caps) = RE_BUNDLE_LINE.captures(trimmed) {
            let name = caps
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let size: f64 = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0.0);
            let unit = caps.get(3).map(|m| m.as_str()).unwrap_or("kB");
            let size_kb = match unit {
                "MB" => size * 1024.0,
                "B" => size / 1024.0,
                _ => size,
            };

            let gzip_kb = caps.get(4).and_then(|m| {
                let gz_size: f64 = m.as_str().parse().ok()?;
                let gz_unit = caps.get(5).map(|m| m.as_str()).unwrap_or("kB");
                Some(match gz_unit {
                    "MB" => gz_size * 1024.0,
                    "B" => gz_size / 1024.0,
                    _ => gz_size,
                })
            });

            bundles.push((name, size_kb, gzip_kb));
        }
    }

    let mut lines = Vec::new();

    // Show errors first
    if !errors.is_empty() {
        for err in &errors {
            lines.push(err.clone());
        }
        lines.push(String::new());
    }

    // Show bundles (only those >10KB, or all if fewer than 5)
    if !bundles.is_empty() {
        let significant: Vec<_> = bundles.iter().filter(|(_, size, _)| *size > 10.0).collect();
        let show = if significant.len() < 5 {
            &bundles
        } else {
            // Show only significant bundles sorted by size
            let mut sorted = significant;
            sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            &bundles // show all, sorted below
        };

        let mut sorted_bundles = show.to_vec();
        sorted_bundles.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        for (name, size_kb, gzip_kb) in sorted_bundles.iter().take(10) {
            let gzip_info = gzip_kb
                .map(|gz| format!(" (gzip: {:.1} kB)", gz))
                .unwrap_or_default();
            lines.push(format!("{}: {:.1} kB{}", name, size_kb, gzip_info));
        }

        if sorted_bundles.len() > 10 {
            lines.push(format!("... +{} more files", sorted_bundles.len() - 10));
        }

        // Total size
        let total_kb: f64 = bundles.iter().map(|(_, s, _)| s).sum();
        let total_gzip: f64 = bundles.iter().filter_map(|(_, _, g)| *g).sum();
        if total_gzip > 0.0 {
            lines.push(format!(
                "\nTotal: {:.1} kB (gzip: {:.1} kB)",
                total_kb, total_gzip
            ));
        } else {
            lines.push(format!("\nTotal: {:.1} kB", total_kb));
        }
    }

    if let Some(time) = build_time {
        lines.push(format!("Built in {}", time));
    }

    if lines.is_empty() {
        return "Build succeeded ✓".to_string();
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_vite_build() {
        let input = "vite v5.0.0 building for production...\n\
                     transforming (1234) modules...\n\
                     transforming (2345) modules...\n\
                     transforming (3456) modules...\n\
                     rendering chunks (1)...\n\
                     rendering chunks (2)...\n\
                     rendering chunks (3)...\n\
                     computing gzip size (1)...\n\
                     computing gzip size (2)...\n\
                     computing gzip size (3)...\n\
                     dist/assets/index-abc123.js    145.23 kB │ gzip: 45.67 kB\n\
                     dist/assets/index-def456.css    22.10 kB │ gzip:  5.30 kB\n\
                     dist/assets/vendor-ghi789.js   312.50 kB │ gzip: 98.20 kB\n\
                     dist/assets/logo-jkl012.svg      1.20 kB\n\
                     dist/index.html                   0.45 kB │ gzip:  0.30 kB\n\
                     ✓ built in 3.45s";

        let output = filter_vite_build(input);
        assert!(output.contains("vendor"));
        assert!(output.contains("index"));
        assert!(output.contains("Total:"));
        assert!(output.contains("Built in"));
        assert!(!output.contains("transforming"));
        assert!(!output.contains("rendering"));
    }

    #[test]
    fn test_filter_vite_build_empty() {
        let input = "vite v5.0.0 building for production...\ntransforming...\nrendering chunks...";
        let output = filter_vite_build(input);
        assert_eq!(output, "Build succeeded ✓");
    }

    #[test]
    fn test_filter_vite_build_with_errors() {
        let input = "vite v5.0.0 building for production...\n\
                     ERROR: Could not resolve './missing-module'\n\
                     error during build:\n\
                     RollupError: missing-module not found";

        let output = filter_vite_build(input);
        assert!(output.contains("ERROR"));
    }

    #[test]
    fn test_bundle_regex() {
        let line = "dist/assets/index-abc123.js  145.23 kB │ gzip: 45.67 kB";
        assert!(RE_BUNDLE_LINE.is_match(line));

        let caps = RE_BUNDLE_LINE.captures(line).unwrap();
        assert!(caps.get(1).unwrap().as_str().contains("index"));
        assert_eq!(caps.get(2).unwrap().as_str(), "145.23");
    }
}
