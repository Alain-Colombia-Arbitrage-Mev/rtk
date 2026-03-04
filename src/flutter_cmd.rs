use crate::tracking;
use crate::utils::strip_ansi;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::ffi::OsString;
use std::process::Command;

lazy_static! {
    static ref RE_PROGRESS: Regex = Regex::new(
        r"(?i)^\s*(downloading|resolving|got dependencies|building|compiling|assembling)"
    )
    .unwrap();
    static ref RE_BUILT_APK: Regex = Regex::new(r"(?i)^\s*✓\s+Built\s+(.+)").unwrap();
    static ref RE_BUILT_PATH: Regex =
        Regex::new(r"(?i)(build/app/outputs|\.apk|\.aab|\.ipa|\.app)").unwrap();
    static ref RE_ANALYZE_ISSUE: Regex =
        Regex::new(r"^\s*(info|warning|error)\s+[•·-]\s+(.+)").unwrap();
    static ref RE_ANALYZE_LOCATION: Regex = Regex::new(r"^\s+(.+\.dart:\d+:\d+)").unwrap();
    static ref RE_PUB_PROGRESS: Regex =
        Regex::new(r"(?i)^\s*(resolving dependencies|downloading|got dependencies|changed \d+)")
            .unwrap();
}

pub fn run_test(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("flutter");
    cmd.arg("test");

    // Inject --machine for JSON output if not already specified
    if !args.iter().any(|a| a == "--machine" || a == "--reporter") {
        cmd.arg("--machine");
    }

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: flutter test --machine {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run flutter test. Is Flutter installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_flutter_test(&stdout);

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "flutter_test", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim());
    }

    timer.track(
        &format!("flutter test {}", args.join(" ")),
        &format!("rtk flutter test {}", args.join(" ")),
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

    let mut cmd = Command::new("flutter");
    cmd.arg("build");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: flutter build {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run flutter build. Is Flutter installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_flutter_build(&strip_ansi(&format!("{}\n{}", stdout, stderr)));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "flutter_build", exit_code) {
        if !filtered.is_empty() {
            println!("{}\n{}", filtered, hint);
        } else {
            println!("{}", hint);
        }
    } else if !filtered.is_empty() {
        println!("{}", filtered);
    }

    timer.track(
        &format!("flutter build {}", args.join(" ")),
        &format!("rtk flutter build {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

pub fn run_analyze(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("flutter");
    cmd.arg("analyze");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: flutter analyze {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run flutter analyze. Is Flutter installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_flutter_analyze(&strip_ansi(&stdout));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "flutter_analyze", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    timer.track(
        &format!("flutter analyze {}", args.join(" ")),
        &format!("rtk flutter analyze {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

pub fn run_pub(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("flutter");
    cmd.arg("pub");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: flutter pub {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run flutter pub. Is Flutter installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_flutter_pub(&strip_ansi(&format!("{}\n{}", stdout, stderr)));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "flutter_pub", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    timer.track(
        &format!("flutter pub {}", args.join(" ")),
        &format!("rtk flutter pub {}", args.join(" ")),
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
        anyhow::bail!("flutter: no subcommand specified");
    }

    let timer = tracking::TimedExecution::start();

    let subcommand = args[0].to_string_lossy();
    let mut cmd = Command::new("flutter");
    cmd.arg(&*subcommand);

    for arg in &args[1..] {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: flutter {} ...", subcommand);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run flutter {}", subcommand))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    print!("{}", stdout);
    eprint!("{}", stderr);

    timer.track(
        &format!("flutter {}", subcommand),
        &format!("rtk flutter {}", subcommand),
        &raw,
        &raw,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

// --- Filter functions ---

fn filter_flutter_test(output: &str) -> String {
    use std::collections::HashMap;

    let mut tests: HashMap<i64, String> = HashMap::new(); // id -> name
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(String, String, Option<String>)> = Vec::new(); // (name, error, stack)
    let mut duration_ms: Option<i64> = None;

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        let event: serde_json::Value = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "testStart" => {
                if let Some(test) = event.get("test") {
                    let id = test.get("id").and_then(|v| v.as_i64()).unwrap_or(-1);
                    let name = test
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    tests.insert(id, name.to_string());
                }
            }
            "testDone" => {
                if let Some(result) = event.get("result").and_then(|v| v.as_str()) {
                    match result {
                        "success" => passed += 1,
                        "failure" | "error" => failed += 1,
                        _ => skipped += 1,
                    }
                }
            }
            "error" => {
                let test_id = event.get("testID").and_then(|v| v.as_i64()).unwrap_or(-1);
                let test_name = tests
                    .get(&test_id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let error = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let stack = event
                    .get("stackTrace")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                failures.push((test_name, error, stack));
            }
            "done" => {
                if let Some(t) = event.get("time").and_then(|v| v.as_i64()) {
                    duration_ms = Some(t);
                }
            }
            _ => {}
        }
    }

    let total = passed + failed + skipped;
    if total == 0 {
        // Fallback: couldn't parse JSON, return truncated raw
        return crate::utils::truncate(output, 2000);
    }

    let mut lines = vec![format!("PASS ({}) FAIL ({})", passed, failed)];

    if !failures.is_empty() {
        lines.push(String::new());
        for (idx, (name, error, stack)) in failures.iter().enumerate().take(5) {
            lines.push(format!("{}. {}", idx + 1, name));
            let error_preview: String = error.lines().take(2).collect::<Vec<_>>().join(" ");
            lines.push(format!("   {}", error_preview));
            if let Some(st) = stack {
                let stack_preview: String = st.lines().take(2).collect::<Vec<_>>().join("\n   ");
                lines.push(format!("   {}", stack_preview));
            }
        }
        if failures.len() > 5 {
            lines.push(format!("\n... +{} more failures", failures.len() - 5));
        }
    }

    if let Some(d) = duration_ms {
        lines.push(format!("\nTime: {}ms", d));
    }

    lines.join("\n")
}

fn filter_flutter_build(output: &str) -> String {
    let mut lines = Vec::new();
    let mut has_error = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Keep error/warning lines
        if trimmed.contains("error:") || trimmed.contains("Error:") || trimmed.starts_with("E/") {
            lines.push(trimmed.to_string());
            has_error = true;
            continue;
        }
        if trimmed.contains("warning:") || trimmed.contains("Warning:") {
            lines.push(trimmed.to_string());
            continue;
        }

        // Keep built artifact path
        if RE_BUILT_APK.is_match(trimmed) || RE_BUILT_PATH.is_match(trimmed) {
            lines.push(trimmed.to_string());
            continue;
        }

        // Keep final build time
        if trimmed.contains("Built ") && trimmed.contains("(") {
            lines.push(trimmed.to_string());
            continue;
        }

        // Skip progress noise
        if RE_PROGRESS.is_match(trimmed) {
            continue;
        }
    }

    if !has_error && lines.is_empty() {
        return "Build succeeded ✓".to_string();
    }

    lines.join("\n")
}

fn filter_flutter_analyze(output: &str) -> String {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    let mut issues: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(caps) = RE_ANALYZE_ISSUE.captures(trimmed) {
            let severity = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let message = caps.get(2).map(|m| m.as_str()).unwrap_or(trimmed);

            match severity.to_lowercase().as_str() {
                "error" => {
                    errors += 1;
                    issues.push(format!("✗ {}", message));
                }
                "warning" => {
                    warnings += 1;
                    issues.push(format!("⚠ {}", message));
                }
                "info" => {
                    infos += 1;
                }
                _ => {}
            }
        }

        // Also check for direct error patterns
        if (trimmed.contains("error •") || trimmed.contains("error -"))
            && !issues
                .iter()
                .any(|i| i.contains(&trimmed[..trimmed.len().min(40)]))
        {
            errors += 1;
            issues.push(format!("✗ {}", trimmed));
        }

        // Check for "No issues found" shortcut
        if trimmed.contains("No issues found") {
            return "No issues found ✓".to_string();
        }
    }

    let total = errors + warnings + infos;
    if total == 0 {
        return "No issues found ✓".to_string();
    }

    let mut lines = vec![format!(
        "Errors: {} | Warnings: {} | Infos: {}",
        errors, warnings, infos
    )];

    if !issues.is_empty() {
        lines.push(String::new());
        for issue in issues.iter().take(10) {
            lines.push(issue.clone());
        }
        if issues.len() > 10 {
            lines.push(format!("\n... +{} more issues", issues.len() - 10));
        }
    }

    lines.join("\n")
}

fn filter_flutter_pub(output: &str) -> String {
    let mut lines = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Keep summary/result lines
        if trimmed.starts_with("Changed ")
            || trimmed.starts_with("Got dependencies")
            || trimmed.starts_with("No dependencies changed")
            || trimmed.contains("is up to date")
            || trimmed.contains("added")
            || trimmed.contains("removed")
            || trimmed.contains("changed")
            || trimmed.contains("error")
            || trimmed.contains("Error")
            || trimmed.contains("warning")
        {
            lines.push(trimmed.to_string());
            continue;
        }

        // Skip resolving/downloading noise
        if RE_PUB_PROGRESS.is_match(trimmed) && !trimmed.starts_with("Changed") {
            continue;
        }
    }

    if lines.is_empty() {
        return "ok ✓".to_string();
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
    fn test_filter_flutter_test_json() {
        // Realistic flutter test --machine output with many passing tests
        let input = r#"{"type":"allSuites","count":3,"time":0}
{"type":"suite","suite":{"id":0,"platform":"vm","path":"test/counter_test.dart"},"time":5}
{"type":"group","group":{"id":1,"suiteID":0,"name":"Counter"},"time":10}
{"type":"testStart","test":{"id":2,"name":"Counter increments smoke test","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":15}
{"type":"testDone","testID":2,"result":"success","hidden":false,"skipped":false,"time":50}
{"type":"testStart","test":{"id":3,"name":"Counter decrements smoke test","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":55}
{"type":"testDone","testID":3,"result":"success","hidden":false,"skipped":false,"time":80}
{"type":"testStart","test":{"id":4,"name":"Counter multiply test","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":85}
{"type":"testDone","testID":4,"result":"success","hidden":false,"skipped":false,"time":100}
{"type":"testStart","test":{"id":5,"name":"Counter divide test","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":105}
{"type":"testDone","testID":5,"result":"success","hidden":false,"skipped":false,"time":120}
{"type":"testStart","test":{"id":6,"name":"Counter resets to zero","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":125}
{"type":"error","testID":6,"error":"Expected: <0>\n  Actual: <1>\n  Which: is not equal to expected value","stackTrace":"package:test_api/src/expect.dart 18:3\ntest/counter_test.dart:25:5\n","time":130}
{"type":"testDone","testID":6,"result":"failure","hidden":false,"skipped":false,"time":135}
{"type":"testStart","test":{"id":7,"name":"Counter handles negative","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":140}
{"type":"testDone","testID":7,"result":"success","hidden":false,"skipped":false,"time":155}
{"type":"testStart","test":{"id":8,"name":"Counter overflow check","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":160}
{"type":"testDone","testID":8,"result":"success","hidden":false,"skipped":false,"time":175}
{"type":"done","success":false,"time":200}"#;

        let output = filter_flutter_test(input);
        assert!(output.contains("PASS (6) FAIL (1)"));
        assert!(output.contains("Counter resets"));
        assert!(output.contains("Expected: <0>"));
        assert!(output.contains("Time: 200ms"));

        let savings = 100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
        assert!(
            savings >= 60.0,
            "Flutter test filter: expected ≥60% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_flutter_test_all_pass() {
        let input = r#"{"type":"testStart","test":{"id":1,"name":"Widget renders","groupIDs":[0]},"time":100}
{"type":"testDone","testID":1,"result":"success","time":110}
{"type":"testStart","test":{"id":2,"name":"Widget taps","groupIDs":[0]},"time":120}
{"type":"testDone","testID":2,"result":"success","time":130}
{"type":"done","success":true,"time":200}"#;

        let output = filter_flutter_test(input);
        assert!(output.contains("PASS (2) FAIL (0)"));
    }

    #[test]
    fn test_filter_flutter_build() {
        let input = "Running Gradle task 'assembleRelease'...\n\
                     Downloading https://services.gradle.org/distributions/gradle-8.0-all.zip\n\
                     Resolving dependencies...\n\
                     Compiling lib/main.dart...\n\
                     Building with sound null safety\n\
                     warning: unused import\n\
                     ✓ Built build/app/outputs/flutter-apk/app-release.apk (24.5MB)\n\
                     Build completed in 45.2s";

        let output = filter_flutter_build(input);
        assert!(output.contains("app-release.apk"));
        assert!(output.contains("warning"));
        assert!(!output.contains("Downloading"));
        assert!(!output.contains("Resolving"));

        let savings = 100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
        assert!(
            savings >= 50.0,
            "Flutter build filter: expected ≥50% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_flutter_build_success_no_output() {
        let input = "Running Gradle task 'assembleRelease'...\n\
                     Resolving dependencies...\n\
                     Compiling lib/main.dart...";

        let output = filter_flutter_build(input);
        assert_eq!(output, "Build succeeded ✓");
    }

    #[test]
    fn test_filter_flutter_analyze_no_issues() {
        let input = "Analyzing project...\nNo issues found!";
        let output = filter_flutter_analyze(input);
        assert!(output.contains("No issues found"));
    }

    #[test]
    fn test_filter_flutter_analyze_with_issues() {
        let input = "Analyzing project...\n\
                     info • Unused import • lib/main.dart:3:1\n\
                     warning • Missing return type • lib/widget.dart:10:5\n\
                     error • Undefined name 'foo' • lib/app.dart:25:3\n\
                     3 issues found (1 error, 1 warning, 1 info)";

        let output = filter_flutter_analyze(input);
        assert!(output.contains("Errors:"));
        assert!(output.contains("Warnings:"));
    }

    #[test]
    fn test_filter_flutter_pub() {
        let input = "Resolving dependencies...\n\
                     Downloading packages...\n\
                     Got dependencies!\n\
                     Changed 5 dependencies!\n\
                     2 packages added, 1 removed, 2 changed";

        let output = filter_flutter_pub(input);
        assert!(output.contains("Got dependencies"));
        assert!(output.contains("Changed 5"));
        assert!(!output.contains("Downloading"));
    }

    #[test]
    fn test_filter_flutter_test_empty_input() {
        let output = filter_flutter_test("");
        // Empty input → fallback to truncated raw (which is empty string)
        assert_eq!(output, "");
    }

    #[test]
    fn test_filter_flutter_test_malformed_json() {
        let input = "not valid json\nmore garbage\nstill not json";
        let output = filter_flutter_test(input);
        assert!(!output.is_empty()); // Should fallback gracefully
    }
}
