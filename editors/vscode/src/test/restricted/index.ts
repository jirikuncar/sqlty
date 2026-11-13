import Mocha from "mocha";

export function run(): Promise<void> {
  const mocha = new Mocha({ ui: "tdd", timeout: 60_000 });
  mocha.addFile(`${__dirname}/restricted.test.js`);
  return new Promise((resolve, reject) => {
    mocha.run((failures) =>
      failures > 0 ? reject(new Error(`${failures} tests failed.`)) : resolve(),
    );
  });
}
