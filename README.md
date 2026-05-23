# tuition

Presentations in the terminal, driven by shell commands.

## Usage

```sh
tuition file1.txt file2.txt
```

Each non-blank line that does not start with `#` after trimming is executed as one slide command using `$SHELL -lc`, falling back to `/bin/sh`.

## Keys

- `l` / right arrow / space: next slide
- `h` / left arrow: previous slide
- `j` / down arrow: scroll down
- `k` / up arrow: scroll up
- `s`: temporary shell; return without re-running current slide
- `q`: quit confirmation
- Ctrl-C: quit immediately
