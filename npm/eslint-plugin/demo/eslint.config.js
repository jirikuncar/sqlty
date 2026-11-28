import parser from "@typescript-eslint/parser";
import sqlty from "@sqlty/eslint-plugin";

const check = ["error", { schema: "schema.sql", driver: "postgres", targets: [{ tag: "sql", transform: "{type}[]" }] }];

export default [
  { files: ["src/**/*.js"], plugins: { sqlty }, rules: { "sqlty/check-sql": check } },
  { files: ["src/**/*.ts"], languageOptions: { parser }, plugins: { sqlty }, rules: { "sqlty/check-sql": check } },
];
