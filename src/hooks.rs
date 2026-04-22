use crate::config::Config;
use std::collections::HashMap;
use std::process::Command;

/// Shell-escape a value by wrapping in single quotes (POSIX).
/// Internal single quotes are replaced with `'\''` (end quote, literal quote, start quote).
fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            escaped.push_str("'\\''");
        } else {
            escaped.push(ch);
        }
    }
    escaped.push('\'');
    escaped
}

/// Expand `{var}` placeholders in a command string.
/// Known variables are replaced with shell-escaped values.
/// Unknown placeholders are left as-is for forward compatibility.
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
                        result.push_str(&shell_escape(value));
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

/// Decide whether a post-rip hook should fire and return the expanded command.
/// Returns None if the hook should be skipped, Some(expanded_command) if it should run.
fn prepare_post_rip(
    config: &Config,
    vars: &HashMap<&str, String>,
    no_hooks: bool,
) -> Option<String> {
    if no_hooks {
        log::debug!("Skipping post-rip hook (no_hooks flag set)");
        return None;
    }

    let hook = config.post_rip.as_ref()?;
    let command = hook.command.as_deref().filter(|c| !c.is_empty())?;

    // Check status - skip on failure unless on_failure is true
    if let Some(status) = vars.get("status") {
        if status != "success" && !hook.on_failure() {
            log::debug!(
                "Skipping post-rip hook (status={}, on_failure=false)",
                status
            );
            return None;
        }
    }

    Some(expand_template(command, vars))
}

/// Run a post-rip hook if configured and appropriate.
pub fn run_post_rip(config: &Config, vars: &HashMap<&str, String>, no_hooks: bool) {
    if let Some(expanded) = prepare_post_rip(config, vars, no_hooks) {
        let hook = config.post_rip.as_ref().unwrap();
        execute_hook(&expanded, hook.blocking(), hook.log_output(), "post-rip");
    }
}

/// Decide whether a post-session hook should fire and return the expanded command.
/// Returns None if the hook should be skipped, Some(expanded_command) if it should run.
fn prepare_post_session(
    config: &Config,
    vars: &HashMap<&str, String>,
    no_hooks: bool,
) -> Option<String> {
    if no_hooks {
        log::debug!("Skipping post-session hook (no_hooks flag set)");
        return None;
    }

    let hook = config.post_session.as_ref()?;
    let command = hook.command.as_deref().filter(|c| !c.is_empty())?;

    // Check failed count - skip if any failed unless on_failure is true
    if let Some(failed) = vars.get("failed") {
        if let Ok(count) = failed.parse::<u32>() {
            if count > 0 && !hook.on_failure() {
                log::debug!(
                    "Skipping post-session hook (failed={}, on_failure=false)",
                    count
                );
                return None;
            }
        }
    }

    Some(expand_template(command, vars))
}

/// Run a post-session hook if configured and appropriate.
pub fn run_post_session(config: &Config, vars: &HashMap<&str, String>, no_hooks: bool) {
    if let Some(expanded) = prepare_post_session(config, vars, no_hooks) {
        let hook = config.post_session.as_ref().unwrap();
        execute_hook(
            &expanded,
            hook.blocking(),
            hook.log_output(),
            "post-session",
        );
    }
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
        assert_eq!(result, "cp 'output.mkv' /backup/ && echo 'success'");
    }

    #[test]
    fn test_unknown_vars_left_as_is() {
        let vars = HashMap::new();
        let result = expand_template("echo {unknown}", &vars);
        assert_eq!(result, "echo {unknown}");
    }

    #[test]
    fn test_empty_var_becomes_empty_quotes() {
        let mut vars = HashMap::new();
        vars.insert("empty", "".to_string());

        let result = expand_template("echo {empty}done", &vars);
        assert_eq!(result, "echo ''done");
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
        assert_eq!(result, "'test.mkv' {unknown} 'test.mkv'");
    }

    #[test]
    fn test_repeated_var() {
        let mut vars = HashMap::new();
        vars.insert("file", "test.mkv".to_string());

        let result = expand_template("cp {file} {file}.bak", &vars);
        assert_eq!(result, "cp 'test.mkv' 'test.mkv'.bak");
    }

    #[test]
    fn test_adjacent_braces() {
        let mut vars = HashMap::new();
        vars.insert("a", "1".to_string());
        vars.insert("b", "2".to_string());

        let result = expand_template("{a}{b}", &vars);
        assert_eq!(result, "'1''2'");
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
        assert_eq!(result, "'value'");
    }

    #[test]
    fn test_shell_metacharacters_escaped() {
        let mut vars = HashMap::new();
        vars.insert("file", "\"; rm -rf / #".to_string());

        let result = expand_template("process {file}", &vars);
        assert_eq!(result, "process '\"; rm -rf / #'");
    }

    #[test]
    fn test_single_quotes_in_value_escaped() {
        let mut vars = HashMap::new();
        vars.insert("title", "It's a Test".to_string());

        let result = expand_template("echo {title}", &vars);
        assert_eq!(result, "echo 'It'\\''s a Test'");
    }

    #[test]
    fn test_backticks_and_dollar_escaped() {
        let mut vars = HashMap::new();
        vars.insert("file", "$(whoami)`id`.mkv".to_string());

        let result = expand_template("cp {file} /out/", &vars);
        assert_eq!(result, "cp '$(whoami)`id`.mkv' /out/");
    }

    #[test]
    fn test_shell_escape_empty() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_with_single_quote() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    // --- prepare_post_rip tests ---

    fn hook_config(command: &str) -> crate::config::HookConfig {
        crate::config::HookConfig {
            command: Some(command.into()),
            on_failure: None,
            blocking: None,
            log_output: None,
        }
    }

    fn success_vars() -> HashMap<&'static str, String> {
        let mut vars = HashMap::new();
        vars.insert("status", "success".into());
        vars.insert("filename", "test.mkv".into());
        vars
    }

    fn failed_vars() -> HashMap<&'static str, String> {
        let mut vars = HashMap::new();
        vars.insert("status", "failed".into());
        vars.insert("error", "some error".into());
        vars
    }

    #[test]
    fn test_post_rip_skips_when_no_hooks() {
        let config = Config {
            post_rip: Some(hook_config("echo hi")),
            ..Default::default()
        };
        assert!(prepare_post_rip(&config, &success_vars(), true).is_none());
    }

    #[test]
    fn test_post_rip_skips_when_no_config() {
        let config = Config::default();
        assert!(prepare_post_rip(&config, &success_vars(), false).is_none());
    }

    #[test]
    fn test_post_rip_skips_when_no_command() {
        let config = Config {
            post_rip: Some(crate::config::HookConfig {
                command: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(prepare_post_rip(&config, &success_vars(), false).is_none());
    }

    #[test]
    fn test_post_rip_skips_when_empty_command() {
        let config = Config {
            post_rip: Some(hook_config("")),
            ..Default::default()
        };
        assert!(prepare_post_rip(&config, &success_vars(), false).is_none());
    }

    #[test]
    fn test_post_rip_fires_on_success() {
        let config = Config {
            post_rip: Some(hook_config("echo {filename}")),
            ..Default::default()
        };
        let result = prepare_post_rip(&config, &success_vars(), false);
        assert_eq!(result.as_deref(), Some("echo 'test.mkv'"));
    }

    #[test]
    fn test_post_rip_skips_failure_by_default() {
        let config = Config {
            post_rip: Some(hook_config("echo hi")),
            ..Default::default()
        };
        assert!(prepare_post_rip(&config, &failed_vars(), false).is_none());
    }

    #[test]
    fn test_post_rip_fires_on_failure_when_configured() {
        let config = Config {
            post_rip: Some(crate::config::HookConfig {
                command: Some("echo {error}".into()),
                on_failure: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = prepare_post_rip(&config, &failed_vars(), false);
        assert_eq!(result.as_deref(), Some("echo 'some error'"));
    }

    // --- prepare_post_session tests ---

    fn session_vars(failed: u32) -> HashMap<&'static str, String> {
        let mut vars = HashMap::new();
        vars.insert("total", "4".into());
        vars.insert("succeeded", (4 - failed).to_string());
        vars.insert("failed", failed.to_string());
        vars.insert("skipped", "0".into());
        vars.insert("label", "DISC_1".into());
        vars
    }

    #[test]
    fn test_post_session_skips_when_no_hooks() {
        let config = Config {
            post_session: Some(hook_config("echo done")),
            ..Default::default()
        };
        assert!(prepare_post_session(&config, &session_vars(0), true).is_none());
    }

    #[test]
    fn test_post_session_skips_when_no_config() {
        let config = Config::default();
        assert!(prepare_post_session(&config, &session_vars(0), false).is_none());
    }

    #[test]
    fn test_post_session_fires_on_all_success() {
        let config = Config {
            post_session: Some(hook_config("echo {label}")),
            ..Default::default()
        };
        let result = prepare_post_session(&config, &session_vars(0), false);
        assert_eq!(result.as_deref(), Some("echo 'DISC_1'"));
    }

    #[test]
    fn test_post_session_skips_on_failures_by_default() {
        let config = Config {
            post_session: Some(hook_config("echo done")),
            ..Default::default()
        };
        assert!(prepare_post_session(&config, &session_vars(2), false).is_none());
    }

    #[test]
    fn test_post_session_fires_on_failures_when_configured() {
        let config = Config {
            post_session: Some(crate::config::HookConfig {
                command: Some("echo {failed} failed".into()),
                on_failure: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = prepare_post_session(&config, &session_vars(2), false);
        assert_eq!(result.as_deref(), Some("echo '2' failed"));
    }
}
