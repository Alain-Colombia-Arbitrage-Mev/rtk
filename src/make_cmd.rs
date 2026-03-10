use crate::tracking;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::process::Command;

lazy_static! {
    /// make entering/leaving directory messages
    static ref RE_MAKE_DIR: Regex =
        Regex::new(r"make\[\d+\]:\s+(Entering|Leaving)\s+directory").unwrap();
    /// Compiler invocation lines (gcc, g++, clang, cc, c++)
    static ref RE_COMPILER: Regex =
        Regex::new(r"^\s*(gcc|g\+\+|clang|clang\+\+|cc|c\+\+|ar|ranlib|ld)\s+").unwrap();
    /// CMake progress lines
    static ref RE_CMAKE_PROGRESS: Regex =
        Regex::new(r"^\[\s*\d+%\]").unwrap();
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("make");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: make {}", args.join(" "));
    }

    let output = cmd.output().context("Failed to run make")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_make(&raw);
    println!("{}", filtered);

    timer.track(
        &format!("make {}", args.join(" ")),
        &format!("rtk make {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}

/// Filter make/cmake output - show errors + summary, strip compilation commands
fn filter_make(output: &str) -> String {
    let mut errors: Vec<String> = Vec::new();
    let mut compiled = 0usize;
    let mut linked = 0usize;
    let mut targets_built: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip directory enter/leave
        if RE_MAKE_DIR.is_match(trimmed) {
            continue;
        }

        // Count compiler invocations but skip the line
        if RE_COMPILER.is_match(trimmed) {
            // -c flag means compile (even with -o for output file name)
            // No -c means linking
            if trimmed.contains(" -c ") {
                compiled += 1;
            } else if trimmed.contains(" -o ") || trimmed.contains("-shared") {
                linked += 1;
            } else {
                compiled += 1;
            }
            continue;
        }

        // Skip cmake progress but count
        if RE_CMAKE_PROGRESS.is_match(trimmed) {
            if trimmed.contains("Building") {
                compiled += 1;
            }
            if trimmed.contains("Linking") {
                linked += 1;
                // Extract target name
                if let Some(target) = trimmed.split("Linking").nth(1) {
                    let name = target.split_whitespace().last().unwrap_or("");
                    if !name.is_empty() {
                        targets_built.push(name.to_string());
                    }
                }
            }
            continue;
        }

        // Keep errors and warnings
        if trimmed.contains("error:") || trimmed.contains("Error:") || trimmed.contains("*** ") {
            errors.push(trimmed.to_string());
            continue;
        }

        if trimmed.contains("warning:") {
            errors.push(trimmed.to_string());
            continue;
        }

        // Keep "Built target" lines from cmake
        if trimmed.contains("Built target") {
            if let Some(name) = trimmed.split("Built target").nth(1) {
                targets_built.push(name.trim().to_string());
            }
            continue;
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

    // Summary
    let mut summary_parts = Vec::new();
    if compiled > 0 {
        summary_parts.push(format!("{} compiled", compiled));
    }
    if linked > 0 {
        summary_parts.push(format!("{} linked", linked));
    }
    if !targets_built.is_empty() {
        let shown: Vec<_> = targets_built.iter().take(5).cloned().collect();
        summary_parts.push(format!("targets: {}", shown.join(", ")));
    }

    if summary_parts.is_empty() && errors.is_empty() {
        result.push("ok \u{2713}".to_string());
    } else if !summary_parts.is_empty() {
        result.push(summary_parts.join(", "));
    }

    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_make_gcc() {
        let output = r#"make[1]: Entering directory '/home/user/project'
gcc -c -o src/main.o src/main.c -Wall -O2
gcc -c -o src/utils.o src/utils.c -Wall -O2
gcc -c -o src/parser.o src/parser.c -Wall -O2
gcc -o myapp src/main.o src/utils.o src/parser.o -lm
make[1]: Leaving directory '/home/user/project'
"#;
        let result = filter_make(output);
        assert!(!result.contains("gcc"), "got: {}", result);
        assert!(!result.contains("Entering"), "got: {}", result);
        assert!(result.contains("compiled"), "got: {}", result);
        assert!(result.contains("linked"), "got: {}", result);
    }

    #[test]
    fn test_filter_make_errors() {
        let output = r#"gcc -c -o src/main.o src/main.c
src/main.c:10:5: error: expected ';' before '}' token
make: *** [Makefile:5: src/main.o] Error 1
"#;
        let result = filter_make(output);
        assert!(result.contains("error:"), "got: {}", result);
        assert!(result.contains("***"), "got: {}", result);
    }

    #[test]
    fn test_filter_make_cmake() {
        let output = r#"[ 14%] Building CXX object src/CMakeFiles/mylib.dir/foo.cpp.o
[ 28%] Building CXX object src/CMakeFiles/mylib.dir/bar.cpp.o
[ 42%] Building CXX object src/CMakeFiles/mylib.dir/baz.cpp.o
[ 57%] Linking CXX shared library libmylib.so
[ 57%] Built target mylib
[ 71%] Building CXX object app/CMakeFiles/myapp.dir/main.cpp.o
[ 85%] Linking CXX executable myapp
[100%] Built target myapp
"#;
        let result = filter_make(output);
        assert!(!result.contains("["), "got: {}", result);
        assert!(result.contains("compiled"), "got: {}", result);
        assert!(result.contains("myapp"), "got: {}", result);
    }

    #[test]
    fn test_filter_make_savings() {
        let raw = r#"make[1]: Entering directory '/home/user/project/src'
gcc -I/usr/include -I../include -DHAVE_CONFIG_H -c -o main.o main.c -Wall -Wextra -O2 -g
gcc -I/usr/include -I../include -DHAVE_CONFIG_H -c -o utils.o utils.c -Wall -Wextra -O2 -g
gcc -I/usr/include -I../include -DHAVE_CONFIG_H -c -o parser.o parser.c -Wall -Wextra -O2 -g
gcc -I/usr/include -I../include -DHAVE_CONFIG_H -c -o lexer.o lexer.c -Wall -Wextra -O2 -g
gcc -I/usr/include -I../include -DHAVE_CONFIG_H -c -o codegen.o codegen.c -Wall -Wextra -O2 -g
ar rcs libcompiler.a main.o utils.o parser.o lexer.o codegen.o
gcc -o compiler main.o utils.o parser.o lexer.o codegen.o -L../lib -lcompiler -lm
make[1]: Leaving directory '/home/user/project/src'
"#;
        let filtered = filter_make(raw);
        let savings = 100.0 - (count_tokens(&filtered) as f64 / count_tokens(raw) as f64 * 100.0);
        assert!(
            savings >= 70.0,
            "Expected >=70% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_make_empty() {
        let result = filter_make("");
        assert_eq!(result, "ok \u{2713}");
    }
}
