# Implementation Plan

## Key semantic choice

`tuition` will **not cache slide outputs**.

Each time a slide becomes active via navigation, its command is re-executed. That means:

```txt
slide 1 -> slide 2 -> slide 1
```

executes slide 1 twice.

Important nuance:

> Re-execute when the active slide changes, not on every TUI redraw.

These should **not** re-execute the command:

- terminal resize
- scrolling with `j/k`
- footer redraw
- quit confirmation open/cancel

These **should** re-execute:

- navigating forward into a slide
- navigating backward into a slide
- startup display of first slide
- returning to a slide later

This preserves dynamic output while avoiding pathological re-execution during normal rendering.

## Final v1 behavior

```sh
tuition file1.txt file2.txt
```

- reads all input files in order
- ignores blank lines
- ignores lines whose trimmed form starts with `#`
- each remaining line is one slide command
- command is executed with:

```sh
$SHELL -lc '<command>'
```

falling back to `/bin/sh`

- command output is captured for the currently active display only
- stdout is rendered for successful commands
- failed commands render a diagnostic error slide
- no output cache
- no command timeout
- successful stderr is not shown

## App model

```rust
struct SlideCommand {
    file: PathBuf,
    line: usize,
    command: String,
}

struct ActiveSlide {
    index: usize,
    output: SlideOutput,
    scroll: u16,
}

struct SlideOutput {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
    duration: Duration,
}

struct App {
    commands: Vec<SlideCommand>,
    active: ActiveSlide,
    mode: Mode,
}

enum Mode {
    Presenting,
    ConfirmQuit,
}
```

No per-slide cache.

## Navigation behavior

Keys:

```txt
l / right-arrow / space = next slide
h / left-arrow          = previous slide
j / down-arrow          = scroll down
k / up-arrow            = scroll up
s                       = temporary shell
q                       = quit confirmation
Ctrl-C                  = immediate quit
```

When navigating:

1. update slide index
2. reset scroll to `0`
3. execute that slide command
4. store output in `active.output`
5. render

At bounds:

- next on final slide does nothing
- previous on first slide does nothing
- no re-execution if index does not change

## Rendering

### Successful command

Render stdout with ANSI support.

- ANSI colors/styles supported via `ansi-to-tui`
- lines wrap to terminal width
- vertical scrolling with `j/k` or up/down

### Failed command

Render generated diagnostic slide:

```txt
Command failed

File: slides/demo.txt
Line: 12
Exit: 1
Duration: 38ms

Command:
cargo test

STDOUT:
...

STDERR:
...
```

## Temporary shell

On `s`:

1. leave alternate screen
2. disable raw mode
3. spawn `$SHELL`
4. wait for exit
5. restore raw mode
6. re-enter alternate screen
7. redraw current slide

Returning from shell should **not automatically re-execute** the current slide. It returns to the same active output.

## Implementation phases

1. Create Rust binary crate.
2. Add deps:
   - `clap`
   - `ratatui`
   - `crossterm`
   - `anyhow`
   - `ansi-to-tui`
3. Implement CLI: `tuition <files>...`.
4. Implement parser:
   - ordered files
   - blank/comment skipping
   - source file + line metadata
5. Implement command runner:
   - `$SHELL -lc`
   - capture stdout/stderr/status
   - no timeout
6. Implement TUI setup/cleanup.
7. Implement event loop.
8. Implement rendering:
   - ANSI stdout slide
   - diagnostic error slide
   - footer
   - wrapping/scrolling
9. Implement temporary shell escape.
10. Add unit/manual tests.

## Final decision

Returning from temporary shell with `s` does **not** refresh/re-execute the current slide.
