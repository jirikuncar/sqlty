import "mocha/mocha";

export function run(): Promise<void> {
  mocha.setup({ ui: "tdd", reporter: undefined, timeout: 60_000 });
  require("./web.test");
  return new Promise((resolve, reject) => {
    mocha.run((failures) =>
      failures > 0 ? reject(new Error(`${failures} tests failed.`)) : resolve(),
    );
  });
}
