use crate::tracking;
use crate::utils::strip_ansi;
use anyhow::{Context, Result};
use lazy_static::lazy_static;
use regex::Regex;
use std::process::Command;

lazy_static! {
    /// Nitro preset line: "Preset: node-server"
    static ref RE_PRESET: Regex = Regex::new(r"(?i)preset:\s+(\S+)").unwrap();
    /// Build time: "Nitro built in 1.23s" or "ℹ Vite client built in 850ms"
    static ref RE_BUILD_TIME: Regex = Regex::new(r"built?\s+in\s+(\d+(?:\.\d+)?)\s*(s|ms)").unwrap();
    /// Route entry: "├── /api/health" or "│   ├── /dashboard"
    static ref RE_ROUTE: Regex = Regex::new(r"[├└│─\s]+(/\S*)").unwrap();
    /// Vite chunk: "dist/client/_nuxt/index-abc123.js  12.34 kB │ gzip: 4.56 kB"
    static ref RE_CHUNK: Regex = Regex::new(r"(\S+\.(?:js|css|mjs))\s+(\d+(?:\.\d+)?)\s*(kB|B)").unwrap();
    /// Nuxt module lines: "ℹ Using <module>"
    static ref RE_MODULE: Regex = Regex::new(r"(?i)using\s+(\S+)").unwrap();
    /// Progress/spinner lines
    static ref RE_PROGRESS: Regex = Regex::new(r"(?i)(building|bundling|transforming|compiling|optimizing|rendering|generating)\.*\s*$").unwrap();
}

pub fn run(args: &[String], verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    let subcommand = args.first().map(|s| s.as_str()).unwrap_or("build");
    let rest = if args.is_empty() { &[] } else { &args[1..] };

    // Try nuxt/nuxi directly, fallback to npx
    let nuxt_exists = Command::new("which")
        .arg("nuxt")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let nuxi_exists = !nuxt_exists
        && Command::new("which")
            .arg("nuxi")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

    let mut cmd = if nuxt_exists {
        Command::new("nuxt")
    } else if nuxi_exists {
        Command::new("nuxi")
    } else {
        let mut c = Command::new("npx");
        c.arg("nuxi");
        c
    };

    cmd.arg(subcommand);
    for arg in rest {
        cmd.arg(arg);
    }

    if verbose > 0 {
        let tool = if nuxt_exists {
            "nuxt"
        } else if nuxi_exists {
            "nuxi"
        } else {
            "npx nuxi"
        };
        eprintln!("Running: {} {} {}", tool, subcommand, rest.join(" "));
    }

    let output = cmd
        .output()
        .context("Failed to run nuxt (try: npm install -g nuxi)")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = format!("{}\n{}", stdout, stderr);

    let filtered = match subcommand {
        "build" => filter_nuxt_build(&raw),
        "generate" => filter_nuxt_generate(&raw),
        "dev" => filter_nuxt_dev(&raw),
        _ => filter_nuxt_generic(&raw),
    };

    println!("{}", filtered);

    timer.track(
        &format!("nuxt {} {}", subcommand, rest.join(" ")),
        &format!("rtk nuxt {} {}", subcommand, rest.join(" ")),
        &raw,
        &filtered,
    );

    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }

    Ok(())
}

/// Filter `nuxt build` output — strip Vite transform noise, keep routes + chunks + errors
fn filter_nuxt_build(output: &str) -> String {
    let clean = strip_ansi(output);

    let mut preset = String::new();
    let mut build_times: Vec<String> = Vec::new();
    let mut routes = 0usize;
    let mut chunks: Vec<(String, f64)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut warnings = 0usize;
    let mut nitro_output_dir: Option<String> = None;

    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip progress/spinner lines
        if RE_PROGRESS.is_match(trimmed) {
            continue;
        }

        // Skip verbose Vite transform lines
        if trimmed.contains("transforming")
            || trimmed.contains("modules transformed")
            || trimmed.starts_with("✓")
                && (trimmed.contains("modules") || trimmed.contains("transform"))
        {
            continue;
        }

        // Skip "ℹ Using ..." module announcement lines
        if trimmed.contains("Using ") && RE_MODULE.is_match(trimmed) {
            continue;
        }

        // Capture preset
        if let Some(caps) = RE_PRESET.captures(trimmed) {
            preset = caps[1].to_string();
            continue;
        }

        // Capture build times
        if let Some(caps) = RE_BUILD_TIME.captures(trimmed) {
            let time = format!("{}{}", &caps[1], &caps[2]);
            if trimmed.contains("client") || trimmed.contains("Vite") {
                build_times.push(format!("client: {}", time));
            } else if trimmed.contains("server") || trimmed.contains("Nitro") {
                build_times.push(format!("server: {}", time));
            } else {
                build_times.push(time);
            }
            continue;
        }

        // Count routes
        if RE_ROUTE.is_match(trimmed) && trimmed.contains('/') {
            routes += 1;
            continue;
        }

        // Capture chunk sizes
        if let Some(caps) = RE_CHUNK.captures(trimmed) {
            let name = caps[1].to_string();
            let mut size: f64 = caps[2].parse().unwrap_or(0.0);
            if &caps[3] == "B" {
                size /= 1024.0;
            }
            chunks.push((name, size));
            continue;
        }

        // Capture output directory
        if trimmed.contains("Output directory:") || trimmed.contains(".output") {
            if let Some(dir) = trimmed.split("Output directory:").nth(1) {
                nitro_output_dir = Some(dir.trim().to_string());
            } else if trimmed.contains(".output") {
                nitro_output_dir = Some(".output".to_string());
            }
            continue;
        }

        // Count warnings
        if trimmed.contains("warning") || trimmed.contains("WARN") {
            warnings += 1;
            continue;
        }

        // Keep errors
        if trimmed.contains("ERROR")
            || trimmed.contains("error:")
            || trimmed.contains("Error:")
            || trimmed.starts_with("✗")
            || trimmed.starts_with("✖")
        {
            errors.push(trimmed.to_string());
        }
    }

    let mut result = Vec::new();
    result.push("Nuxt Build".to_string());
    result.push("═══════════════════════════════════════".to_string());

    if !errors.is_empty() {
        for e in errors.iter().take(10) {
            result.push(e.clone());
        }
        if errors.len() > 10 {
            result.push(format!("... +{} more errors", errors.len() - 10));
        }
    }

    // Summary line
    let mut summary_parts = Vec::new();

    if !preset.is_empty() {
        summary_parts.push(format!("preset: {}", preset));
    }

    if routes > 0 {
        summary_parts.push(format!("{} routes", routes));
    }

    if !chunks.is_empty() {
        let total_kb: f64 = chunks.iter().map(|(_, s)| s).sum();
        summary_parts.push(format!("{} chunks ({:.0} kB)", chunks.len(), total_kb));
    }

    if warnings > 0 {
        summary_parts.push(format!("{} warnings", warnings));
    }

    if !summary_parts.is_empty() {
        result.push(summary_parts.join(" | "));
    }

    // Top chunks by size
    if !chunks.is_empty() {
        chunks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        result.push(String::new());
        result.push("Top chunks:".to_string());
        for (name, size) in chunks.iter().take(8) {
            // Shorten chunk name
            let short = if name.len() > 40 {
                format!("...{}", &name[name.len() - 37..])
            } else {
                name.clone()
            };
            result.push(format!("  {:<40} {:>6.1} kB", short, size));
        }
        if chunks.len() > 8 {
            result.push(format!("  ... +{} more", chunks.len() - 8));
        }
    }

    // Build times
    if !build_times.is_empty() {
        result.push(format!("\nTime: {}", build_times.join(", ")));
    }

    if let Some(dir) = nitro_output_dir {
        result.push(format!("Output: {}", dir));
    }

    if errors.is_empty() && result.len() <= 2 {
        // No meaningful output captured
        result.push("ok \u{2713}".to_string());
    }

    result.join("\n")
}

/// Filter `nuxt generate` output — strip per-page rendering noise, keep summary
fn filter_nuxt_generate(output: &str) -> String {
    let clean = strip_ansi(output);

    let mut pages_generated = 0usize;
    let mut has_bulk_count = false;
    let mut errors: Vec<String> = Vec::new();
    let mut build_time = String::new();

    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Bulk "Prerendered X routes in Ys" line (check first, takes priority)
        if trimmed.contains("prerendered") || trimmed.contains("Prerendered") {
            if let Some(caps) = Regex::new(r"(\d+)\s+route")
                .ok()
                .and_then(|r| r.captures(trimmed))
            {
                pages_generated = caps[1].parse().unwrap_or(pages_generated);
                has_bulk_count = true;
            }
            // Also extract time from "in X.Ys" portion
            if let Some(caps) = Regex::new(r"in\s+(\d+(?:\.\d+)?)\s*(s|ms)")
                .ok()
                .and_then(|r| r.captures(trimmed))
            {
                build_time = format!("{}{}", &caps[1], &caps[2]);
            }
            continue;
        }

        // Count individual generated pages (only if no bulk count)
        if !has_bulk_count && trimmed.contains("Generated") && trimmed.contains('/') {
            pages_generated += 1;
            continue;
        }

        // Capture build time
        if let Some(caps) = RE_BUILD_TIME.captures(trimmed) {
            build_time = format!("{}{}", &caps[1], &caps[2]);
            continue;
        }

        // Keep errors
        if trimmed.contains("ERROR") || trimmed.contains("error:") {
            errors.push(trimmed.to_string());
        }
    }

    let mut result = Vec::new();
    result.push("Nuxt Generate".to_string());
    result.push("═══════════════════════════════════════".to_string());

    if !errors.is_empty() {
        for e in errors.iter().take(10) {
            result.push(e.clone());
        }
    }

    if pages_generated > 0 {
        result.push(format!("{} pages generated \u{2713}", pages_generated));
    }

    if !build_time.is_empty() {
        result.push(format!("Time: {}", build_time));
    }

    if result.len() <= 2 {
        result.push("ok \u{2713}".to_string());
    }

    result.join("\n")
}

/// Filter `nuxt dev` output — strip HMR/Vite noise, keep server URL + errors
fn filter_nuxt_dev(output: &str) -> String {
    let clean = strip_ansi(output);

    let mut server_url: Option<String> = None;
    let mut errors: Vec<String> = Vec::new();
    let mut ready = false;

    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip HMR/hot reload noise
        if trimmed.contains("[HMR]")
            || trimmed.contains("hmr update")
            || trimmed.contains("[vite]")
            || trimmed.contains("page reload")
            || trimmed.contains("hot updated")
        {
            continue;
        }

        // Skip Vite transform progress
        if trimmed.contains("modules transformed")
            || trimmed.contains("transforming")
            || RE_PROGRESS.is_match(trimmed)
        {
            continue;
        }

        // Capture server URL
        if trimmed.contains("Local:") || trimmed.contains("localhost") || trimmed.contains("://") {
            if let Some(url) = extract_url(trimmed) {
                server_url = Some(url);
                continue;
            }
        }

        // Detect ready state
        if trimmed.contains("ready") || trimmed.contains("Ready") || trimmed.contains("Listening") {
            ready = true;
            // Also try to extract URL from this line
            if server_url.is_none() {
                if let Some(url) = extract_url(trimmed) {
                    server_url = Some(url);
                }
            }
            continue;
        }

        // Keep errors
        if trimmed.contains("ERROR") || trimmed.contains("error:") || trimmed.starts_with("✗") {
            errors.push(trimmed.to_string());
        }
    }

    let mut result = Vec::new();

    if !errors.is_empty() {
        for e in errors.iter().take(10) {
            result.push(e.clone());
        }
    }

    if let Some(url) = server_url {
        result.push(format!("Nuxt dev: {} \u{2713}", url));
    } else if ready {
        result.push("Nuxt dev: ready \u{2713}".to_string());
    }

    if result.is_empty() {
        "ok \u{2713}".to_string()
    } else {
        result.join("\n")
    }
}

/// Filter generic nuxt subcommands — strip verbose info, keep errors + summary
fn filter_nuxt_generic(output: &str) -> String {
    let clean = strip_ansi(output);
    let mut result = Vec::new();

    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip info/debug lines
        if trimmed.starts_with("ℹ")
            || trimmed.starts_with("[info]")
            || trimmed.starts_with("[debug]")
        {
            continue;
        }

        // Skip progress
        if RE_PROGRESS.is_match(trimmed) {
            continue;
        }

        // Keep errors, warnings, and summary lines
        if trimmed.contains("ERROR")
            || trimmed.contains("error:")
            || trimmed.contains("warning:")
            || trimmed.contains("WARN")
            || trimmed.contains("✓")
            || trimmed.contains("✗")
            || trimmed.contains("Done")
            || trimmed.contains("Success")
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

/// Extract URL from a line
fn extract_url(line: &str) -> Option<String> {
    lazy_static! {
        static ref RE_URL: Regex = Regex::new(r"https?://\S+").unwrap();
    }
    RE_URL.find(line).map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_tokens(text: &str) -> usize {
        text.split_whitespace().count()
    }

    #[test]
    fn test_filter_nuxt_build() {
        let output = r#"ℹ Using @nuxt/devtools
ℹ Using @nuxtjs/tailwindcss
ℹ Using @pinia/nuxt
Preset: node-server
✓ 23 modules transformed
✓ 45 modules transformed
✓ Client built in 2.5s
✓ Server built in 1.2s

dist/client/_nuxt/index-abc123.js  45.2 kB
dist/client/_nuxt/vendor-def456.js  128.7 kB
dist/client/_nuxt/app-ghi789.css  12.3 kB

Output directory: .output/server
"#;
        let result = filter_nuxt_build(output);
        assert!(result.contains("Nuxt Build"), "got: {}", result);
        assert!(result.contains("node-server"), "got: {}", result);
        assert!(!result.contains("modules transformed"), "got: {}", result);
        assert!(!result.contains("Using @"), "got: {}", result);
        assert!(result.contains("chunks"), "got: {}", result);
    }

    #[test]
    fn test_filter_nuxt_build_savings() {
        let raw = r#"ℹ Using @nuxt/devtools v1.0.0
ℹ Using @nuxtjs/tailwindcss v6.8.0
ℹ Using @pinia/nuxt v0.5.0
ℹ Using nuxt-icon v0.4.0
Preset: node-server
Building client...
✓ 156 modules transformed.
✓ 89 modules transformed.
✓ 234 modules transformed.
transforming (45) pages/index.vue
transforming (46) pages/about.vue
transforming (47) components/Header.vue
transforming (48) components/Footer.vue
✓ Client built in 4.5s
Building server...
✓ 78 modules transformed.
✓ Server built in 2.1s
dist/client/_nuxt/index-abc123.js  45.2 kB │ gzip: 15.6 kB
dist/client/_nuxt/vendor-def456.js  128.7 kB │ gzip: 42.3 kB
dist/client/_nuxt/app-ghi789.js  23.4 kB │ gzip: 8.1 kB
dist/client/_nuxt/pages-index-xyz.js  5.6 kB │ gzip: 2.1 kB
dist/client/_nuxt/entry.css  12.3 kB │ gzip: 3.4 kB
Output directory: .output
"#;
        let filtered = filter_nuxt_build(raw);
        let input_t = count_tokens(raw);
        let output_t = count_tokens(&filtered);
        let savings = 100.0 - (output_t as f64 / input_t as f64 * 100.0);
        assert!(
            savings >= 50.0,
            "Expected >=50% savings, got {:.1}%",
            savings
        );
    }

    #[test]
    fn test_filter_nuxt_build_with_errors() {
        let output = r#"Preset: node-server
ERROR: Cannot find module '@/components/Missing'
error: Build failed with 1 error
"#;
        let result = filter_nuxt_build(output);
        assert!(result.contains("Cannot find module"), "got: {}", result);
        assert!(result.contains("Build failed"), "got: {}", result);
    }

    #[test]
    fn test_filter_nuxt_generate() {
        let output = r#"ℹ Using @nuxtjs/tailwindcss
Building...
✓ Client built in 3.2s
Prerendered 25 routes in 4.5s
Generated /about
Generated /contact
Generated /blog/post-1
Generated /blog/post-2
"#;
        let result = filter_nuxt_generate(output);
        assert!(result.contains("Nuxt Generate"), "got: {}", result);
        assert!(result.contains("25 pages generated"), "got: {}", result);
        assert!(result.contains("4.5s"), "got: {}", result);
    }

    #[test]
    fn test_filter_nuxt_dev() {
        let output = r#"Nuxt 3.10.0 with Nitro 2.9.0
ℹ Using @nuxtjs/tailwindcss
ℹ Vite client warmed up in 1234ms
✓ 45 modules transformed
[HMR] connected
[vite] hot updated: /pages/index.vue
[HMR] updated /components/Header.vue
Listening on http://localhost:3000
✓ Nuxt ready in 2543ms
"#;
        let result = filter_nuxt_dev(output);
        assert!(result.contains("http://localhost:3000"), "got: {}", result);
        assert!(!result.contains("[HMR]"), "got: {}", result);
        assert!(!result.contains("[vite]"), "got: {}", result);
        assert!(!result.contains("modules transformed"), "got: {}", result);
    }

    #[test]
    fn test_filter_nuxt_generic() {
        let output = r#"ℹ Loading nuxt config
ℹ Using @nuxtjs/tailwindcss
[info] Starting cleanup
[debug] Checking cache
✓ Done cleaning build artifacts
"#;
        let result = filter_nuxt_generic(output);
        assert!(result.contains("Done"), "got: {}", result);
        assert!(!result.contains("[info]"), "got: {}", result);
        assert!(!result.contains("[debug]"), "got: {}", result);
    }
}
