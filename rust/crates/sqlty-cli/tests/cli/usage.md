# Version and usage errors

```console
$ sqlty --version
sqlty [VERSION]

$ sqlty version
sqlty [VERSION]

$ sqlty lint
? 2
error: unrecognized subcommand 'lint'

Usage: sqlty [COMMAND]

For more information, try '--help'.

$ sqlty check --output-format xml
? 2
error: invalid value 'xml' for '--output-format <OUTPUT_FORMAT>'
  [possible values: text, json, github, sarif]

For more information, try '--help'.

```
