# tuition implementation notes

## Architecture

`tuition` is a command-driven terminal slide presenter. The current implementation intentionally runs slide commands directly attached to the user's terminal.

Do **not** introduce a ratatui layer, captured-output renderer, scrolling UI, or output cache unless explicitly requested later.

## Execution model

- Each slide is one shell command line from an input file.
- The active slide is run with:

  ```sh
  $SHELL -lc '<generated script>'
  ```

  falling back to `/bin/sh` when `$SHELL` is unset.
- The generated script prints the status bar, loads aliases where applicable, then executes the slide command.
- Commands run with their current directory set to the slide file's directory.
- Output is **not captured** by `tuition`; stdout, stderr, prompts, cursor control, and terminal protocols go directly to the terminal.
- Interactive commands and terminal image protocols are intentionally supported.
- There is no command timeout and no cached output.

## Slide parsing

For:

```sh
tuition file1.txt file2.txt
```

- Read input files in the order supplied.
- Ignore blank or whitespace-only lines.
- Ignore lines whose trimmed form starts with `#`.
- Preserve the original command line text for every slide command.
- Store the source file path and line number with each command.
- Inline `#` characters are part of the command; they are not treated as comments.

## Navigation

Keys:

```txt
l / right-arrow / space = next slide
h / left-arrow          = previous slide
r                       = rerun current slide
s                       = temporary shell
q                       = quit confirmation
Ctrl-C                  = immediate quit
```

There are no scrolling keys in the current terminal-attached implementation.

## Re-execution behavior

- Startup runs slide 1.
- Moving to a different slide runs that slide.
- Pressing `r` reruns the current slide.
- Attempting to move past the first or final slide does nothing and does not rerun anything.
- Returning from the temporary shell does not rerun the slide; it only redraws the status bar/navigation hints.

## Status bar

The status bar is printed before each slide command. It includes:

- current slide number
- total slide count
- source file
- source line
- key hints for rerun, shell, and quit

## Aliases

Aliases may be provided explicitly:

```sh
tuition --aliases './aliases;./more-aliases' slides.txt
```

- Empty `--aliases` segments are ignored.
- Relative alias paths are resolved relative to the process current directory.
- Absolute alias paths remain absolute.

If `--aliases` is not supplied, `tuition` searches upward from the slide command cwd for the nearest file named `aliases`.

When alias files are used:

- Bash gets `shopt -s expand_aliases`.
- Alias files are sourced before slide commands.
- Alias definitions are also converted into shell functions so they work in non-interactive slide command execution.
- Invalid alias names are ignored during function generation.

## Temporary shell

On `s`, `tuition` launches the user's shell with a custom `(TUITION)` prompt.

- Bash, zsh, and fish receive shell-specific prompt setup.
- Aliases are loaded where applicable.
- Exiting the temporary shell returns to `tuition` without rerunning the current slide.

## Implementation guidance

- Keep the current terminal-attached execution model.
- Do not add output caching.
- Do not capture slide output for rendering.
- Do not add ratatui or scrolling behavior unless explicitly requested later.
- Preserve current keybindings and temporary shell behavior.
