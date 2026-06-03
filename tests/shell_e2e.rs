use assert_cmd::cargo::cargo_bin;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use tempfile::{TempDir, tempdir};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestShell {
    Bash,
    Zsh,
    Fish,
    Sh,
}

impl TestShell {
    fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Sh => "sh",
        }
    }

    fn env_override(self) -> &'static str {
        match self {
            Self::Bash => "TUITION_TEST_BASH",
            Self::Zsh => "TUITION_TEST_ZSH",
            Self::Fish => "TUITION_TEST_FISH",
            Self::Sh => "TUITION_TEST_SH",
        }
    }
}

fn find_shell(shell: TestShell) -> Option<PathBuf> {
    if let Ok(path) = env::var(shell.env_override())
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path));
    }

    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {}", shell.name()))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.lines().next()?.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn shell_or_skip(shell: TestShell) -> Option<PathBuf> {
    let found = find_shell(shell);
    if found.is_none() {
        let message = format!(
            "skipping real-shell e2e test: {} not found (set {} to override)",
            shell.name(),
            shell.env_override()
        );
        if env::var_os("TUITION_REQUIRE_E2E_SHELLS").is_some() {
            panic!("{message}");
        }
        eprintln!("{message}");
    }
    found
}

fn run_single_slide(shell_path: &Path, slide_command: &str, aliases: Option<&Path>) -> String {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(&slides, format!("{slide_command}\n")).unwrap();

    let mut command = assert_cmd::Command::cargo_bin("tuition").unwrap();
    command
        .env("SHELL", shell_path)
        .env("TERM", "xterm")
        .args(["--slide", "1"]);
    if let Some(aliases) = aliases {
        command.arg("--aliases").arg(aliases);
    }
    let output = command.arg(&slides).assert().success().get_output().clone();

    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn bash_slide_runs_under_real_bash() {
    let Some(shell) = shell_or_skip(TestShell::Bash) else {
        return;
    };

    let stdout = run_single_slide(
        &shell,
        "test -n \"$BASH_VERSION\" && printf 'bash-ok:%s\\n' \"$BASH_VERSION\"",
        None,
    );

    assert!(stdout.contains("bash-ok:"), "stdout was: {stdout:?}");
}

#[test]
fn zsh_slide_runs_under_real_zsh() {
    let Some(shell) = shell_or_skip(TestShell::Zsh) else {
        return;
    };

    let stdout = run_single_slide(
        &shell,
        "test -n \"$ZSH_VERSION\" && printf 'zsh-ok:%s\\n' \"$ZSH_VERSION\"",
        None,
    );

    assert!(stdout.contains("zsh-ok:"), "stdout was: {stdout:?}");
}

#[test]
fn fish_slide_runs_under_real_fish() {
    let Some(shell) = shell_or_skip(TestShell::Fish) else {
        return;
    };

    let stdout = run_single_slide(
        &shell,
        "test -n \"$version\"; and printf 'fish-ok\\n'",
        None,
    );

    assert!(stdout.contains("fish-ok"), "stdout was: {stdout:?}");
}

#[test]
fn sh_slide_runs_under_real_sh() {
    let Some(shell) = shell_or_skip(TestShell::Sh) else {
        return;
    };

    let stdout = run_single_slide(&shell, "printf 'sh-ok\\n'", None);

    assert!(stdout.contains("sh-ok"), "stdout was: {stdout:?}");
}

#[test]
fn bash_loads_aliases_in_non_interactive_slide() {
    alias_slide_works(TestShell::Bash);
}

#[test]
fn zsh_loads_aliases_in_non_interactive_slide() {
    alias_slide_works(TestShell::Zsh);
}

#[test]
fn fish_loads_aliases_in_non_interactive_slide() {
    alias_slide_works(TestShell::Fish);
}

#[test]
fn sh_loads_alias_functions_in_non_interactive_slide() {
    alias_slide_works(TestShell::Sh);
}

fn alias_slide_works(shell: TestShell) {
    let Some(shell_path) = shell_or_skip(shell) else {
        return;
    };
    let dir = tempdir().unwrap();
    let aliases = dir.path().join("aliases");
    fs::write(
        &aliases,
        "export TUITION_E2E_MSG=alias-ok\nalias hi='printf \"%s\\n\" \"$TUITION_E2E_MSG\"'\n",
    )
    .unwrap();

    let stdout = run_single_slide(&shell_path, "hi", Some(&aliases));

    assert!(stdout.contains("alias-ok"), "stdout was: {stdout:?}");
}

#[test]
fn zsh_slide_uses_sh_word_splitting() {
    let Some(shell) = shell_or_skip(TestShell::Zsh) else {
        return;
    };

    let stdout = run_single_slide(
        &shell,
        "row='a b'; for name in $row; do printf '<%s>\\n' \"$name\"; done",
        None,
    );

    assert!(stdout.contains("<a>"), "stdout was: {stdout:?}");
    assert!(stdout.contains("<b>"), "stdout was: {stdout:?}");
}

#[test]
fn fish_accepts_native_slide_syntax() {
    let Some(shell) = shell_or_skip(TestShell::Fish) else {
        return;
    };

    let stdout = run_single_slide(
        &shell,
        "test -n \"$version\"; and printf 'fish-ok\\n'",
        None,
    );

    assert!(stdout.contains("fish-ok"), "stdout was: {stdout:?}");
}

#[test]
fn temporary_bash_shell_uses_prompt_aliases_and_original_rc() {
    temporary_shell_works(TestShell::Bash);
}

#[test]
fn temporary_zsh_shell_uses_prompt_aliases_and_original_rc() {
    temporary_shell_works(TestShell::Zsh);
}

#[test]
fn temporary_fish_shell_uses_prompt_aliases_and_original_rc() {
    temporary_shell_works(TestShell::Fish);
}

fn temporary_shell_works(shell: TestShell) {
    let Some(shell_path) = shell_or_skip(shell) else {
        return;
    };
    let fixture = TemporaryShellFixture::new(shell);
    let bin = cargo_bin("tuition");

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let mut command = CommandBuilder::new(bin);
    command.arg("--aliases");
    command.arg(&fixture.aliases);
    command.arg(&fixture.slides);
    command.cwd(fixture.dir.path());
    command.env("SHELL", &shell_path);
    command.env("TERM", "xterm");
    command.env("HOME", &fixture.home);
    if let Some(zdotdir) = &fixture.zdotdir {
        command.env("ZDOTDIR", zdotdir);
    } else {
        command.env_remove("ZDOTDIR");
    }
    if let Some(xdg_config_home) = &fixture.xdg_config_home {
        command.env("XDG_CONFIG_HOME", xdg_config_home);
    } else {
        command.env_remove("XDG_CONFIG_HOME");
    }

    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let output = spawn_pty_reader(reader);
    let mut transcript = String::new();

    read_until(
        &output,
        &mut transcript,
        "slide-ready",
        Duration::from_secs(10),
    );
    writer.write_all(b"s").unwrap();
    writer.flush().unwrap();
    read_until(
        &output,
        &mut transcript,
        "(TUITION)",
        Duration::from_secs(20),
    );

    writer.write_all(b"hi\n").unwrap();
    writer.flush().unwrap();
    read_until(
        &output,
        &mut transcript,
        "alias-ok",
        Duration::from_secs(10),
    );

    writer.write_all(b"fromrc\n").unwrap();
    writer.flush().unwrap();
    read_until(
        &output,
        &mut transcript,
        fixture.fromrc_output,
        Duration::from_secs(10),
    );

    writer.write_all(b"printf 'inside-shell-ok\\n'\n").unwrap();
    writer.flush().unwrap();
    read_until(
        &output,
        &mut transcript,
        "inside-shell-ok",
        Duration::from_secs(10),
    );

    writer.write_all(b"exit\n").unwrap();
    writer.flush().unwrap();
    read_until(
        &output,
        &mut transcript,
        "(r)erun | (s)hell | (q)uit",
        Duration::from_secs(10),
    );

    thread::sleep(Duration::from_millis(200));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    read_until(
        &output,
        &mut transcript,
        "Quit tuition? y/n",
        Duration::from_secs(10),
    );
    writer.write_all(b"y").unwrap();
    writer.flush().unwrap();
    wait_for_child_exit(&mut child, Duration::from_secs(10));

    assert!(transcript.contains("slide-ready"), "{transcript:?}");
    assert!(transcript.contains("(TUITION)"), "{transcript:?}");
    assert!(transcript.contains("alias-ok"), "{transcript:?}");
    assert!(transcript.contains(fixture.fromrc_output), "{transcript:?}");
    assert!(transcript.contains("inside-shell-ok"), "{transcript:?}");
    assert!(
        transcript.contains("(r)erun | (s)hell | (q)uit"),
        "{transcript:?}"
    );
}

struct TemporaryShellFixture {
    dir: TempDir,
    slides: PathBuf,
    aliases: PathBuf,
    home: PathBuf,
    zdotdir: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    fromrc_output: &'static str,
}

impl TemporaryShellFixture {
    fn new(shell: TestShell) -> Self {
        let dir = tempdir().unwrap();
        let slides = dir.path().join("slides.txt");
        let aliases = dir.path().join("aliases");
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(&slides, "printf 'slide-ready\\n'\n").unwrap();
        fs::write(
            &aliases,
            "export TUITION_E2E_MSG=alias-ok\nalias hi='printf \"%s\\n\" \"$TUITION_E2E_MSG\"'\n",
        )
        .unwrap();

        match shell {
            TestShell::Bash => {
                fs::write(
                    home.join(".bashrc"),
                    "alias fromrc='printf \"fromrc-bash\\n\"'\n",
                )
                .unwrap();
                Self {
                    dir,
                    slides,
                    aliases,
                    home,
                    zdotdir: None,
                    xdg_config_home: None,
                    fromrc_output: "fromrc-bash",
                }
            }
            TestShell::Zsh => {
                let zdotdir = dir.path().join("zdotdir");
                fs::create_dir_all(&zdotdir).unwrap();
                fs::write(
                    zdotdir.join(".zshrc"),
                    "alias fromrc='printf \"fromrc-zsh\\n\"'\n",
                )
                .unwrap();
                Self {
                    dir,
                    slides,
                    aliases,
                    home,
                    zdotdir: Some(zdotdir),
                    xdg_config_home: None,
                    fromrc_output: "fromrc-zsh",
                }
            }
            TestShell::Fish => {
                let xdg_config_home = dir.path().join("xdg-config");
                let fish_dir = xdg_config_home.join("fish");
                fs::create_dir_all(&fish_dir).unwrap();
                fs::write(
                    fish_dir.join("config.fish"),
                    "function fromrc\n  printf \"fromrc-fish\\n\"\nend\n",
                )
                .unwrap();
                Self {
                    dir,
                    slides,
                    aliases,
                    home,
                    zdotdir: None,
                    xdg_config_home: Some(xdg_config_home),
                    fromrc_output: "fromrc-fish",
                }
            }
            TestShell::Sh => unreachable!("sh PTY temporary-shell test is intentionally optional"),
        }
    }
}

fn wait_for_child_exit(child: &mut Box<dyn portable_pty::Child + Send + Sync>, timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    panic!("timed out waiting for tuition to exit");
}

fn spawn_pty_reader(mut reader: Box<dyn Read + Send>) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                        .is_err()
                    {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
    rx
}

fn read_until(output: &Receiver<String>, transcript: &mut String, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if transcript.contains(needle) {
            return;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        match output.recv_timeout(remaining.min(Duration::from_millis(100))) {
            Ok(chunk) => transcript.push_str(&chunk),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if transcript.contains(needle) {
        return;
    }
    panic!("timed out waiting for {needle:?}; transcript: {transcript:?}");
}
