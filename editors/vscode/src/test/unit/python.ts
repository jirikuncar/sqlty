// `@vscode/python-extension` for unit tests: the interpreter selected is `python.selected`.
import { event } from "./vscode";

export const python = { selected: undefined as string | undefined };

export const PythonExtension = {
  api: async () => ({
    environments: {
      onDidChangeActiveEnvironmentPath: event("python"),
      getActiveEnvironmentPath: () => ({ path: python.selected }),
      resolveEnvironment: async ({ path }: { path?: string }) =>
        path === undefined ? undefined : { executable: { uri: { fsPath: path } } },
    },
  }),
};
