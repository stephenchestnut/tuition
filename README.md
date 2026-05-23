# tuition

`tuition` is a tiny command-driven terminal slide presenter. A slide deck is a text file where each slide is a shell command.

Slide commands run directly attached to your terminal, so normal terminal output, interactive programs, ANSI control sequences, and terminal image protocols can work naturally.

## Quick start

Clone the repo and run an included example with `cargo run`:

```sh
git clone https://github.com/stephenchestnut/tuition.git
cd tuition
cargo run -- examples/hello_world.txt
```

Try the color example too:

```sh
cargo run -- examples/colors.txt
```

Use `l`, right arrow, or space to advance; use `h` or left arrow to go back; press `q` to quit.

## Installation / build

```sh
cargo build --release
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
- The original command line text is preserved.

Example:

```txt
# intro
echo 'Welcome to tuition'

printf '\e[31mred text\e[0m\n'
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
```

## Development

```sh
cargo fmt
cargo clippy --all-targets --all-features
cargo test
```
