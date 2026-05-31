use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn slide_one_runs_first_slide() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(&slides, "printf 'first\\n'\nprintf 'second\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "1"])
        .arg(&slides)
        .assert()
        .success()
        .stdout(predicate::str::contains("first"))
        .stdout(predicate::str::contains("second").not());
}

#[test]
fn slide_two_runs_second_slide() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(&slides, "printf 'first\\n'\nprintf 'second\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "2"])
        .arg(&slides)
        .assert()
        .success()
        .stdout(predicate::str::contains("second"))
        .stdout(predicate::str::contains("first").not());
}

#[test]
fn slide_zero_fails() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(&slides, "printf 'first\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "0"])
        .arg(&slides)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--slide must be at least 1"));
}

#[test]
fn slide_out_of_range_fails() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(&slides, "printf 'first\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "2"])
        .arg(&slides)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--slide 2 is out of range; there are 1 slides",
        ));
}

#[test]
fn empty_or_comment_only_file_fails() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(&slides, "\n  # comment\n\t\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "1"])
        .arg(&slides)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no slide commands found"));
}

#[test]
fn backslash_continuation_runs_as_one_slide() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(
        &slides,
        "printf '%s %s\\n' \\\nhello \\\nworld\nprintf 'second\\n'\n",
    )
    .unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "1"])
        .arg(&slides)
        .assert()
        .success()
        .stdout(predicate::str::contains("hello world"))
        .stdout(predicate::str::contains("second").not());
}

#[test]
fn continuation_followed_by_comment_fails() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    fs::write(&slides, "printf hello \\\n# comment\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "1"])
        .arg(&slides)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "line continuation must be followed by a command line",
        ));
}

#[test]
fn command_cwd_is_slide_file_directory() {
    let dir = tempdir().unwrap();
    let slides_dir = dir.path().join("deck");
    fs::create_dir(&slides_dir).unwrap();
    let slides = slides_dir.join("slides.txt");
    fs::write(&slides, "pwd\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "1"])
        .arg(&slides)
        .assert()
        .success()
        .stdout(predicate::str::contains(slides_dir.display().to_string()));
}

#[test]
fn multiple_input_files_preserve_slide_order() {
    let dir = tempdir().unwrap();
    let first = dir.path().join("first.txt");
    let second = dir.path().join("second.txt");
    fs::write(&first, "printf 'one\\n'\n").unwrap();
    fs::write(&second, "printf 'two\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--slide", "2"])
        .arg(&first)
        .arg(&second)
        .assert()
        .success()
        .stdout(predicate::str::contains("two"))
        .stdout(predicate::str::contains("one").not());
}

#[test]
fn pdf_export_creates_pdf() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    let output = dir.path().join("deck.pdf");
    fs::write(&slides, "printf 'hello pdf\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--pdf"])
        .arg(&output)
        .arg(&slides)
        .assert()
        .success();

    let bytes = fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    assert!(!bytes.is_empty());
}

#[test]
fn pdf_export_accepts_multiple_slides() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    let output = dir.path().join("deck.pdf");
    fs::write(&slides, "printf 'one\\n'\nprintf 'two\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--pdf"])
        .arg(&output)
        .arg(&slides)
        .assert()
        .success();

    assert!(fs::metadata(&output).unwrap().len() > 0);
}

#[test]
fn pdf_conflicts_with_slide() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    let output = dir.path().join("deck.pdf");
    fs::write(&slides, "printf 'hello\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--pdf"])
        .arg(&output)
        .args(["--slide", "1"])
        .arg(&slides)
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn pdf_export_accepts_dimensions() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    let output = dir.path().join("deck.pdf");
    fs::write(&slides, "printf 'sized\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--pdf"])
        .arg(&output)
        .args(["--pdfcols", "80", "--pdfrows", "24"])
        .arg(&slides)
        .assert()
        .success();

    assert!(fs::metadata(&output).unwrap().len() > 0);
}

#[test]
fn pdf_export_accepts_capture_terminal_style() {
    let dir = tempdir().unwrap();
    let slides = dir.path().join("slides.txt");
    let output = dir.path().join("deck.pdf");
    fs::write(&slides, "printf 'styled\\n'\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--pdf"])
        .arg(&output)
        .args(["--capture-terminal-style"])
        .arg(&slides)
        .assert()
        .success();

    assert!(fs::metadata(&output).unwrap().len() > 0);
}

#[test]
fn pdf_export_loads_aliases() {
    let dir = tempdir().unwrap();
    let aliases = dir.path().join("aliases");
    let slides = dir.path().join("slides.txt");
    let output = dir.path().join("deck.pdf");
    fs::write(&aliases, "alias hi='printf alias-ok\\n'\n").unwrap();
    fs::write(&slides, "hi\n").unwrap();

    Command::cargo_bin("tuition")
        .unwrap()
        .args(["--aliases"])
        .arg(&aliases)
        .args(["--pdf"])
        .arg(&output)
        .arg(&slides)
        .assert()
        .success();

    assert!(fs::metadata(&output).unwrap().len() > 0);
}
