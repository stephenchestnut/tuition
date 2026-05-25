---
name: tuition
description: Use this skill when creating a presentation for `tuition`, a terminal slide presenter.
---

# Tuition presentation skill

Use this skill when creating a presentation for `tuition`, a terminal slide presenter.

## What to create

A tuition presentation is a plain text file. Each slide is one line in that text file:

- Every non-blank, non-comment line is executed as a bash/shell command.
- Blank lines are ignored.
- Lines whose trimmed form starts with `#` are comments and are ignored.
- Inline `#` characters are part of the command, not comments.
- The file should be clearly organized and readable by a human. Use comments and blank lines to group sections.

Because each slide is a shell command, use commands such as `printf`, `cat`, `clear`, `figlet`, `gum`, `bat`, `python`, `node`, `vim`, image-display commands, or project-specific tools to present content.

## Style guidance

- Prefer readable one-line commands for each slide.
- Join multiple commands on one-line with `;` or `&&`.
- Use comments to label sections of the presentation.
- Use the defined color environment variables from the `aliases` file when adding color.
- Reset formatting with `$RESET` or `$RESETFMT` after colored text.
- You may create new aliases if they make the presentation easier to author, but aliases are optional.
- Keep presentations useful as text files: someone should be able to open the `.txt` presentation and understand the sequence of slides.

## Example presentation: `hello_world.txt`

```txt
# A short tuition hello world presentation
printf '\033[1;36mHello, world!\033[0m\n\nWelcome to tuition.\n'
printf 'Each non-comment line is a shell command.\n\nThis is slide 2.\n'
printf 'Use\n - l/space/right for next,\n - h/left for previous,\n - up/down to scroll when the slide is long,\n - r to rerun the command on a slide,\n - s to drop into the shell temporarily,\n - q to quit.\n'
printf 'You can display images using `imgcat` on mac or `img2sixel` on linux.\n'; imgcat -W 500px -s ../tuition_icon_128_easel.png; printf "\nIf you can't see the tuition logo above, something is wrong.\n"
printf 'Some commands may reference files.\n\nThe paths can be absolute or relative to the slides txt.'
printf 'Slides can run arbitrary programs!\n\n You can advance the slide when the program exits.\n\n....You do remember how to quit vim, right?'
vim
printf '\033[1;32mDone!\033[0m\nThanks for trying tuition.\n'
```

## Color exports from `aliases`

Source the aliases file when running tuition, or copy these exports into a custom aliases file. Use these variables in slide commands, for example:

```sh
printf "${BOLD}${FGCYAN}Title${RESET}\n\nBody text\n"
```

```sh
# Reset everything
export RESET="\033[0m"

# --- Standard foreground colors ---
export FGBLACK="\033[30m"   # Black
export FGRED="\033[31m"     # Red
export FGGREEN="\033[32m"   # Green
export FGYELLOW="\033[33m"  # Yellow
export FGBLUE="\033[34m"    # Blue
export FGMAGENTA="\033[35m" # Magenta
export FGCYAN="\033[36m"    # Cyan
export FGWHITE="\033[37m"   # White

# --- Bright foreground colors ---
export FGGRAY="\033[90m"         # Bright Black (Gray)
export FGBRED="\033[91m"   # Bright Red
export FGBGREEN="\033[92m" # Bright Green
export FGBYELLOW="\033[93m" # Bright Yellow
export FGBBLUE="\033[94m"  # Bright Blue
export FGBMAGENTA="\033[95m" # Bright Magenta
export FGBCYAN="\033[96m"  # Bright Cyan
export FGBWHITE="\033[97m" # Bright White

# --- Background colors (sets background, some also set fg) ---
export BGBLACK="\033[40m"        # Black bg
export BGRED="\033[31;41m"      # Red fg/bg
export BGGREEN="\033[32;42m"    # Green fg/bg
export BGYELLOW="\033[33;43m"   # Yellow fg/bg
export BGBLUE="\033[34;44m"     # Blue fg/bg
export BGMAGENTA="\033[35;45m"  # Magenta fg/bg
export BGCYAN="\033[36;46m"     # Cyan fg/bg
export BGWHITE="\033[37;47m"    # White fg/bg

# --- Bright background colors ----
export BGBBLACK="\033[30;100m"         # Bright Black bg
export BGBRED="\033[31;101m"          # Bright Red bg
export BGBGREEN="\033[32;102m"       # Bright Green bg
export BGBYELLOW="\033[33;103m"      # Bright Yellow bg
export BGBBLUE="\033[34;104m"       # Bright Blue bg
export BGBMAGENTA="\033[35;105m"    # Bright Magenta bg
export BGBCYAN="\033[36;106m"       # Bright Cyan bg
export BGBWHITE="\033[37;107m"      # Bright White bg

# --- Highlight/status aliases for emphasis ---
export HIGHLIGHT="\033[38;5;226;48;5;160m"  # White on bright red
export SUCCESS="\033[38;5;231;48;5;46m"    # White on bright green
export WARNING="\033[38;5;16;48;5;226m"    # Dark on yellow
export INFO="\033[38;5;231;48;5;21m"       # White on bright blue

# --- Style modifiers ---
export BOLD="\033[1m"
export DIM="\033[2m"
export ITALIC="\033[3m"
export UNDERLINE="\033[4m"
export BLINK="\033[5m"
export STRIKETHROUGH="\033[9m"
export REVERSE="\033[7m"
export HIDDEN="\033[8m"
export BOLDOFF="\033[21m"
export ITALICOFF="\033[23m"
export UNDERLINEOFF="\033[24m"
export REVERSEOFF="\033[27m"
export HIDDENOFF="\033[28m"
export STRIKETHROUGHOFF="\033[29m"

# Reset everything
export RESETFMT="\033[0m"
```
