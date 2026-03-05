use crate::tracking;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::process::Command;

lazy_static! {
    /// Maven downloading lines
    static ref RE_MVN_DOWNLOAD: Regex =
        Regex::new(r"(?i)^(Downloading|Downloaded)\s+from\s+").unwrap();
    /// Maven progress indicator
    static ref RE_MVN_PROGRESS: Regex =
        Regex::new(r"^\s*\d+/\d+\s+(KB|MB|B)").unwrap();
    /// Maven build result line
    static ref RE_MVN_RESULT: Regex =
        Regex::new(r"^\[INFO\]\s+BUILD\s+(SUCCESS|FAILURE)").unwrap();
    /// Maven module line
    static ref RE_MVN_MODULE: Regex =
        Regex::new(r"^\[INFO\]\s+--- .+ ---$").unwrap();
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("mvn");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: mvn {}", args.join(" "));
    }

    let output = cmd.output().context("Failed to run mvn")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_mvn(&raw);
    println!("{}", filtered);

    timer.track(
        &format!("mvn {}", args.join(" ")),
        &format!("rtk mvn {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}

/// Filter Maven output - strip downloads/progress, keep errors + BUILD result
fn filter_mvn(output: &str) -> String {
    let mut errors: Vec<String> = Vec::new();
    let mut result_line: Option<String> = None;
    let mut total_time: Option<String> = None;
    let mut modules_built = 0usize;
    let mut in_error_block = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if in_error_block {
                in_error_block = false;
            }
            continue;
        }

        // Skip download lines
        if RE_MVN_DOWNLOAD.is_match(trimmed) {
            continue;
        }

        // Skip progress
        if RE_MVN_PROGRESS.is_match(trimmed) {
            continue;
        }

        // Skip [INFO] separator lines
        if trimmed
            == "[INFO] ------------------------------------------------------------------------"
            || trimmed.starts_with("[INFO] Scanning for projects")
            || trimmed.starts_with("[INFO] Reactor Build Order")
        {
            continue;
        }

        // Count modules
        if RE_MVN_MODULE.is_match(trimmed) {
            modules_built += 1;
            continue;
        }

        // Capture build result
        if RE_MVN_RESULT.is_match(trimmed) {
            result_line = Some(trimmed.replace("[INFO] ", ""));
            continue;
        }

        // Capture total time
        if trimmed.contains("Total time:") {
            total_time = Some(
                trimmed
                    .replace("[INFO] ", "")
                    .replace("[INFO]", "")
                    .trim()
                    .to_string(),
            );
            continue;
        }

        // Keep errors
        if trimmed.starts_with("[ERROR]") {
            in_error_block = true;
            errors.push(trimmed.replace("[ERROR] ", "").to_string());
            continue;
        }

        // Keep warning lines
        if trimmed.starts_with("[WARNING]") && !trimmed.contains("Using platform encoding") {
            errors.push(trimmed.replace("[WARNING] ", "WARN: ").to_string());
            continue;
        }

        // Continue error blocks
        if in_error_block && !trimmed.starts_with("[INFO]") {
            errors.push(format!("  {}", trimmed));
        }
    }

    let mut result = Vec::new();

    if !errors.is_empty() {
        for e in errors.iter().take(15) {
            result.push(e.clone());
        }
        if errors.len() > 15 {
            result.push(format!("... +{} more", errors.len() - 15));
        }
    }

    let mut summary = Vec::new();
    if let Some(res) = result_line {
        summary.push(res);
    }
    if modules_built > 0 {
        summary.push(format!("{} modules", modules_built));
    }
    if let Some(time) = total_time {
        summary.push(time);
    }

    if !summary.is_empty() {
        result.push(summary.join(" | "));
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
    fn test_filter_mvn_build_success() {
        let output = r#"[INFO] Scanning for projects...
[INFO] ------------------------------------------------------------------------
[INFO] --- maven-compiler-plugin:3.11.0:compile (default-compile) ---
[INFO] --- maven-resources-plugin:3.3.1:resources ---
Downloading from central: https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.pom
Downloaded from central: https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.pom
Downloading from central: https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.jar
Downloaded from central: https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.jar
[INFO] --- maven-jar-plugin:3.3.0:jar ---
[INFO] --- maven-install-plugin:3.1.1:install ---
[INFO] ------------------------------------------------------------------------
[INFO] BUILD SUCCESS
[INFO] ------------------------------------------------------------------------
[INFO] Total time:  12.345 s
[INFO] Finished at: 2026-03-05T10:00:00Z
[INFO] ------------------------------------------------------------------------
"#;
        let result = filter_mvn(output);
        assert!(!result.contains("Downloading"), "got: {}", result);
        assert!(!result.contains("Downloaded"), "got: {}", result);
        assert!(result.contains("BUILD SUCCESS"), "got: {}", result);
        assert!(result.contains("12.345"), "got: {}", result);
    }

    #[test]
    fn test_filter_mvn_savings() {
        let raw = r#"[INFO] Scanning for projects...
[INFO] Reactor Build Order:
[INFO] module-api
[INFO] module-core
[INFO] module-web
[INFO] ------------------------------------------------------------------------
[INFO] --- maven-compiler-plugin:3.11.0:compile ---
Downloading from central: https://repo.maven.apache.org/maven2/a/b/1.0/b-1.0.pom
Downloaded from central: https://repo.maven.apache.org/maven2/a/b/1.0/b-1.0.pom
Downloading from central: https://repo.maven.apache.org/maven2/c/d/2.0/d-2.0.pom
Downloaded from central: https://repo.maven.apache.org/maven2/c/d/2.0/d-2.0.pom
Downloading from central: https://repo.maven.apache.org/maven2/e/f/3.0/f-3.0.jar
Downloaded from central: https://repo.maven.apache.org/maven2/e/f/3.0/f-3.0.jar
[INFO] --- maven-resources-plugin:3.3.1:resources ---
[INFO] --- maven-jar-plugin:3.3.0:jar ---
[INFO] --- maven-install-plugin:3.1.1:install ---
[INFO] ------------------------------------------------------------------------
[INFO] BUILD SUCCESS
[INFO] ------------------------------------------------------------------------
[INFO] Total time:  45.678 s
"#;
        let filtered = filter_mvn(raw);
        let savings = 100.0 - (count_tokens(&filtered) as f64 / count_tokens(raw) as f64 * 100.0);
        assert!(
            savings >= 70.0,
            "Expected >=70% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_mvn_errors() {
        let output = r#"[INFO] --- maven-compiler-plugin:3.11.0:compile ---
[ERROR] /src/main/java/App.java:[10,1] error: ';' expected
[ERROR] /src/main/java/App.java:[15,5] error: cannot find symbol
[INFO] BUILD FAILURE
[INFO] Total time:  3.456 s
"#;
        let result = filter_mvn(output);
        assert!(result.contains("';' expected"), "got: {}", result);
        assert!(result.contains("BUILD FAILURE"), "got: {}", result);
    }
}
