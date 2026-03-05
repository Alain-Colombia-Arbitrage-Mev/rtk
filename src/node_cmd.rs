use crate::tracking;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::process::Command;

lazy_static! {
    /// Node.js warning lines: (node:12345) ExperimentalWarning: ...
    static ref RE_NODE_WARNING: Regex =
        Regex::new(r"^\(node:\d+\)\s+(ExperimentalWarning|DeprecationWarning|Warning):").unwrap();
    /// Hint to use --trace-warnings
    static ref RE_TRACE_HINT: Regex =
        Regex::new(r"^\(Use `node --trace-warnings").unwrap();
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("node");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: node {}", args.join(" "));
    }

    let output = cmd.output().context("Failed to run node")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let (filtered_stdout, filtered_stderr) = filter_node_output(&stdout, &stderr);
    let raw = format!("{}\n{}", stdout, stderr);
    let filtered = format!("{}\n{}", filtered_stdout, filtered_stderr)
        .trim()
        .to_string();

    if !filtered.is_empty() {
        println!("{}", filtered);
    }

    timer.track(
        &format!("node {}", args.join(" ")),
        &format!("rtk node {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

/// Filter Node.js output: strip warnings, deprecations, trace hints
fn filter_node_output(stdout: &str, stderr: &str) -> (String, String) {
    // stdout passes through unchanged
    let filtered_stdout = stdout.to_string();

    // stderr: strip Node.js warnings
    let filtered_stderr = stderr
        .lines()
        .filter(|line| !RE_NODE_WARNING.is_match(line) && !RE_TRACE_HINT.is_match(line))
        .collect::<Vec<_>>()
        .join("\n");

    (filtered_stdout, filtered_stderr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_node_warnings() {
        let stdout = "Hello, world!\nResult: 42";
        let stderr = r#"(node:12345) ExperimentalWarning: The fs.promises API is experimental
(Use `node --trace-warnings ...` to show where the warning was created)
(node:12345) DeprecationWarning: Buffer() is deprecated due to security and usability issues
(Use `node --trace-warnings ...` to show where the warning was created)
Error: Something went wrong
    at Object.<anonymous> (/app/index.js:10:15)"#;

        let (out, err) = filter_node_output(stdout, stderr);

        // stdout preserved
        assert!(out.contains("Hello, world!"));
        assert!(out.contains("Result: 42"));

        // warnings stripped
        assert!(!err.contains("ExperimentalWarning"));
        assert!(!err.contains("DeprecationWarning"));
        assert!(!err.contains("--trace-warnings"));

        // real errors preserved
        assert!(err.contains("Error: Something went wrong"));
        assert!(err.contains("at Object.<anonymous>"));
    }

    #[test]
    fn test_filter_node_no_warnings() {
        let stdout = "output line";
        let stderr = "";
        let (out, err) = filter_node_output(stdout, stderr);
        assert_eq!(out, "output line");
        assert!(err.is_empty());
    }

    #[test]
    fn test_filter_node_savings() {
        let stdout = "42";
        let stderr = r#"(node:98765) ExperimentalWarning: VM Modules is an experimental feature
(Use `node --trace-warnings ...` to show where the warning was created)
(node:98765) DeprecationWarning: crypto.DEFAULT_ENCODING is deprecated
(Use `node --trace-warnings ...` to show where the warning was created)
(node:98765) Warning: Closing file descriptor 19 on garbage collection
(Use `node --trace-warnings ...` to show where the warning was created)"#;

        let (_, filtered_err) = filter_node_output(stdout, stderr);

        let input_tokens = count_tokens(stderr);
        let output_tokens = count_tokens(&filtered_err);

        // With all warnings stripped, output should be empty (100% savings)
        assert!(input_tokens > 0, "Input should have tokens");
        assert_eq!(output_tokens, 0, "All warnings should be stripped");
    }
}
