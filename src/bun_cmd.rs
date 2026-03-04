use crate::tracking;
use crate::utils::strip_ansi;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::ffi::OsString;
use std::process::Command;

lazy_static! {
    static ref RE_BUN_TEST_PASS: Regex = Regex::new(r"(?m)^\s*✓\s+(.+)$").unwrap();
    static ref RE_BUN_TEST_FAIL: Regex = Regex::new(r"(?m)^\s*✗\s+(.+)$").unwrap();
    static ref RE_BUN_TEST_SKIP: Regex = Regex::new(r"(?m)^\s*-\s+(.+)$").unwrap();
    static ref RE_BUN_TEST_SUMMARY: Regex = Regex::new(r"(?i)(\d+)\s+pass.*?(\d+)\s+fail").unwrap();
    static ref RE_BUN_INSTALL_PROGRESS: Regex =
        Regex::new(r"(?i)^\s*(\[\d+/\d+\]|Resolving|Downloading|Extracting)").unwrap();
    static ref RE_BUN_INSTALL_SUMMARY: Regex =
        Regex::new(r"(?i)(installed|added|removed|resolved|packages|done)").unwrap();
    static ref RE_BUN_BUILD_OUTPUT: Regex =
        Regex::new(r"^\s*(.+?)\s+([\d.]+)\s*(KB|MB|B|kB)").unwrap();
    static ref RE_BUN_BUILD_TIME: Regex =
        Regex::new(r"(?i)(done\s+in|built\s+in)\s+([\d.]+\s*(?:ms|s))").unwrap();
}

pub fn run_test(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("bun");
    cmd.arg("test");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: bun test {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run bun test. Is Bun installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_bun_test(&strip_ansi(&raw));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "bun_test", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    timer.track(
        &format!("bun test {}", args.join(" ")),
        &format!("rtk bun test {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

pub fn run_install(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("bun");
    cmd.arg("install");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: bun install {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run bun install. Is Bun installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_bun_install(&strip_ansi(&raw));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "bun_install", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    timer.track(
        &format!("bun install {}", args.join(" ")),
        &format!("rtk bun install {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

pub fn run_build(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("bun");
    cmd.arg("build");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: bun build {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run bun build. Is Bun installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_bun_build(&strip_ansi(&raw));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "bun_build", exit_code) {
        if !filtered.is_empty() {
            println!("{}\n{}", filtered, hint);
        } else {
            println!("{}", hint);
        }
    } else if !filtered.is_empty() {
        println!("{}", filtered);
    }

    timer.track(
        &format!("bun build {}", args.join(" ")),
        &format!("rtk bun build {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

pub fn run_other(args: &[OsString], verbose: u8) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("bun: no subcommand specified");
    }

    let timer = tracking::TimedExecution::start();

    let subcommand = args[0].to_string_lossy();
    let mut cmd = Command::new("bun");
    cmd.arg(&*subcommand);

    for arg in &args[1..] {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: bun {} ...", subcommand);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run bun {}", subcommand))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    print!("{}", stdout);
    eprint!("{}", stderr);

    timer.track(
        &format!("bun {}", subcommand),
        &format!("rtk bun {}", subcommand),
        &raw,
        &raw,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

// --- Filter functions ---

fn filter_bun_test(output: &str) -> String {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let mut in_failure_block = false;
    let mut failure_detail_lines = 0;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_failure_block {
                in_failure_block = false;
            }
            continue;
        }

        // Count pass/fail/skip
        if RE_BUN_TEST_PASS.is_match(trimmed) {
            passed += 1;
            continue;
        }

        if RE_BUN_TEST_FAIL.is_match(trimmed) {
            failed += 1;
            if let Some(caps) = RE_BUN_TEST_FAIL.captures(trimmed) {
                let name = caps.get(1).map(|m| m.as_str()).unwrap_or("unknown");
                failures.push(format!("{}. {}", failed, name));
                in_failure_block = true;
                failure_detail_lines = 0;
            }
            continue;
        }

        if RE_BUN_TEST_SKIP.is_match(trimmed) {
            skipped += 1;
            continue;
        }

        // Try to parse summary line
        if let Some(caps) = RE_BUN_TEST_SUMMARY.captures(trimmed) {
            let p: usize = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let f: usize = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            if p > passed {
                passed = p;
            }
            if f > failed {
                failed = f;
            }
            continue;
        }

        // Capture failure details (limited)
        if in_failure_block && failure_detail_lines < 3 {
            failures.push(format!("   {}", trimmed));
            failure_detail_lines += 1;
        }
    }

    let total = passed + failed + skipped;
    if total == 0 {
        return crate::utils::truncate(output, 2000);
    }

    let mut lines = vec![format!("PASS ({}) FAIL ({})", passed, failed)];

    if !failures.is_empty() {
        lines.push(String::new());
        for f in failures.iter().take(20) {
            lines.push(f.clone());
        }
        if failures.len() > 20 {
            lines.push(format!("\n... +{} more lines", failures.len() - 20));
        }
    }

    if skipped > 0 {
        lines.push(format!("\nSkipped: {}", skipped));
    }

    lines.join("\n")
}

fn filter_bun_install(output: &str) -> String {
    let mut lines = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip progress bars and resolution noise
        if RE_BUN_INSTALL_PROGRESS.is_match(trimmed) {
            continue;
        }

        // Keep summary/result lines
        if RE_BUN_INSTALL_SUMMARY.is_match(trimmed)
            || trimmed.contains("error")
            || trimmed.contains("Error")
            || trimmed.contains("warn")
        {
            lines.push(trimmed.to_string());
        }
    }

    if lines.is_empty() {
        return "ok ✓".to_string();
    }

    lines.join("\n")
}

fn filter_bun_build(output: &str) -> String {
    let mut bundles: Vec<(String, String)> = Vec::new(); // (name, size_str)
    let mut build_time: Option<String> = None;
    let mut errors: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Capture errors
        if trimmed.contains("error:") || trimmed.contains("Error:") {
            errors.push(trimmed.to_string());
            continue;
        }

        // Capture build time
        if let Some(caps) = RE_BUN_BUILD_TIME.captures(trimmed) {
            build_time = Some(
                caps.get(2)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
            );
            continue;
        }

        // Capture output files
        if let Some(caps) = RE_BUN_BUILD_OUTPUT.captures(trimmed) {
            let name = caps
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let size = caps.get(2).map(|m| m.as_str()).unwrap_or("0");
            let unit = caps.get(3).map(|m| m.as_str()).unwrap_or("KB");
            bundles.push((name, format!("{} {}", size, unit)));
        }
    }

    let mut lines = Vec::new();

    if !errors.is_empty() {
        for err in &errors {
            lines.push(err.clone());
        }
    }

    if !bundles.is_empty() {
        for (name, size) in bundles.iter().take(10) {
            lines.push(format!("{}: {}", name, size));
        }
        if bundles.len() > 10 {
            lines.push(format!("... +{} more files", bundles.len() - 10));
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

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_bun_test() {
        let input = "bun test v1.0.0\n\n\
                     ✓ adds two numbers [0.50ms]\n\
                     ✓ subtracts numbers [0.30ms]\n\
                     ✓ multiplies numbers [0.20ms]\n\
                     ✗ divides by zero [1.00ms]\n\
                     Expected: no error\n\
                     Received: threw Error\n\
                     \n\
                     ✓ handles strings [0.10ms]\n\
                     - pending feature [skipped]\n\
                     \n\
                     4 pass, 1 fail, 1 skip | 6 tests";

        let output = filter_bun_test(input);
        assert!(output.contains("PASS"));
        assert!(output.contains("FAIL"));
        assert!(output.contains("divides by zero"));

        let savings = 100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
        assert!(
            savings >= 30.0,
            "Bun test filter: expected ≥30% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_bun_test_all_pass() {
        let input = "bun test v1.0.0\n\
                     ✓ test A [0.50ms]\n\
                     ✓ test B [0.30ms]\n\
                     2 pass, 0 fail | 2 tests";

        let output = filter_bun_test(input);
        assert!(output.contains("PASS (2) FAIL (0)"));
    }

    #[test]
    fn test_filter_bun_install() {
        let input = "bun install v1.0.0\n\
                     [1/5] Resolving packages...\n\
                     [2/5] Resolving packages...\n\
                     [3/5] Downloading packages...\n\
                     [4/5] Extracting packages...\n\
                     [5/5] Installing packages...\n\
                     + react@18.2.0\n\
                     + react-dom@18.2.0\n\
                     125 packages installed in 2.4s";

        let output = filter_bun_install(input);
        assert!(output.contains("installed"));
        assert!(!output.contains("Resolving"));
        assert!(!output.contains("[1/5]"));

        let savings = 100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
        assert!(
            savings >= 60.0,
            "Bun install filter: expected ≥60% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_bun_build() {
        let input = "bun build v1.0.0\n\
                     dist/index.js  45.23 KB\n\
                     dist/chunk-abc.js  12.10 KB\n\
                     done in 1.2s";

        let output = filter_bun_build(input);
        assert!(output.contains("index.js"));
        assert!(output.contains("Built in"));
    }

    #[test]
    fn test_filter_bun_build_empty() {
        let input = "bun build v1.0.0";
        let output = filter_bun_build(input);
        assert_eq!(output, "Build succeeded ✓");
    }

    #[test]
    fn test_filter_bun_test_empty() {
        let output = filter_bun_test("");
        assert_eq!(output, "");
    }

    #[test]
    fn test_filter_bun_install_empty() {
        let output = filter_bun_install("");
        assert_eq!(output, "ok ✓");
    }
}
