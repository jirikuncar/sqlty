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
});
