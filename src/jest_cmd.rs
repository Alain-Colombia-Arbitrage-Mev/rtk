use crate::tracking;
use crate::utils::{package_manager_exec, strip_ansi};
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;

lazy_static! {
    static ref RE_JEST_PASS: Regex = Regex::new(r"(?m)^\s*(PASS|FAIL)\s+(.+)$").unwrap();
    static ref RE_JEST_SUMMARY: Regex =
        Regex::new(r"(?i)(Tests?|Suites?|Snapshots?):\s+(.+)").unwrap();
    static ref RE_JEST_TIME: Regex = Regex::new(r"(?i)Time:\s+(.+)").unwrap();
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JestJsonOutput {
    #[serde(rename = "numPassedTests")]
    num_passed_tests: Option<i64>,
    #[serde(rename = "numFailedTests")]
    num_failed_tests: Option<i64>,
    #[serde(rename = "numPendingTests")]
    num_pending_tests: Option<i64>,
    #[serde(rename = "numTotalTests")]
    num_total_tests: Option<i64>,
    #[serde(rename = "numPassedTestSuites")]
    num_passed_test_suites: Option<i64>,
    #[serde(rename = "numFailedTestSuites")]
    num_failed_test_suites: Option<i64>,
    #[serde(rename = "testResults")]
    test_results: Option<Vec<JestTestResult>>,
    success: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JestTestResult {
    name: Option<String>,
    status: Option<String>,
    message: Option<String>,
    #[serde(rename = "assertionResults")]
    assertion_results: Option<Vec<JestAssertionResult>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JestAssertionResult {
    #[serde(rename = "ancestorTitles")]
    ancestor_titles: Option<Vec<String>>,
    #[serde(rename = "fullName")]
    full_name: Option<String>,
    status: Option<String>,
    #[serde(rename = "failureMessages")]
    failure_messages: Option<Vec<String>>,
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = package_manager_exec("jest");

    // Inject --json for structured output
    if !args.iter().any(|a| a == "--json") {
        cmd.arg("--json");
    }

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: jest --json {}", args.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run jest. Is Jest installed?")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let exit_code = output
        .status
        .code()
        .unwrap_or(if output.status.success() { 0 } else { 1 });

    // Try JSON parsing first (stdout), fallback to text
    let filtered = match try_filter_jest_json(&stdout) {
        Some(f) => f,
        None => {
            // Jest sometimes writes JSON to stderr when tests fail
            match try_filter_jest_json(&stderr) {
                Some(f) => f,
                None => filter_jest_text(&strip_ansi(&raw)),
            }
        }
    };

    if let Some(hint) = crate::tee::tee_and_hint(&raw, "jest", exit_code) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    timer.track(
        &format!("jest {}", args.join(" ")),
        &format!("rtk jest {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(exit_code);
    }

    Ok(())
}

fn try_filter_jest_json(output: &str) -> Option<String> {
    // Jest --json output can be preceded by console.log noise
    // Find the JSON object
    let json_str = crate::parser::extract_json_object(output)?;
    let data: JestJsonOutput = serde_json::from_str(json_str).ok()?;

    let passed = data.num_passed_tests.unwrap_or(0);
    let failed = data.num_failed_tests.unwrap_or(0);
    let skipped = data.num_pending_tests.unwrap_or(0);

    if passed == 0 && failed == 0 && skipped == 0 {
        return None;
    }

    let mut lines = vec![format!("PASS ({}) FAIL ({})", passed, failed)];

    // Show failures
    if failed > 0 {
        if let Some(results) = &data.test_results {
            let mut failure_idx = 0;
            for result in results {
                if let Some(assertions) = &result.assertion_results {
                    for assertion in assertions {
                        if assertion.status.as_deref() == Some("failed") {
                            failure_idx += 1;
                            if failure_idx > 5 {
                                continue;
                            }
                            let name = assertion.full_name.as_deref().unwrap_or("unknown test");
                            lines.push(format!("\n{}. {}", failure_idx, name));
                            if let Some(messages) = &assertion.failure_messages {
                                for msg in messages.iter().take(1) {
                                    let clean = strip_ansi(msg);
                                    let preview: String =
                                        clean.lines().take(3).collect::<Vec<_>>().join("\n   ");
                                    lines.push(format!("   {}", preview));
                                }
                            }
                        }
                    }
                }
            }
            if failure_idx > 5 {
                lines.push(format!("\n... +{} more failures", failure_idx - 5));
            }
        }
    }

    if skipped > 0 {
        lines.push(format!("\nSkipped: {}", skipped));
    }

    Some(lines.join("\n"))
}

fn filter_jest_text(output: &str) -> String {
    let mut lines = Vec::new();
    let mut in_failure = false;
    let mut failure_lines = 0;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_failure {
                in_failure = false;
            }
            continue;
        }

        // Capture PASS/FAIL lines
        if RE_JEST_PASS.is_match(trimmed) {
            if trimmed.contains("FAIL") {
                lines.push(trimmed.to_string());
                in_failure = true;
                failure_lines = 0;
            }
            continue;
        }

        // Capture summary lines
        if RE_JEST_SUMMARY.is_match(trimmed) || RE_JEST_TIME.is_match(trimmed) {
            lines.push(trimmed.to_string());
            continue;
        }

        // Include failure details (limited)
        if in_failure && failure_lines < 5 {
            lines.push(format!("  {}", trimmed));
            failure_lines += 1;
        }
    }

    if lines.is_empty() {
        return crate::utils::truncate(output, 2000);
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
    fn test_filter_jest_json() {
        let input = r#"{
            "numPassedTests": 45,
            "numFailedTests": 2,
            "numPendingTests": 1,
            "numTotalTests": 48,
            "numPassedTestSuites": 8,
            "numFailedTestSuites": 1,
            "success": false,
            "testResults": [
                {
                    "name": "src/__tests__/utils.test.ts",
                    "status": "failed",
                    "message": "",
                    "assertionResults": [
                        {
                            "ancestorTitles": ["Utils"],
                            "fullName": "Utils formatDate returns ISO string",
                            "status": "failed",
                            "failureMessages": ["Expected: \"2024-01-15\"\nReceived: \"2024-1-15\"\n    at Object.<anonymous> (src/__tests__/utils.test.ts:25:3)"]
                        },
                        {
                            "ancestorTitles": ["Utils"],
                            "fullName": "Utils parseDate handles invalid input",
                            "status": "failed",
                            "failureMessages": ["TypeError: Cannot read property 'split' of null\n    at parseDate (src/utils.ts:10:5)"]
                        }
                    ]
                },
                {
                    "name": "src/__tests__/api.test.ts",
                    "status": "passed",
                    "message": "",
                    "assertionResults": [
                        {
                            "ancestorTitles": ["API"],
                            "fullName": "API fetches data",
                            "status": "passed",
                            "failureMessages": []
                        }
                    ]
                }
            ]
        }"#;

        let output = try_filter_jest_json(input).unwrap();
        assert!(output.contains("PASS (45) FAIL (2)"));
        assert!(output.contains("formatDate"));
        assert!(output.contains("parseDate"));
        assert!(output.contains("Skipped: 1"));

        let savings = 100.0 - (count_tokens(&output) as f64 / count_tokens(input) as f64 * 100.0);
        assert!(
            savings >= 60.0,
            "Jest JSON filter: expected ≥60% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_jest_json_all_pass() {
        let input = r#"{
            "numPassedTests": 10,
            "numFailedTests": 0,
            "numPendingTests": 0,
            "numTotalTests": 10,
            "success": true,
            "testResults": []
        }"#;

        let output = try_filter_jest_json(input).unwrap();
        assert!(output.contains("PASS (10) FAIL (0)"));
    }

    #[test]
    fn test_filter_jest_text_fallback() {
        let input = "PASS src/__tests__/api.test.ts\n\
                     PASS src/__tests__/auth.test.ts\n\
                     FAIL src/__tests__/utils.test.ts\n\
                     ● Utils > formatDate returns ISO string\n\
                     Expected: \"2024-01-15\"\n\
                     Received: \"2024-1-15\"\n\
                     \n\
                     Tests:  2 passed, 1 failed, 3 total\n\
                     Time:   1.234s";

        let output = filter_jest_text(input);
        assert!(output.contains("FAIL"));
        assert!(output.contains("Tests:"));
        assert!(output.contains("Time:"));
    }

    #[test]
    fn test_filter_jest_json_invalid() {
        let input = "not valid json";
        let output = try_filter_jest_json(input);
        assert!(output.is_none());
    }

    #[test]
    fn test_filter_jest_text_empty() {
        let output = filter_jest_text("");
        assert_eq!(output, "");
    }
}
