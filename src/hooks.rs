use crate::config::Config;
use std::collections::HashMap;
use std::process::Command;

/// Expand `{var}` placeholders in a command string.
/// Known variables are replaced with their values. Empty values become "".
/// Unknown placeholders are left as-is for forward compatibility.
///
/// TODO(debt): Research shell injection risk from template substitution.
/// Filenames with shell metacharacters ($, `, ", ;, etc.) in template
/// variables could be exploited when expanded into an sh -c command string.
/// Future fix: switch to environment variables or add shell escaping.
pub fn expand_template(command: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = String::with_capacity(command.len());
    let mut i = 0;
    let chars: Vec<char> = command.chars().collect();

    while i < chars.len() {
        if chars[i] == '{' {
            // Check for adjacent braces {{ which we treat as literal
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                result.push('{');
                i += 1;
                continue;
            }

            // Try to parse {name} where name is [a-z_]+
            i += 1; // move past '{'
            let name_start = i;

            // Collect name characters
            while i < chars.len() && (chars[i].is_ascii_lowercase() || chars[i] == '_') {
                i += 1;
            }

            let name_end = i;

            // Check if we have a valid closing brace (and not adjacent }})
            if i < chars.len() && chars[i] == '}' && name_end > name_start {
                // Check for adjacent }} which makes the whole pattern invalid
                if i + 1 < chars.len() && chars[i + 1] == '}' {
                    // Invalid pattern - restore and output literally
                    result.push('{');
                    result.push_str(&chars[name_start..i].iter().collect::<String>());
                    result.push('}');
                    i += 1;
                } else {
                    // Valid placeholder: {name}
                    let name: String = chars[name_start..name_end].iter().collect();
                    if let Some(value) = vars.get(name.as_str()) {
                        result.push_str(value);
                    } else {
                        // Unknown var - leave as-is
                        result.push('{');
                        result.push_str(&name);
                        result.push('}');
                    }
                    i += 1; // move past '}'
                }
            } else {
                // Not a valid placeholder - output the '{' and continue from name_start
                result.push('{');
                i = name_start;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Run a post-rip hook if configured and appropriate.
pub fn run_post_rip(config: &Config, vars: &HashMap<&str, String>, no_hooks: bool) {
    if no_hooks {
        log::debug!("Skipping post-rip hook (no_hooks flag set)");
        return;
    }

    let hook = match &config.post_rip {
        Some(h) => h,
        None => return, // No hook configured
    };

    let command = match &hook.command {
        Some(c) => c,
        None => return, // No command specified
    };

    // Check status - skip on failure unless on_failure is true
    if let Some(status) = vars.get("status") {
        if status != "success" && !hook.on_failure() {
            log::debug!("Skipping post-rip hook (status={}, on_failure=false)", status);
            return;
        }
    }

    let expanded = expand_template(command, vars);
    execute_hook(&expanded, hook.blocking(), hook.log_output(), "post-rip");
}

/// Run a post-session hook if configured and appropriate.
pub fn run_post_session(config: &Config, vars: &HashMap<&str, String>, no_hooks: bool) {
    if no_hooks {
        log::debug!("Skipping post-session hook (no_hooks flag set)");
        return;
    }

    let hook = match &config.post_session {
        Some(h) => h,
        None => return, // No hook configured
    };

    let command = match &hook.command {
        Some(c) => c,
        None => return, // No command specified
    };

    // Check failed count - skip if any failed unless on_failure is true
    if let Some(failed) = vars.get("failed") {
        if let Ok(count) = failed.parse::<u32>() {
            if count > 0 && !hook.on_failure() {
                log::debug!(
                    "Skipping post-session hook (failed={}, on_failure=false)",
                    count
                );
                return;
            }
        }
    }

    let expanded = expand_template(command, vars);
    execute_hook(&expanded, hook.blocking(), hook.log_output(), "post-session");
}

/// Execute a hook command, either blocking or in a background thread.
fn execute_hook(command: &str, blocking: bool, log_output: bool, label: &str) {
    log::info!("Running {} hook: {}", label, command);

    if blocking {
        execute_blocking(command, log_output, label);
    } else {
        let cmd = command.to_string();
        let lbl = label.to_string();
        std::thread::spawn(move || {
            execute_blocking(&cmd, log_output, &lbl);
        });
    }
}

/// Execute a command using sh -c and log the results.
fn execute_blocking(command: &str, log_output: bool, label: &str) {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(output) => {
            if log_output {
                if !output.stdout.is_empty() {
                    if let Ok(stdout) = String::from_utf8(output.stdout) {
                        for line in stdout.lines() {
                            log::info!("{} stdout: {}", label, line);
                        }
                    }
                }
                if !output.stderr.is_empty() {
                    if let Ok(stderr) = String::from_utf8(output.stderr) {
                        for line in stderr.lines() {
                            log::warn!("{} stderr: {}", label, line);
                        }
                    }
                }
            }

            if !output.status.success() {
                log::warn!(
                    "{} hook exited with status: {}",
                    label,
                    output.status.code().unwrap_or(-1)
                );
            }
        }
        Err(e) => {
            log::warn!("Failed to execute {} hook: {}", label, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_vars_replaced() {
        let mut vars = HashMap::new();
        vars.insert("file", "output.mkv".to_string());
        vars.insert("status", "success".to_string());

        let result = expand_template("cp {file} /backup/ && echo {status}", &vars);
        assert_eq!(result, "cp output.mkv /backup/ && echo success");
    }

    #[test]
    fn test_unknown_vars_left_as_is() {
        let vars = HashMap::new();
        let result = expand_template("echo {unknown}", &vars);
        assert_eq!(result, "echo {unknown}");
    }

    #[test]
    fn test_empty_var_becomes_empty() {
        let mut vars = HashMap::new();
        vars.insert("empty", "".to_string());

        let result = expand_template("echo {empty}done", &vars);
        assert_eq!(result, "echo done");
    }

    #[test]
    fn test_no_placeholders_unchanged() {
        let vars = HashMap::new();
        let result = expand_template("echo hello world", &vars);
        assert_eq!(result, "echo hello world");
    }

    #[test]
    fn test_mixed_known_unknown() {
        let mut vars = HashMap::new();
        vars.insert("file", "test.mkv".to_string());

        let result = expand_template("{file} {unknown} {file}", &vars);
        assert_eq!(result, "test.mkv {unknown} test.mkv");
    }

    #[test]
    fn test_repeated_var() {
        let mut vars = HashMap::new();
        vars.insert("file", "test.mkv".to_string());

        let result = expand_template("cp {file} {file}.bak", &vars);
        assert_eq!(result, "cp test.mkv test.mkv.bak");
    }

    #[test]
    fn test_adjacent_braces() {
        let mut vars = HashMap::new();
        vars.insert("a", "1".to_string());
        vars.insert("b", "2".to_string());

        let result = expand_template("{a}{b}", &vars);
        assert_eq!(result, "12");
    }

    #[test]
    fn test_nested_braces_not_valid() {
        let mut vars = HashMap::new();
        vars.insert("file", "test.mkv".to_string());

        // Nested braces are not valid placeholders - left as-is
        let result = expand_template("{{file}}", &vars);
        assert_eq!(result, "{{file}}");
    }

    #[test]
    fn test_invalid_chars_in_placeholder() {
        let mut vars = HashMap::new();
        vars.insert("file", "test.mkv".to_string());

        // Uppercase and numbers are invalid - left as-is
        let result = expand_template("{File} {file1}", &vars);
        assert_eq!(result, "{File} {file1}");
    }

    #[test]
    fn test_unclosed_brace() {
        let vars = HashMap::new();
        let result = expand_template("echo {unclosed", &vars);
        assert_eq!(result, "echo {unclosed");
    }

    #[test]
    fn test_underscore_in_var_name() {
        let mut vars = HashMap::new();
        vars.insert("var_name", "value".to_string());

        let result = expand_template("{var_name}", &vars);
        assert_eq!(result, "value");
    }
}
