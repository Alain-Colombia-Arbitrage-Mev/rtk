use crate::tracking;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    version: String,
    #[serde(default)]
    latest_version: Option<String>,
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    // Auto-detect uv vs pip
    let use_uv = which_command("uv").is_some();
    let base_cmd = if use_uv { "uv" } else { "pip" };

    if verbose > 0 && use_uv {
        eprintln!("Using uv (pip-compatible)");
    }

    // Detect subcommand
    let subcommand = args.first().map(|s| s.as_str()).unwrap_or("");

    let (cmd_str, filtered) = match subcommand {
        "list" => run_list(base_cmd, &args[1..], verbose)?,
        "outdated" => run_outdated(base_cmd, &args[1..], verbose)?,
        "install" => run_pip_install(base_cmd, &args[1..], verbose)?,
        "uninstall" | "show" => {
            // Passthrough for other write operations
            run_passthrough(base_cmd, args, verbose)?
        }
        _ => {
            anyhow::bail!(
                "rtk pip: unsupported subcommand '{}'\nSupported: list, outdated, install, uninstall, show",
                subcommand
            );
        }
    };

    timer.track(
        &format!("{} {}", base_cmd, args.join(" ")),
        &format!("rtk {} {}", base_cmd, args.join(" ")),
        &cmd_str,
        &filtered,
    );

    Ok(())
}

fn run_list(base_cmd: &str, args: &[String], verbose: u8) -> Result<(String, String)> {
    let mut cmd = Command::new(base_cmd);

    if base_cmd == "uv" {
        cmd.arg("pip");
    }

    cmd.arg("list").arg("--format=json");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: {} pip list --format=json", base_cmd);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run {} pip list", base_cmd))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_pip_list(&stdout);
    println!("{}", filtered);

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok((raw, filtered))
}

fn run_outdated(base_cmd: &str, args: &[String], verbose: u8) -> Result<(String, String)> {
    let mut cmd = Command::new(base_cmd);

    if base_cmd == "uv" {
        cmd.arg("pip");
    }

    cmd.arg("list").arg("--outdated").arg("--format=json");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: {} pip list --outdated --format=json", base_cmd);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run {} pip list --outdated", base_cmd))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_pip_outdated(&stdout);
    println!("{}", filtered);

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok((raw, filtered))
}

fn run_passthrough(base_cmd: &str, args: &[String], verbose: u8) -> Result<(String, String)> {
    let mut cmd = Command::new(base_cmd);

    if base_cmd == "uv" {
        cmd.arg("pip");
    }

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: {} pip {}", base_cmd, args.join(" "));
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run {} pip {}", base_cmd, args.join(" ")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    print!("{}", stdout);
    eprint!("{}", stderr);

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok((raw.clone(), raw))
}

fn run_pip_install(base_cmd: &str, args: &[String], verbose: u8) -> Result<(String, String)> {
    let mut cmd = Command::new(base_cmd);

    if base_cmd == "uv" {
        cmd.arg("pip");
    }

    cmd.arg("install");

    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: {} pip install {}", base_cmd, args.join(" "));
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run {} pip install", base_cmd))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_pip_install(&raw);
    println!("{}", filtered);

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok((raw, filtered))
}

/// Filter pip install output - strip compilation noise, keep summary
fn filter_pip_install(output: &str) -> String {
    let mut result = Vec::new();
    let mut building_wheel = false;
    let mut current_wheel: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip gcc/clang compilation lines
        if trimmed.starts_with("gcc")
            || trimmed.starts_with("g++")
            || trimmed.starts_with("clang")
            || trimmed.starts_with("cc")
            || trimmed.starts_with("c++")
            || trimmed.starts_with("running")
            || trimmed.starts_with("creating")
            || trimmed.starts_with("copying")
            || trimmed.starts_with("Installing")
            || trimmed.starts_with("writing")
        {
            continue;
        }

        // Track wheel building
        if trimmed.contains("Building wheel for") {
            if let Some(pkg) = trimmed.split("Building wheel for").nth(1) {
                let name = pkg.split_whitespace().next().unwrap_or("");
                current_wheel = Some(name.to_string());
                building_wheel = true;
            }
            continue;
        }

        // End of wheel building
        if building_wheel
            && (trimmed.contains("Created wheel") || trimmed.contains("Successfully built"))
        {
            if let Some(name) = &current_wheel {
                result.push(format!("Built wheel: {}", name));
            }
            building_wheel = false;
            current_wheel = None;
            continue;
        }

        // Skip download progress
        if trimmed.contains("Downloading") || trimmed.contains("━") || trimmed.contains("eta") {
            continue;
        }

        // Skip "Collecting" verbose lines
        if trimmed.starts_with("Collecting") {
            continue;
        }

        // Skip "Using cached" lines
        if trimmed.starts_with("Using cached") {
            continue;
        }

        // Keep errors
        if trimmed.contains("ERROR") || trimmed.contains("error:") {
            result.push(trimmed.to_string());
            continue;
        }

        // Keep summary lines
        if trimmed.starts_with("Successfully installed")
            || trimmed.starts_with("Requirement already satisfied")
            || trimmed.contains("already satisfied")
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

/// Check if a command exists in PATH
fn which_command(cmd: &str) -> Option<String> {
    Command::new("which")
        .arg(cmd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Filter pip list JSON output
fn filter_pip_list(output: &str) -> String {
    let packages: Vec<Package> = match serde_json::from_str(output) {
        Ok(p) => p,
        Err(e) => {
            return format!("pip list (JSON parse failed: {})", e);
        }
    };

    if packages.is_empty() {
        return "pip list: No packages installed".to_string();
    }

    let mut result = String::new();
    result.push_str(&format!("pip list: {} packages\n", packages.len()));
    result.push_str("═══════════════════════════════════════\n");

    // Group by first letter for easier scanning
    let mut by_letter: std::collections::HashMap<char, Vec<&Package>> =
        std::collections::HashMap::new();

    for pkg in &packages {
        let first_char = pkg.name.chars().next().unwrap_or('?').to_ascii_lowercase();
        by_letter.entry(first_char).or_default().push(pkg);
    }

    let mut letters: Vec<_> = by_letter.keys().collect();
    letters.sort();

    for letter in letters {
        let pkgs = by_letter.get(letter).unwrap();
        result.push_str(&format!("\n[{}]\n", letter.to_uppercase()));

        for pkg in pkgs.iter().take(10) {
            result.push_str(&format!("  {} ({})\n", pkg.name, pkg.version));
        }

        if pkgs.len() > 10 {
            result.push_str(&format!("  ... +{} more\n", pkgs.len() - 10));
        }
    }

    result.trim().to_string()
}

/// Filter pip outdated JSON output
fn filter_pip_outdated(output: &str) -> String {
    let packages: Vec<Package> = match serde_json::from_str(output) {
        Ok(p) => p,
        Err(e) => {
            return format!("pip outdated (JSON parse failed: {})", e);
        }
    };

    if packages.is_empty() {
        return "✓ pip outdated: All packages up to date".to_string();
    }

    let mut result = String::new();
    result.push_str(&format!("pip outdated: {} packages\n", packages.len()));
    result.push_str("═══════════════════════════════════════\n");

    for (i, pkg) in packages.iter().take(20).enumerate() {
        let latest = pkg.latest_version.as_deref().unwrap_or("unknown");
        result.push_str(&format!(
            "{}. {} ({} → {})\n",
            i + 1,
            pkg.name,
            pkg.version,
            latest
        ));
    }

    if packages.len() > 20 {
        result.push_str(&format!("\n... +{} more packages\n", packages.len() - 20));
    }

    result.push_str("\n💡 Run `pip install --upgrade <package>` to update\n");

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_pip_list() {
        let output = r#"[
  {"name": "requests", "version": "2.31.0"},
  {"name": "pytest", "version": "7.4.0"},
  {"name": "rich", "version": "13.0.0"}
]"#;

        let result = filter_pip_list(output);
        assert!(result.contains("3 packages"));
        assert!(result.contains("requests"));
        assert!(result.contains("2.31.0"));
        assert!(result.contains("pytest"));
    }

    #[test]
    fn test_filter_pip_list_empty() {
        let output = "[]";
        let result = filter_pip_list(output);
        assert!(result.contains("No packages installed"));
    }

    #[test]
    fn test_filter_pip_outdated_none() {
        let output = "[]";
        let result = filter_pip_outdated(output);
        assert!(result.contains("✓"));
        assert!(result.contains("All packages up to date"));
    }

    #[test]
    fn test_filter_pip_outdated_some() {
        let output = r#"[
  {"name": "requests", "version": "2.31.0", "latest_version": "2.32.0"},
  {"name": "pytest", "version": "7.4.0", "latest_version": "8.0.0"}
]"#;

        let result = filter_pip_outdated(output);
        assert!(result.contains("2 packages"));
        assert!(result.contains("requests"));
        assert!(result.contains("2.31.0 → 2.32.0"));
        assert!(result.contains("pytest"));
        assert!(result.contains("7.4.0 → 8.0.0"));
    }

    #[test]
    fn test_filter_pip_install_with_compilation() {
        let output = r#"Collecting numpy==1.26.0
  Using cached numpy-1.26.0.tar.gz (15.8 MB)
  Building wheel for numpy (setup.py) ...
running build
running build_ext
gcc -O2 -fPIC -c numpy/core/src/multiarray/array_assign_scalar.c -o build/numpy/core/src/multiarray/array_assign_scalar.o
gcc -O2 -fPIC -c numpy/core/src/multiarray/arrayobject.c -o build/numpy/core/src/multiarray/arrayobject.o
gcc -O2 -fPIC -c numpy/core/src/multiarray/buffer.c -o build/numpy/core/src/multiarray/buffer.o
gcc -O2 -fPIC -c numpy/core/src/multiarray/casting.c -o build/numpy/core/src/multiarray/casting.o
creating build/lib
copying numpy/__init__.py -> build/lib
writing numpy/core/setup.cfg
  Created wheel for numpy
Successfully installed numpy-1.26.0
"#;
        let result = filter_pip_install(output);
        assert!(!result.contains("gcc"), "got: {}", result);
        assert!(!result.contains("running"), "got: {}", result);
        assert!(!result.contains("Collecting"), "got: {}", result);
        assert!(result.contains("Built wheel: numpy"), "got: {}", result);
        assert!(result.contains("Successfully installed"), "got: {}", result);
    }

    #[test]
    fn test_filter_pip_install_savings() {
        let raw = r#"Collecting cryptography==41.0.0
  Downloading cryptography-41.0.0.tar.gz (23.4 MB)
     ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ 23.4/23.4 MB 15.2 MB/s eta 0:00:00
  Building wheel for cryptography (setup.py) ...
running build
running build_ext
gcc -O2 -fPIC -c src/_cffi_backend.c -o build/src/_cffi_backend.o
gcc -O2 -fPIC -c src/openssl/bio.c -o build/src/openssl/bio.o
gcc -O2 -fPIC -c src/openssl/ssl.c -o build/src/openssl/ssl.o
gcc -O2 -fPIC -c src/openssl/x509.c -o build/src/openssl/x509.o
creating build/lib
copying src/__init__.py -> build/lib
writing setup.cfg
  Created wheel for cryptography
Installing collected packages: cryptography
Successfully installed cryptography-41.0.0
"#;
        let filtered = filter_pip_install(raw);
        let input_t = raw.split_whitespace().count();
        let output_t = filtered.split_whitespace().count();
        let savings = 100.0 - (output_t as f64 / input_t as f64 * 100.0);
        assert!(
            savings >= 70.0,
            "Expected >=70% savings, got {:.1}%",
            savings
        );
    }
}
