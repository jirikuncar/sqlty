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
});
