import * as assert from "node:assert/strict";
import * as vscode from "vscode";

suite("sqlty in Restricted Mode", () => {
  test("checks a buffer with the bundled engine, without sqlty server", async () => {
    assert.equal(vscode.workspace.isTrusted, false);
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "q.py");
    await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(uri));
    for (let tries = 0; tries < 100; tries++) {
      const [found] = vscode.languages.getDiagnostics(uri);
      if (found !== undefined) {
        assert.equal(found.message, "SQL[tuple[int]] should be SQL[tuple[float | None]]");
        assert.equal((found.code as { value: string }).value, "narrow-type");
        return;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    assert.fail("no diagnostics for q.py");
  });

  test("wraps and formats a file with the bundled engine", async () => {
    const [folder] = vscode.workspace.workspaceFolders ?? [];
    const uri = vscode.Uri.joinPath(folder.uri, "wrap.py");
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);
    await vscode.commands.executeCommand("sqlty.rewrite");
    assert.match(document.getText(), /SQL\[tuple\[int \| None\]\]\(/);
    await vscode.commands.executeCommand("sqlty.strip");
    assert.equal(document.getText(), 'q = "--sql\\nSELECT id FROM users"\n');
    await vscode.commands.executeCommand("workbench.action.files.revert");
  });
});
