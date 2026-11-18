# SARIF

```console
$ sqlty check --output-format sarif
? 1
{
  "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
  "version": "2.1.0",
  "runs": [
    {
      "tool": {
        "driver": {
          "name": "sqlty",
          "version": "[VERSION]",
          "informationUri": "https://sqlty.kuncar.dev/",
          "rules": [
...
      "columnKind": "unicodeCodePoints",
      "results": [
        {
          "ruleId": "narrow-type",
          "level": "error",
          "message": {
            "text": "SQL[tuple[str]] should be SQL[tuple[int]]"
          },
          "locations": [
            {
              "physicalLocation": {
                "artifactLocation": {
                  "uri": "q.py"
                },
                "region": {
                  "startLine": 3,
                  "startColumn": 9,
                  "endLine": 3,
                  "endColumn": 19
                }
              }
            }
          ],
          "fixes": [
            {
              "artifactChanges": [
                {
                  "artifactLocation": {
                    "uri": "q.py"
                  },
                  "replacements": [
                    {
                      "deletedRegion": {
                        "startLine": 3,
                        "startColumn": 9,
                        "endLine": 3,
                        "endColumn": 19
                      },
                      "insertedContent": {
                        "text": "tuple[int]"
                      }
                    }
                  ]
                }
              ]
            }
          ]
        }
      ]
    }
  ]
}

```
