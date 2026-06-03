use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::shell::shell_single_quote;

pub fn parse_aliases_arg(aliases: Option<&str>) -> Result<Vec<PathBuf>> {
    aliases
        .into_iter()
        .flat_map(|aliases| aliases.split(';'))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| absolutize_path(Path::new(path)))
        .collect()
}

fn absolutize_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()
            .context("failed to determine current directory")?
            .join(path))
    }
}

pub fn aliases_prelude(
    shell_name: Option<&str>,
    aliases_paths: &[PathBuf],
    start_dir: &Path,
) -> Option<String> {
    let aliases = effective_aliases_paths(aliases_paths, start_dir)?;

    if matches!(shell_name, Some("fish")) {
        let mut prelude = String::new();
        for aliases_path in aliases {
            prelude.push_str(&fish_aliases_definitions(&aliases_path));
        }
        return Some(prelude);
    }

    let mut prelude = String::new();
    if matches!(shell_name, Some("bash")) {
        prelude.push_str("shopt -s expand_aliases\n");
    }
    for aliases_path in aliases {
        let quoted_path = shell_single_quote(&aliases_path.to_string_lossy());
        prelude.push_str(&format!(
            "if [ -r {} ]; then\n  . {}\nfi\n",
            quoted_path, quoted_path
        ));
        prelude.push_str(&aliases_function_definitions(&aliases_path));
    }
    Some(prelude)
}

pub fn effective_aliases_paths(
    aliases_paths: &[PathBuf],
    start_dir: &Path,
) -> Option<Vec<PathBuf>> {
    if aliases_paths.is_empty() {
        find_aliases_file(start_dir).map(|path| vec![path])
    } else {
        Some(aliases_paths.to_vec())
    }
}

pub fn aliases_function_definitions(path: &Path) -> String {
    let Ok(contents) = fs::read_to_string(path) else {
        return String::new();
    };

    let mut defs = String::new();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("alias ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !is_shell_name(name) {
            continue;
        }

        let body = strip_matching_quotes(value.trim());
        defs.push_str(name);
        defs.push_str("() { ");
        defs.push_str(body);
        defs.push_str("; }\n");
    }
    defs
}

pub fn fish_aliases_definitions(path: &Path) -> String {
    let Ok(contents) = fs::read_to_string(path) else {
        return String::new();
    };

    let mut defs = String::new();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("export ") {
            let Some((name, value)) = rest.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if is_shell_name(name) {
                defs.push_str("set -gx ");
                defs.push_str(name);
                defs.push(' ');
                defs.push_str(&shell_single_quote(posix_assignment_value(value.trim())));
                defs.push_str("\n");
            }
            continue;
        }

        let Some(rest) = trimmed.strip_prefix("alias ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !is_shell_name(name) {
            continue;
        }

        let body = translate_posix_vars(strip_matching_quotes(value.trim()));
        defs.push_str("function ");
        defs.push_str(name);
        defs.push_str("\n  ");
        defs.push_str(&body);
        defs.push_str("\nend\n");
    }
    defs
}

fn strip_matching_quotes(s: &str) -> &str {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        let first = bytes[0];
        let last = bytes[s.len() - 1];
        if (first == b'\'' || first == b'\"') && first == last {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn posix_assignment_value(value: &str) -> &str {
    let value = value.trim();
    let Some(quote) = value.as_bytes().first().copied() else {
        return value.split_whitespace().next().unwrap_or("");
    };
    if quote != b'\'' && quote != b'\"' {
        return value.split_whitespace().next().unwrap_or("");
    }
    value[1..]
        .find(quote as char)
        .map(|end| &value[1..end + 1])
        .unwrap_or(value)
}

fn translate_posix_vars(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        out.push('$');
        out.push_str(name);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn is_shell_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn find_aliases_file(start_dir: &Path) -> Option<PathBuf> {
    for dir in start_dir.ancestors() {
        let candidate = dir.join("aliases");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_semicolon_separated_aliases_arg() {
        let aliases = parse_aliases_arg(Some("aliases; examples/more-aliases ;")).unwrap();
        let cwd = env::current_dir().unwrap();

        assert_eq!(
            aliases,
            vec![cwd.join("aliases"), cwd.join("examples/more-aliases")]
        );
    }

    #[test]
    fn empty_alias_arg_segments_are_ignored() {
        let aliases = parse_aliases_arg(Some("; aliases ;; ;")).unwrap();
        let cwd = env::current_dir().unwrap();

        assert_eq!(aliases, vec![cwd.join("aliases")]);
    }

    #[test]
    fn absolute_alias_paths_remain_absolute() {
        let absolute = unique_temp_path("aliases");
        let aliases = parse_aliases_arg(Some(&absolute.to_string_lossy())).unwrap();

        assert_eq!(aliases, vec![absolute]);
    }

    #[test]
    fn bash_alias_prelude_includes_expand_aliases() {
        let aliases = unique_temp_path("aliases");
        let prelude = aliases_prelude(Some("bash"), &[aliases], Path::new(".")).unwrap();

        assert!(prelude.contains("shopt -s expand_aliases"));
    }

    #[test]
    fn aliases_function_definitions_turn_aliases_into_functions() {
        let path = temp_file(
            "aliases",
            "alias el=\"printf \\\"\\n\\n\\\"\"\nalias s4=\"printf \\\"    \\\"\"\nexport FOO=bar\n",
        );

        let defs = aliases_function_definitions(&path);
        assert!(defs.contains("el() { printf \\\"\\n\\n\\\"; }"));
        assert!(defs.contains("s4() { printf \\\"    \\\"; }"));
    }

    #[test]
    fn invalid_alias_names_are_ignored() {
        let path = temp_file("aliases", "alias 1bad='echo no'\nalias good='echo yes'\n");
        let defs = aliases_function_definitions(&path);

        assert!(!defs.contains("1bad()"));
        assert!(defs.contains("good() { echo yes; }"));
    }

    #[test]
    fn quoted_alias_bodies_are_stripped() {
        let path = temp_file(
            "aliases",
            "alias sq='echo single'\nalias dq=\"echo double\"\n",
        );
        let defs = aliases_function_definitions(&path);

        assert!(defs.contains("sq() { echo single; }"));
        assert!(defs.contains("dq() { echo double; }"));
    }

    #[test]
    fn missing_alias_file_returns_empty_function_definitions() {
        let missing = unique_temp_path("missing-aliases");

        assert_eq!(aliases_function_definitions(&missing), "");
    }

    #[test]
    fn fish_aliases_convert_exports_and_aliases() {
        let path = temp_file(
            "aliases",
            "export FGRED=\"\\033[31m\"   # Red\nalias fgred=\"printf ${FGRED}\"\n",
        );

        let defs = fish_aliases_definitions(&path);
        assert!(defs.contains("set -gx FGRED '\\033[31m'"));
        assert!(defs.contains("function fgred\n  printf $FGRED\nend"));
    }

    #[test]
    fn fish_alias_prelude_uses_fish_syntax() {
        let path = temp_file("aliases", "alias hi='echo hi'\n");

        let prelude = aliases_prelude(Some("fish"), &[path], Path::new(".")).unwrap();
        assert!(prelude.contains("function hi"));
        assert!(!prelude.contains("if [ -r"));
    }

    #[test]
    fn find_aliases_file_finds_nearest_upward_aliases() {
        let root = temp_dir("aliases-root");
        let child = root.join("child");
        let grandchild = child.join("grandchild");
        fs::create_dir_all(&grandchild).unwrap();
        fs::write(root.join("aliases"), "alias root='echo root'\n").unwrap();
        fs::write(child.join("aliases"), "alias child='echo child'\n").unwrap();

        assert_eq!(find_aliases_file(&grandchild), Some(child.join("aliases")));
    }

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path = unique_temp_path(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path = unique_temp_path(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("tuition-{name}-{}-{unique}", std::process::id()))
    }
}
