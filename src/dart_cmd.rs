use crate::tracking;
use crate::utils::strip_ansi;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::ffi::OsString;
use std::process::Command;

lazy_static! {
    // Format: SEVERITY|TYPE|CODE|FILE|LINE|COL|LEN|MESSAGE
    static ref RE_DART_ANALYZE_MACHINE: Regex =
        Regex::new(r"^(ERROR|WARNING|INFO)\|[^|]+\|[^|]+\|([^|]+)\|(\d+)\|(\d+)\|\d+\|(.+)$").unwrap();
    static ref RE_DART_COMPILE_SUCCESS: Regex =
        Regex::new(r"(?i)(generated|compiled|info:)\s+(.+)").unwrap();
}

pub fn run_test(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("dart");
    cmd.arg("test");

    // Inject --reporter=json for structured output
    if !args.iter().any(|a| a.starts_with("--reporter")) {
        cmd.arg("--reporter=json");
    }

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: dart test --reporter=json {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run dart test. Is Dart installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_dart_test(&stdout);

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "dart_test", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim());
    }

    timer.track(
        &format!("dart test {}", args.join(" ")),
        &format!("rtk dart test {}", args.join(" ")),
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

    let mut cmd = Command::new("dart");
    cmd.arg("analyze");

    // Inject --format=machine for structured output
    if !args.iter().any(|a| a.starts_with("--format")) {
        cmd.arg("--format=machine");
    }

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: dart analyze --format=machine {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run dart analyze. Is Dart installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_dart_analyze(&strip_ansi(&stdout));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "dart_analyze", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    timer.track(
        &format!("dart analyze {}", args.join(" ")),
        &format!("rtk dart analyze {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

pub fn run_compile(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("dart");
    cmd.arg("compile");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: dart compile {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run dart compile. Is Dart installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    let filtered = filter_dart_compile(&strip_ansi(&format!("{}\n{}", stdout, stderr)));

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "dart_compile", exit_code) {
        if !filtered.is_empty() {
            println!("{}\n{}", filtered, hint);
        } else {
            println!("{}", hint);
        }
    } else if !filtered.is_empty() {
        println!("{}", filtered);
    }

    timer.track(
        &format!("dart compile {}", args.join(" ")),
        &format!("rtk dart compile {}", args.join(" ")),
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
        anyhow::bail!("dart: no subcommand specified");
    }

    let timer = tracking::TimedExecution::start();

    let subcommand = args[0].to_string_lossy();
    let mut cmd = Command::new("dart");
    cmd.arg(&*subcommand);

    for arg in &args[1..] {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: dart {} ...", subcommand);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run dart {}", subcommand))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    print!("{}", stdout);
    eprint!("{}", stderr);

    timer.track(
        &format!("dart {}", subcommand),
        &format!("rtk dart {}", subcommand),
        &raw,
        &raw,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

// --- Filter functions ---

fn filter_dart_test(output: &str) -> String {
    use std::collections::HashMap;

    let mut tests: HashMap<i64, String> = HashMap::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(String, String, Option<String>)> = Vec::new();
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

fn filter_dart_analyze(output: &str) -> String {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    let mut issues: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Machine format: SEVERITY|TYPE|CODE|FILE|LINE|COL|MESSAGE
        if let Some(caps) = RE_DART_ANALYZE_MACHINE.captures(trimmed) {
            let severity = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let file = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let line_num = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            let message = caps.get(5).map(|m| m.as_str()).unwrap_or("");

            match severity {
                "ERROR" => {
                    errors += 1;
                    issues.push(format!("✗ {}:{} {}", file, line_num, message));
                }
                "WARNING" => {
                    warnings += 1;
                    issues.push(format!("⚠ {}:{} {}", file, line_num, message));
                }
                "INFO" => {
                    infos += 1;
                }
                _ => {}
            }
            continue;
        }

        // Fallback: check for "No issues found"
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

fn filter_dart_compile(output: &str) -> String {
    let mut lines = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Keep error lines
        if trimmed.contains("Error:") || trimmed.contains("error:") {
            lines.push(trimmed.to_string());
            continue;
        }

        // Keep final result line (Generated/Compiled/Info:)
        if RE_DART_COMPILE_SUCCESS.is_match(trimmed) {
            lines.push(trimmed.to_string());
            continue;
        }

        // Keep lines with output paths
        if trimmed.contains(".exe") || trimmed.contains(".js") || trimmed.contains(".aot") {
            lines.push(trimmed.to_string());
            continue;
        }
    }

    if lines.is_empty() {
        return "Compiled ✓".to_string();
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
    fn test_filter_dart_test_json() {
        // Realistic dart test --reporter=json output
        let input = r#"{"type":"allSuites","count":2,"time":0}
{"type":"suite","suite":{"id":0,"platform":"vm","path":"test/math_test.dart"},"time":5}
{"type":"group","group":{"id":1,"suiteID":0,"name":"Math operations"},"time":10}
{"type":"testStart","test":{"id":2,"name":"Math operations adds two numbers","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":15}
{"type":"testDone","testID":2,"result":"success","hidden":false,"skipped":false,"time":50}
{"type":"testStart","test":{"id":3,"name":"Math operations subtracts numbers","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":55}
{"type":"testDone","testID":3,"result":"success","hidden":false,"skipped":false,"time":80}
{"type":"testStart","test":{"id":4,"name":"Math operations multiplies values","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":85}
{"type":"testDone","testID":4,"result":"success","hidden":false,"skipped":false,"time":100}
{"type":"testStart","test":{"id":5,"name":"Math operations divides by zero","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":105}
{"type":"error","testID":5,"error":"Expected: no error\n  Actual: threw DivisionByZero\n  Which: threw an exception","stackTrace":"package:test_api/src/expect.dart 18:3\ntest/math_test.dart:30:5\n","time":110}
{"type":"testDone","testID":5,"result":"failure","hidden":false,"skipped":false,"time":115}
{"type":"testStart","test":{"id":6,"name":"Math operations handles large numbers","suiteID":0,"groupIDs":[1],"metadata":{"skip":false}},"time":120}
{"type":"testDone","testID":6,"result":"success","hidden":false,"skipped":false,"time":135}
{"type":"done","success":false,"time":200}"#;

        let output = filter_dart_test(input);
        assert!(output.contains("PASS (4) FAIL (1)"));
        assert!(output.contains("divides by zero"));
        assert!(output.contains("DivisionByZero"));
        assert!(output.contains("Time: 200ms"));

        let savings = 100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
        assert!(
            savings >= 50.0,
            "Dart test filter: expected ≥50% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_dart_test_all_pass() {
        let input = r#"{"type":"testStart","test":{"id":1,"name":"test A"},"time":100}
{"type":"testDone","testID":1,"result":"success","time":110}
{"type":"done","success":true,"time":200}"#;

        let output = filter_dart_test(input);
        assert!(output.contains("PASS (1) FAIL (0)"));
    }

    #[test]
    fn test_filter_dart_analyze_machine_format() {
        let input = "ERROR|COMPILE_TIME_ERROR|UNDEFINED_IDENTIFIER|lib/main.dart|25|3|42|Undefined name 'foo'\n\
                     WARNING|STATIC_WARNING|UNUSED_IMPORT|lib/utils.dart|1|1|30|Unused import\n\
                     INFO|HINT|UNNECESSARY_CAST|lib/app.dart|10|5|20|Unnecessary cast";

        let output = filter_dart_analyze(input);
        assert!(output.contains("Errors: 1"));
        assert!(output.contains("Warnings: 1"));
        assert!(output.contains("Infos: 1"));
        assert!(output.contains("main.dart"));
    }

    #[test]
    fn test_filter_dart_analyze_no_issues() {
        let input = "Analyzing project...\nNo issues found!";
        let output = filter_dart_analyze(input);
        assert!(output.contains("No issues found"));
    }

    #[test]
    fn test_filter_dart_compile() {
        let input = "Compiling lib/main.dart...\n\
                     Building AOT snapshot...\n\
                     Generated: bin/myapp.exe\n\
                     Total size: 5.2 MB";

        let output = filter_dart_compile(input);
        assert!(output.contains("myapp.exe"));
        assert!(!output.contains("Building AOT"));
    }

    #[test]
    fn test_filter_dart_compile_empty() {
        let input = "Compiling...\nLinking...";
        let output = filter_dart_compile(input);
        assert_eq!(output, "Compiled ✓");
    }

    #[test]
    fn test_filter_dart_test_empty_input() {
        let output = filter_dart_test("");
        assert_eq!(output, "");
    }
}
