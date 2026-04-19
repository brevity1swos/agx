# agx demo — step-through debugger for AI agent traces
# Prerequisites (run from the repo root):
#   cargo build --release
#   vhs assets/demo.tape

Output assets/demo.gif
Set Shell "bash"
Set FontSize 14
Set Width 1400
Set Height 800
Set Padding 10
Set TypingSpeed 50ms

# Put the local release binary at the front of PATH so the typed command
# below reads as plain `agx …`. Hidden so the export line doesn't appear
# in the GIF.
Hide
Type "export PATH=$PWD/target/release:$PATH"
Enter
Sleep 0.2s
Ctrl+L
Show

# Launch agx on the Claude Code sample session
Type "agx assets/sample_session.jsonl"
Enter
Sleep 1.5s

# Navigate down through steps
Type "jjjj"
Sleep 1s

# Jump to a specific step
Type ":8"
Enter
Sleep 1s

# Filter to tool_use steps only
Type "f"
Sleep 0.3s
Type "[tool]"
Enter
Sleep 1.5s

# Clear filter
Type "f"
Sleep 0.3s
Enter
Sleep 0.5s

# Search for "fib"
Type "/"
Sleep 0.3s
Type "fib"
Enter
Sleep 1s

# Navigate search matches
Type "n"
Sleep 0.5s
Type "n"
Sleep 1s

# Clear search
Type "/"
Sleep 0.3s
Enter
Sleep 0.5s

# Toggle heatmap mode
Type "h"
Sleep 1.5s
Type "h"
Sleep 0.5s

# Open stats overlay
Type "s"
Sleep 2s

# Dismiss stats
Type "q"
Sleep 0.3s

# Toggle to 2-pane layout
Tab
Sleep 1s

# Back to 3-pane
Tab
Sleep 1s

# Open help
Type "?"
Sleep 2s

# Dismiss and quit
Escape
Sleep 0.3s
Type "q"
Sleep 0.5s
