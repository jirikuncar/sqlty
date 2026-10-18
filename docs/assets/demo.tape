# README demo. From the repository root, with sqlty, mypy and psycopg installed:
#   vhs docs/assets/demo.tape

Output docs/assets/demo.gif

Set Shell bash
Set Theme "Catppuccin Mocha"
Set FontSize 26
Set Width 1440
Set Height 640
Set Padding 30
Set TypingSpeed 40ms

Hide
Type `cd "$(mktemp -d)" && export PS1='$ '`
Enter
Type `printf '%s\n' '[tool.sqlty]' 'schema = "schema.sql"' > pyproject.toml`
Enter
Type `printf '%s\n' 'CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL);' > schema.sql`
Enter
Type `printf '%s\n' 'import psycopg' 'from sqlty.drivers.psycopg import execute' '' 'users = "--sql\nSELECT id, email FROM users"' '' '' 'def emails(conn: psycopg.Connection) -> list[str]:' '    return [email.lower() for _, email in execute(conn, users)]' > app.py`
Enter
Type "clear"
Enter
Show

Type "cat schema.sql app.py"
Enter
Sleep 3s

Hide
Type "clear"
Enter
Show

Type "sqlty rewrite --diff ."
Enter
Sleep 4s

Hide
Type "clear"
Enter
Show

Type "sqlty rewrite . && mypy app.py"
Enter
Sleep 2s

Type "# a migration makes email nullable"
Enter
Type "sed -i 's/ NOT NULL//' schema.sql"
Enter
Sleep 1s

Type "sqlty check ."
Enter
Sleep 2.5s

Type "sqlty check --fix . && mypy app.py"
Enter
Sleep 5s
