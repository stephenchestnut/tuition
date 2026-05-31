# tuition

<p align="center">
  <img src="tuition_icon_128_easel.png" alt="tuition logo" width="128" height="128">
</p>

`tuition` is a tiny command-driven terminal slide presenter. Your slide deck is just a text file where each slide is a shell command.

Slide commands run directly attached to your terminal, so normal terminal output, interactive programs, ANSI control sequences, and terminal image protocols all work as normal.

## Quick start

Clone the repo and run an included example with `cargo run`:

```sh
git clone https://github.com/stephenchestnut/tuition.git
cd tuition
cargo run -- examples/hello_world.txt
```

Try the color example too:

```sh
cargo run -- --aliases ./aliases examples/colors.txt
```

Use `l`, right arrow, or space to advance; use `h` or left arrow to go back; press `q` to quit.

## Installation / build

```sh
cargo build --release
cargo install --path .
```

The binary will be at:

```sh
target/release/tuition
```

## Basic usage

```sh
tuition file1.txt file2.txt
```

Input files are read in order. Each slide is run when it becomes active.

## Slide file format

- One slide command per non-blank line.
- Blank or whitespace-only lines are ignored.
- Lines whose trimmed form starts with `#` are ignored.
- Inline `#` characters are preserved as part of the command.
- If a command line ends with `\`, the next line is joined to it, with the `\` removed.
- Continuations may span multiple command lines, but the line after `\` must be another command line, not a blank line or comment.

Example:

```txt
# intro
echo 'Welcome to tuition'

printf '\e[31mred text\e[0m\n'
printf '%s %s\n' \
  long \
  command
python3 demo.py
```

## Keyboard controls

- `l` / right arrow / space: next slide
- `h` / left arrow: previous slide
- `r`: rerun current slide
- `s`: open a temporary shell; return without rerunning the current slide
- `q`: quit confirmation
- Ctrl-C: quit immediately

There are no scrolling controls; slide output is written directly to the terminal.

## Options

Run a single slide and exit:

```sh
tuition --slide 3 slides.txt
```

Load alias files explicitly:

```sh
tuition --aliases './aliases;./more-aliases' slides.txt
```

Export a deck to PDF:

```sh
tuition --pdf deck.pdf slides.txt
tuition --pdf deck.pdf --pdfcols 120 --pdfrows 40 slides.txt
tuition --pdf deck.pdf --capture-terminal-style slides.txt
```

`--pdf` creates one PDF page per slide from the final terminal screen after each command exits, including ANSI foreground colors, bold/italic/underline styles, and iTerm2/imgcat inline images. It cannot be used with `--slide`. PDF export assumes non-interactive commands. `--pdfcols` and `--pdfrows` control the export terminal size; omitted dimensions use the current terminal size, or `100x30` if size detection fails. By default, PDF export uses black text on a white background; add `--capture-terminal-style` to query the calling terminal's default foreground/background colors and use those instead, falling back to the defaults if the terminal does not respond.

## Execution behavior

- Commands run via `$SHELL -lc`, falling back to `/bin/sh` when `$SHELL` is unset.
- Commands run from the directory containing their slide file.
- A status bar is printed before each slide command with slide number, total slides, file, line, and key hints.
- Output is not captured or re-rendered by `tuition`; it goes directly to the terminal.
- Interactive commands are allowed.
- Startup runs the first slide.
- Navigating to a different slide runs that slide.
- Pressing `r` reruns the current slide.
- Moving past deck bounds does nothing.
- Returning from a temporary shell does not rerun the current slide.

## Aliases

Alias files can be passed explicitly with `--aliases`. Separate multiple files with semicolons:

```sh
tuition --aliases './aliases;./more-aliases' slides.txt
```

If `--aliases` is not supplied, `tuition` searches upward from the slide command's working directory for the nearest file named `aliases`.

When aliases are used, bash gets `shopt -s expand_aliases`, alias files are sourced before slide commands, and alias definitions are also converted into shell functions for use in non-interactive slide commands.

## Examples

```txt
# slides.txt
echo 'Slide 1'
printf 'Slide 2 from %s\n' "$PWD"
read -p 'Press enter inside this slide command...'
```

```sh
tuition slides.txt
tuition --slide 2 slides.txt
tuition --aliases './aliases' slides.txt
tuition --pdf deck.pdf slides.txt
```

## Agent skill

This repo includes an agent skill for creating tuition presentations at [`skills/tuition.md`](skills/tuition.md). It explains the slide text format, style guidance, aliases, colors, and includes the hello world presentation as an example.

## Development

See [docs/architecture.md](docs/architecture.md) for implementation details.

```sh
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Local coverage:

```sh
cargo install cargo-llvm-cov
cargo llvm-cov --all-targets --all-features
```
