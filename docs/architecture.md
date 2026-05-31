# Architecture

`tuition` is a command-driven terminal slide presenter. A deck is one or more text files containing shell command lines; each active slide is executed as a command.

## Slide model and parsing

- Input files are read in the order supplied on the command line.
- Blank and whitespace-only lines are ignored.
- Lines whose trimmed form starts with `#` are ignored.
- Inline `#` characters are preserved as part of the command.
- A trailing `\` joins the following command line to the current command, with the `\` removed.
- Continuations may span multiple command lines, but a `\` followed by a blank line, comment line, or end of file is a parse error.
- Each slide stores the command text, source file, and source line where the command begins.
- A slide command's working directory is the directory containing its slide file.

## Execution model

Slides run directly attached to the user's terminal. `tuition` does not capture output, cache output, render command output in a UI layer, or use ratatui. This is intentional: stdout, stderr, prompts, cursor control, interactive programs, and terminal image protocols all go directly to the terminal.

The active slide is executed with:

```sh
$SHELL -lc '<generated script>'
```

If `$SHELL` is unset, `/bin/sh` is used. The generated script clears the terminal, prepares shell-specific behavior and aliases, prints the status bar, then runs the slide command.

## PDF export

Interactive presentation keeps the terminal-attached execution model described above. PDF export is a separate mode selected with `--pdf`; it does not add an output cache, captured-output UI renderer, ratatui layer, or scrolling behavior to normal presentation.

In PDF export mode, each slide command is run in a PTY sized from `--pdfcols`/`--pdfrows`, the current terminal size, or the `100x30` fallback. The generated shell script clears the terminal, prepares shell-specific behavior and aliases, and runs the slide command without printing the interactive status bar. PTY output is parsed into a final terminal buffer, and that final frame is rendered as one PDF page per slide. ANSI foreground colors are mapped to PDF text colors. iTerm2/imgcat inline image escape sequences are decoded and embedded into the PDF. By default the PDF uses black text on a white background; `--capture-terminal-style` asks the calling terminal for its default foreground/background colors with OSC 10/11 and uses them when available. Export mode assumes slide commands are non-interactive.

## Raw mode lifecycle

Raw mode is enabled only while reading navigation keys. It is disabled before running slide commands or opening a temporary shell, and cleanup also attempts to disable raw mode before exit.

## Navigation behavior

Startup runs slide 1. Moving to a different slide runs that slide. `r` reruns the current slide. Moving past deck bounds does nothing and does not rerun. `s` opens a temporary shell and, on return, redraws navigation hints without rerunning the current slide. `q` asks for confirmation; Ctrl-C exits immediately.

## Aliases

Explicit aliases are passed with `--aliases` as semicolon-separated paths. Empty segments are ignored. Relative paths are resolved relative to the process current directory; absolute paths remain absolute.

Without `--aliases`, `tuition` searches upward from the slide command cwd for the nearest file named `aliases`. Alias files are sourced before slide commands. Bash enables `expand_aliases`, and alias definitions are also converted into shell functions for non-interactive command execution.

## Temporary shell

The `s` key launches the user's shell with a `(TUITION)` prompt. Bash, zsh, and fish receive shell-specific temporary configuration files. Aliases are loaded where applicable. Exiting the temporary shell returns to `tuition` without rerunning the slide.

## Code organization

- `cli.rs`: clap command-line types.
- `slides.rs`: slide parsing and slide cwd resolution.
- `aliases.rs`: alias argument parsing, upward lookup, and alias function generation.
- `shell.rs`: shell detection, shell quoting, slide script generation, PDF slide script generation, and temporary shell launching/config generation.
- `pdf.rs`: PTY-backed PDF export and final terminal-buffer rendering.
- `terminal.rs`: raw mode/key reading/status output abstraction and crossterm implementation.
- `app.rs`: app state, navigation, status bar, and deterministic app loop.
- `lib.rs`: top-level entrypoint and module wiring.
- `main.rs`: parse CLI and call the library.

## Test architecture

The project uses pure unit tests for parsing, aliases, shell script/config generation, and status formatting. App-loop tests use fake terminal and command-runner implementations so navigation behavior can be tested without spawning real shells or manipulating the real terminal. CLI integration tests continue to exercise the compiled binary for user-visible behavior.

Coverage can be generated locally with `cargo llvm-cov`; CI uploads a Linux coverage artifact without enforcing a coverage percentage.
