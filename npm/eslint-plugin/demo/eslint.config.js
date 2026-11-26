import parser from "@typescript-eslint/parser";
import sqlty from "@sqlty/eslint-plugin";

export default [
  {
    files: ["src/**/*.ts"],
    languageOptions: { parser },
    plugins: { sqlty },
    rules: {
      "sqlty/check-sql": [
        "error",
        { schema: "schema.sql", driver: "postgres", targets: [{ tag: "sql", transform: "{type}[]" }] },
      ],
    },
  },
];
