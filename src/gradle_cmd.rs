use crate::tracking;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::process::Command;

lazy_static! {
    /// Gradle task execution lines: "> Task :compileJava"
    static ref RE_GRADLE_TASK: Regex =
        Regex::new(r"^>\s+Task\s+:").unwrap();
    /// Gradle download lines
    static ref RE_GRADLE_DOWNLOAD: Regex =
        Regex::new(r"(?i)^(Downloading|Download)\s+https?://").unwrap();
    /// Gradle build result
    static ref RE_GRADLE_RESULT: Regex =
        Regex::new(r"^BUILD\s+(SUCCESSFUL|FAILED)").unwrap();
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    // Try gradlew first, fallback to gradle
    let gradle_cmd = if std::path::Path::new("./gradlew").exists() {
        "./gradlew"
    } else {
        "gradle"
    };

    let mut cmd = Command::new(gradle_cmd);
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: {} {}", gradle_cmd, args.join(" "));
    }

    let output = cmd.output().context("Failed to run gradle")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_gradle(&raw);
    println!("{}", filtered);

    timer.track(
        &format!("gradle {}", args.join(" ")),
        &format!("rtk gradle {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}

/// Filter Gradle output - strip task execution/downloads, keep errors + BUILD result
fn filter_gradle(output: &str) -> String {
    let mut errors: Vec<String> = Vec::new();
    let mut result_line: Option<String> = None;
    let mut tasks_executed = 0usize;
    let mut tasks_up_to_date = 0usize;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip download lines
        if RE_GRADLE_DOWNLOAD.is_match(trimmed) {
            continue;
        }

        // Count task lines
        if RE_GRADLE_TASK.is_match(trimmed) {
            if trimmed.contains("UP-TO-DATE") || trimmed.contains("NO-SOURCE") {
                tasks_up_to_date += 1;
            } else {
                tasks_executed += 1;
            }
            continue;
        }

        // Skip "Deprecated Gradle features" noise
        if trimmed.starts_with("Deprecated Gradle features") || trimmed.contains("--warning-mode") {
            continue;
        }

        // Capture build result
        if RE_GRADLE_RESULT.is_match(trimmed) {
            result_line = Some(trimmed.to_string());
            continue;
        }

        // Capture timing
        if trimmed.contains("actionable task") || trimmed.ends_with("executed") {
            result_line = Some(trimmed.to_string());
            continue;
        }

        // Keep errors
        if trimmed.starts_with("FAILURE:")
            || trimmed.starts_with("* What went wrong:")
            || trimmed.contains("error:")
            || trimmed.contains("Error:")
        {
            errors.push(trimmed.to_string());
            continue;
        }

        // Keep execution failure detail
        if trimmed.starts_with("* Exception is:")
            || trimmed.starts_with("Execution failed for task")
        {
            errors.push(trimmed.to_string());
        }
    }

    let mut result = Vec::new();

    if !errors.is_empty() {
        for e in errors.iter().take(10) {
            result.push(e.clone());
        }
    }

    let mut summary_parts = Vec::new();
    if let Some(res) = result_line {
        summary_parts.push(res);
    }
    if tasks_executed > 0 || tasks_up_to_date > 0 {
        let mut task_info = Vec::new();
        if tasks_executed > 0 {
            task_info.push(format!("{} executed", tasks_executed));
        }
        if tasks_up_to_date > 0 {
            task_info.push(format!("{} up-to-date", tasks_up_to_date));
        }
        summary_parts.push(task_info.join(", "));
    }

    if !summary_parts.is_empty() {
        result.push(summary_parts.join(" | "));
    }

    if result.is_empty() {
        "ok \u{2713}".to_string()
    } else {
        result.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_gradle_build_success() {
        let output = r#"> Task :compileJava UP-TO-DATE
> Task :processResources UP-TO-DATE
> Task :classes UP-TO-DATE
> Task :compileTestJava
> Task :processTestResources NO-SOURCE
> Task :testClasses
> Task :test
> Task :jar
Deprecated Gradle features were used in this build, making it incompatible with Gradle 9.0.
Use '--warning-mode all' to show the individual deprecation warnings.

BUILD SUCCESSFUL in 8s
7 actionable tasks: 4 executed, 3 up-to-date
"#;
        let result = filter_gradle(output);
        assert!(!result.contains("> Task"), "got: {}", result);
        assert!(!result.contains("Deprecated"), "got: {}", result);
        assert!(
            result.contains("BUILD SUCCESSFUL") || result.contains("executed"),
            "got: {}",
            result
        );
    }

    #[test]
    fn test_filter_gradle_savings() {
        let raw = r#"Downloading https://services.gradle.org/distributions/gradle-8.5-bin.zip
> Task :compileJava UP-TO-DATE
> Task :processResources UP-TO-DATE
> Task :classes UP-TO-DATE
> Task :compileTestJava
> Task :processTestResources NO-SOURCE
> Task :testClasses
> Task :test
> Task :check
> Task :jar
> Task :assemble
> Task :build
Deprecated Gradle features were used in this build, making it incompatible with Gradle 9.0.
Use '--warning-mode all' to show the individual deprecation warnings.

BUILD SUCCESSFUL in 15s
12 actionable tasks: 7 executed, 5 up-to-date
"#;
        let filtered = filter_gradle(raw);
        let savings = 100.0 - (count_tokens(&filtered) as f64 / count_tokens(raw) as f64 * 100.0);
        assert!(
            savings >= 60.0,
            "Expected >=60% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_gradle_failure() {
        let output = r#"> Task :compileJava
> Task :test FAILED

FAILURE: Build failed with an exception.
* What went wrong:
Execution failed for task ':test'.

BUILD FAILED in 5s
"#;
        let result = filter_gradle(output);
        assert!(result.contains("FAILURE"), "got: {}", result);
        assert!(result.contains("Execution failed"), "got: {}", result);
    }
}
