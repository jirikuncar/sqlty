import * as assert from "node:assert/strict";
import * as vscode from "vscode";

suite("sqlty", () => {
  test("shows the diagnostics of sqlty check, and fixes them", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "q.py");
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);

    const [diagnostic] = await diagnostics(uri, (found) => found.length > 0);
    assert.equal(diagnostic.source, "sqlty");
    assert.equal(diagnostic.message, "SQL[tuple[int]] should be SQL[tuple[float | None]]");
    assert.deepEqual(diagnostic.range, new vscode.Range(2, 8, 2, 18));
    const code = diagnostic.code as { value: string; target: vscode.Uri };
    assert.equal(code.value, "narrow-type");
    assert.equal(
      code.target.toString(true),
      "https://jirikuncar.github.io/sqlty/reference/rules/#narrow-type",
    );

    const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      "vscode.executeCodeActionProvider",
      uri,
      diagnostic.range,
      "source.fixAll.sqlty",
    );
    const fixAll = actions.find((action) => action.kind?.value === "source.fixAll.sqlty");
    assert.ok(fixAll?.edit, "a fix-all action with an edit");
    assert.ok(await vscode.workspace.applyEdit(fixAll.edit));
    assert.match(document.getText(), /SQL\[tuple\[float \| None\]\]/);
    await diagnostics(uri, (found) => found.length === 0);
  });

  test("offers the closest column as the quick fix of unknown-column", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "typo.py");
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);

    const [diagnostic] = await diagnostics(uri, (found) => found.length > 0);
    assert.equal(
      diagnostic.message,
      "column `surname` does not exist in `users`; did you mean `name`?",
    );

    const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      "vscode.executeCodeActionProvider",
      uri,
      diagnostic.range,
      vscode.CodeActionKind.QuickFix.value,
    );
    const fix = actions.find((action) => action.title === "Fix unknown-column");
    assert.ok(fix?.edit, "a quick fix with an edit");
    assert.ok(await vscode.workspace.applyEdit(fix.edit));
    assert.match(document.getText(), /SELECT id, name FROM users/);
  });
});

/** The diagnostics of `uri` once `done` holds for them. */
async function diagnostics(
  uri: vscode.Uri,
  done: (found: vscode.Diagnostic[]) => boolean,
): Promise<vscode.Diagnostic[]> {
  for (let attempt = 0; attempt < 300; attempt++) {
    const found = vscode.languages.getDiagnostics(uri);
    if (done(found)) {
      return found;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(
    `diagnostics of ${uri.toString()}: ${JSON.stringify(vscode.languages.getDiagnostics(uri))}`,
  );
}
