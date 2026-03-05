use crate::tracking;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::process::Command;

lazy_static! {
    /// Plan summary: "Plan: X to add, Y to change, Z to destroy."
    static ref RE_PLAN_SUMMARY: Regex =
        Regex::new(r"Plan:\s*(\d+)\s+to add,\s*(\d+)\s+to change,\s*(\d+)\s+to destroy").unwrap();
    /// Apply summary: "Apply complete! Resources: X added, Y changed, Z destroyed."
    static ref RE_APPLY_SUMMARY: Regex =
        Regex::new(r"Resources:\s*(\d+)\s+added,\s*(\d+)\s+changed,\s*(\d+)\s+destroyed").unwrap();
    /// Resource action lines: "# module.foo.resource will be created/destroyed/updated"
    static ref RE_RESOURCE_ACTION: Regex =
        Regex::new(r"^  #\s+(.+?)\s+will be\s+(.+)$").unwrap();
    /// Error lines
    static ref RE_TF_ERROR: Regex =
        Regex::new(r"(?i)^│?\s*Error:").unwrap();
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let mut cmd = Command::new("terraform");
    for arg in args {
        cmd.arg(arg);
    }

    if verbose > 0 {
        eprintln!("Running: terraform {}", args.join(" "));
    }

    let output = cmd.output().context("Failed to run terraform")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = filter_terraform(&raw);
    println!("{}", filtered);

    timer.track(
        &format!("terraform {}", args.join(" ")),
        &format!("rtk terraform {}", args.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}

/// Filter terraform output - show resource actions + summary, strip verbose diff
fn filter_terraform(output: &str) -> String {
    let mut resource_actions: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut summary_line: Option<String> = None;
    let mut in_error_block = false;

    for line in output.lines() {
        let trimmed = line.trim();

        // Capture errors
        if RE_TF_ERROR.is_match(trimmed) {
            in_error_block = true;
            let clean = trimmed.trim_start_matches('│').trim();
            errors.push(clean.to_string());
            continue;
        }

        if in_error_block {
            if trimmed.is_empty() || trimmed == "│" {
                in_error_block = false;
                continue;
            }
            let clean = trimmed.trim_start_matches('│').trim();
            if !clean.is_empty() {
                errors.push(format!("  {}", clean));
            }
            continue;
        }

        // Capture resource action lines
        if let Some(caps) = RE_RESOURCE_ACTION.captures(line) {
            let resource = caps.get(1).map(|m| m.as_str()).unwrap_or("?");
            let action = caps.get(2).map(|m| m.as_str()).unwrap_or("?");
            resource_actions.push(format!("  {} {}", action_symbol(action), resource));
            continue;
        }

        // Capture plan/apply summary
        if RE_PLAN_SUMMARY.is_match(trimmed) || RE_APPLY_SUMMARY.is_match(trimmed) {
            summary_line = Some(trimmed.to_string());
            continue;
        }

        // Capture "No changes" message
        if trimmed.contains("No changes") || trimmed.contains("Infrastructure is up-to-date") {
            summary_line = Some("No changes \u{2713}".to_string());
        }
    }

    let mut result = Vec::new();

    if !errors.is_empty() {
        for e in errors.iter().take(10) {
            result.push(e.clone());
        }
    }

    if !resource_actions.is_empty() {
        result.push(format!("{} resources:", resource_actions.len()));
        for action in resource_actions.iter().take(20) {
            result.push(action.clone());
        }
        if resource_actions.len() > 20 {
            result.push(format!("  ... +{} more", resource_actions.len() - 20));
        }
    }

    if let Some(summary) = summary_line {
        result.push(summary);
    }

    if result.is_empty() {
        "ok \u{2713}".to_string()
    } else {
        result.join("\n")
    }
}

fn action_symbol(action: &str) -> &str {
    if action.contains("created") {
        "+"
    } else if action.contains("destroyed") {
        "-"
    } else if action.contains("updated") || action.contains("changed") {
        "~"
    } else if action.contains("replaced") {
        "+/-"
    } else {
        "?"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_terraform_plan() {
        let output = r#"Terraform will perform the following actions:

  # aws_instance.web will be created
  + resource "aws_instance" "web" {
      + ami                          = "ami-12345678"
      + arn                          = (known after apply)
      + associate_public_ip_address  = (known after apply)
      + availability_zone            = (known after apply)
      + cpu_core_count               = (known after apply)
      + instance_type                = "t2.micro"
      + id                           = (known after apply)
    }

  # aws_security_group.allow_ssh will be created
  + resource "aws_security_group" "allow_ssh" {
      + arn                    = (known after apply)
      + description            = "Allow SSH"
      + id                     = (known after apply)
    }

  # aws_db_instance.old will be destroyed
  - resource "aws_db_instance" "old" {
      - engine = "mysql" -> null
    }

Plan: 2 to add, 0 to change, 1 to destroy.
"#;
        let result = filter_terraform(output);
        assert!(result.contains("+ aws_instance.web"), "got: {}", result);
        assert!(
            result.contains("+ aws_security_group.allow_ssh"),
            "got: {}",
            result
        );
        assert!(result.contains("- aws_db_instance.old"), "got: {}", result);
        assert!(result.contains("Plan: 2 to add"), "got: {}", result);
        // Should NOT contain the verbose resource attributes
        assert!(!result.contains("ami-12345678"), "got: {}", result);
        assert!(!result.contains("known after apply"), "got: {}", result);
    }

    #[test]
    fn test_filter_terraform_savings() {
        let raw = r#"Terraform will perform the following actions:

  # aws_instance.web will be created
  + resource "aws_instance" "web" {
      + ami                          = "ami-12345678"
      + arn                          = (known after apply)
      + associate_public_ip_address  = (known after apply)
      + availability_zone            = (known after apply)
      + cpu_core_count               = (known after apply)
      + instance_type                = "t2.micro"
      + id                           = (known after apply)
      + tags                         = { "Name" = "web" }
    }

  # aws_security_group.allow_ssh will be created
  + resource "aws_security_group" "allow_ssh" {
      + arn                    = (known after apply)
      + description            = "Allow SSH"
      + id                     = (known after apply)
      + name                   = "allow_ssh"
      + vpc_security_group_ids = (known after apply)
    }

Plan: 2 to add, 0 to change, 0 to destroy.
"#;
        let filtered = filter_terraform(raw);
        let savings = 100.0 - (count_tokens(&filtered) as f64 / count_tokens(raw) as f64 * 100.0);
        assert!(
            savings >= 60.0,
            "Expected >=60% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_terraform_no_changes() {
        let output = "No changes. Infrastructure is up-to-date.\n";
        let result = filter_terraform(output);
        assert!(result.contains("No changes"), "got: {}", result);
    }

    #[test]
    fn test_filter_terraform_errors() {
        let output = r#"│ Error: Missing required argument
│
│   on main.tf line 5, in resource "aws_instance" "web":
│    5: resource "aws_instance" "web" {
│
│ The argument "ami" is required, but no definition was found.
"#;
        let result = filter_terraform(output);
        assert!(result.contains("Error:"), "got: {}", result);
    }
}
