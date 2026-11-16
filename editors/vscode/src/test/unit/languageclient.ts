// `vscode-languageclient/node` for unit tests: a client that serves `server`, and that, as
// the real one, cannot be stopped until it has started.
import { calls } from "./vscode";

/**
 * What the next client's server answers `initialize` with, once `starting` resolves; a
 * server that `fails` does not start.
 */
export const server = {
  capabilities: {} as Record<string, unknown>,
  starting: Promise.resolve(),
  fails: false,
};

export class LanguageClient {
  initializeResult?: { capabilities: Record<string, unknown> };
  constructor(
    readonly id: string,
    readonly name: string,
    readonly serverOptions: { command: string; args: string[] },
    readonly clientOptions: unknown,
  ) {}
  private state: "initial" | "starting" | "running" | "failed" = "initial";
  async start(): Promise<void> {
    calls.push(["client.start", this.serverOptions.command, ...this.serverOptions.args]);
    this.state = "starting";
    await server.starting;
    if (server.fails) {
      this.state = "failed";
      throw new Error("Server initialization failed.");
    }
    this.initializeResult = { capabilities: structuredClone(server.capabilities) };
    this.state = "running";
  }
  async dispose(): Promise<void> {
    calls.push(["client.dispose"]);
    if (this.state === "starting" || this.state === "failed") {
      throw new Error(
        `Client is not running and can't be stopped. It's current state is: ${this.state}`,
      );
    }
  }
}
