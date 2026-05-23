use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideCommand {
    pub file: PathBuf,
    pub line: usize,
    pub command: String,
}

pub fn parse_slide_files(files: &[PathBuf]) -> Result<Vec<SlideCommand>> {
    let mut commands = Vec::new();

    for file in files {
        let contents = fs::read_to_string(file)
            .with_context(|| format!("failed to read slide file {}", file.display()))?;

        for (idx, line) in contents.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            commands.push(SlideCommand {
                file: file.clone(),
                line: idx + 1,
                command: line.to_string(),
            });
        }
    }

    Ok(commands)
}

pub fn slide_command_cwd(slide: &SlideCommand) -> PathBuf {
    slide
        .file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parser_ignores_blank_and_comment_lines() {
        let path = temp_file(
            "slides",
            "\n# comment\n echo one\n\t# indented comment\necho two\n",
        );
        let commands = parse_slide_files(std::slice::from_ref(&path)).unwrap();

        assert_eq!(
            commands,
            vec![
                SlideCommand {
                    file: path.clone(),
                    line: 3,
                    command: " echo one".to_string()
                },
                SlideCommand {
                    file: path,
                    line: 5,
                    command: "echo two".to_string()
                },
            ]
        );
    }

    #[test]
    fn parser_preserves_multiple_file_order() {
        let first = temp_file("slides-a", "echo a1\necho a2\n");
        let second = temp_file("slides-b", "echo b1\n");
        let commands = parse_slide_files(&[first.clone(), second.clone()]).unwrap();

        assert_eq!(commands[0].file, first);
        assert_eq!(commands[0].command, "echo a1");
        assert_eq!(commands[1].command, "echo a2");
        assert_eq!(commands[2].file, second);
        assert_eq!(commands[2].command, "echo b1");
    }

    #[test]
    fn parser_missing_file_returns_contextual_error() {
        let missing = unique_temp_path("missing-slides");
        let err = parse_slide_files(std::slice::from_ref(&missing))
            .unwrap_err()
            .to_string();

        assert!(err.contains("failed to read slide file"));
        assert!(err.contains(&missing.display().to_string()));
    }

    #[test]
    fn parser_ignores_whitespace_only_lines() {
        let path = temp_file("slides", "   \n\t\n echo one\n");
        let commands = parse_slide_files(&[path]).unwrap();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].line, 3);
        assert_eq!(commands[0].command, " echo one");
    }

    #[test]
    fn parser_ignores_indented_comments() {
        let path = temp_file("slides", "  # comment\necho one\n");
        let commands = parse_slide_files(&[path]).unwrap();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].line, 2);
    }

    #[test]
    fn parser_preserves_inline_hashes() {
        let path = temp_file("slides", "echo before # after\n");
        let commands = parse_slide_files(&[path]).unwrap();

        assert_eq!(commands[0].command, "echo before # after");
    }

    #[test]
    fn slide_command_cwd_uses_slide_file_directory() {
        let slide = SlideCommand {
            file: PathBuf::from("/tmp/tuition/slides.txt"),
            line: 1,
            command: "pwd".to_string(),
        };

        assert_eq!(slide_command_cwd(&slide), PathBuf::from("/tmp/tuition"));
    }

    #[test]
    fn slide_command_cwd_returns_dot_for_files_without_parent_directory() {
        let slide = SlideCommand {
            file: PathBuf::from("slides.txt"),
            line: 1,
            command: "pwd".to_string(),
        };

        assert_eq!(slide_command_cwd(&slide), PathBuf::from("."));
    }

    fn temp_file(name: &str, contents: &str) -> PathBuf {
        let path = unique_temp_path(name);
        fs::write(&path, contents).unwrap();
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
