import * as vscode from "vscode";

suite("sqlty on the web", () => {
  test("loads, so VS Code for the Web gets the SQL grammars", async () => {
    const extension = vscode.extensions.getExtension("jirikuncar.sqlty");
    if (extension === undefined) {
      throw new Error("the web extension host skipped the extension");
    }
    await extension.activate();
    const grammars: { scopeName: string }[] = extension.packageJSON.contributes.grammars;
    const scopes = grammars.map((grammar) => grammar.scopeName).join(", ");
    if (scopes !== "sqlty.sql.injection, sqlty.sql.injection.multi") {
      throw new Error(`grammars: ${scopes}`);
    }
  });

  test("checks a buffer against the schema of its pyproject.toml", async () => {
    const [found] = await diagnostics("q.py");
    equal(found.message, "SQL[tuple[int]] should be SQL[tuple[float | None]]");
    equal((found.code as { value: string }).value, "narrow-type");
    equal(`${found.range.start.line}:${found.range.start.character}`, "2:8");
    const uri = file("q.py");
    const actions = await vscode.commands.executeCommand<vscode.CodeAction[]>(
      "vscode.executeCodeActionProvider",
      uri,
      found.range,
    );
    const fix = actions.find((action) => action.title === "Fix narrow-type");
    if (fix?.edit === undefined) {
      throw new Error(`no quick fix: ${actions.map((action) => action.title).join(", ")}`);
    }
    const [[, [edit]]] = fix.edit.entries();
    equal(edit.newText, "tuple[float | None]");
  });

  test("reads a nested config with extend, its schema relative to the file that sets it", async () => {
    const [found] = await diagnostics("mono/app/items.py");
    equal(found.message, "SQL[tuple[int]] should be SQL[tuple[float]]");
  });

  test("completes columns and hovers tables in a query string", async () => {
    const uri = file("complete.py");
    const document = await vscode.workspace.openTextDocument(uri);
    const position = document.positionAt(document.getText().indexOf("u.") + 2);
    const list = await vscode.commands.executeCommand<vscode.CompletionList>(
      "vscode.executeCompletionItemProvider",
      uri,
      position,
      ".",
    );
    const labels = list.items
      .filter((item) => item.sortText?.match(/^\d{4}$/))
      .map((item) => item.label);
    equal(labels.join(", "), "id, name, score");
    const at = document.positionAt(document.getText().indexOf("users") + 1);
    const [hover] = await vscode.commands.executeCommand<vscode.Hover[]>(
      "vscode.executeHoverProvider",
      uri,
      at,
    );
    const contents = hover.contents.map((part) => (part as vscode.MarkdownString).value);
    equal(contents[0].split("\n")[0], "Table `public.users`");
  });
});

function file(path: string): vscode.Uri {
  const [folder] = vscode.workspace.workspaceFolders ?? [];
  return vscode.Uri.joinPath(folder.uri, path);
}

async function diagnostics(path: string): Promise<vscode.Diagnostic[]> {
  const uri = file(path);
  await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(uri));
  for (let tries = 0; tries < 100; tries++) {
    const found = vscode.languages.getDiagnostics(uri);
    if (found.length > 0) {
      return found;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`no diagnostics for ${path}`);
}

function equal(actual: string, expected: string): void {
  if (actual !== expected) {
    throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}
