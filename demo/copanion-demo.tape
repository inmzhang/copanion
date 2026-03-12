Output demo/copanion-demo.gif

Set Shell "bash"
Set Theme "GruvboxDark"
Set FontSize 26
Set Width 1500
Set Height 960
Set Padding 24
Set WindowBar Rings
Set TypingSpeed 20ms
Set PlaybackSpeed 1.1

Hide
Type "cd /home/inm/open-source-project/copanion" Enter
Type "export XDG_DATA_HOME=$(mktemp -d)" Enter
Show

Type "./target/release/copanion --stdout" Enter
Sleep 1s

Type "src/tui/app.rs"
Sleep 300ms
Enter
Sleep 1s

Down 12
Sleep 300ms
Type "v"
Sleep 300ms
Down 2
Sleep 300ms
Type "n"
Sleep 500ms
Type "This block defines the main copanion app state and navigation helpers."
Sleep 300ms
Ctrl+S
Sleep 1s

Down 8
Sleep 300ms
Type "a"
Sleep 500ms
Type "Why does the app keep both packet data and loaded file state?"
Sleep 300ms
Ctrl+S
Sleep 1s

Type "g"
Sleep 300ms
Type "]"
Sleep 600ms
Type "]"
Sleep 900ms

Type "x"
Sleep 3s
