// `vscode-languageclient/node` for unit tests: a client that starts at once and serves `server`.
import { calls } from "./vscode";

/** What the next client's server answers `initialize` with. */
export const server = { capabilities: {} as Record<string, unknown> };

export class LanguageClient {
  initializeResult?: typeof server;
  constructor(
    readonly id: string,
    readonly name: string,
    readonly serverOptions: { command: string; args: string[] },
    readonly clientOptions: unknown,
  ) {}
  async start(): Promise<void> {
    calls.push(["client.start", this.serverOptions.command, ...this.serverOptions.args]);
    this.initializeResult = structuredClone(server);
  }
  async dispose(): Promise<void> {
    calls.push(["client.dispose"]);
  }
}
