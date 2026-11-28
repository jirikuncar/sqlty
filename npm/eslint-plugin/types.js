// What each driver gives JavaScript for a PostgreSQL type, by the canonical spelling sqlty
// reports as `sql_type` (`INT`, `ARRAY<BIGINT>`). A type missing here arrives as the text
// PostgreSQL sends: a `string`.

const INTERVAL = 'import("postgres-interval").IPostgresInterval';

// node-postgres 8 parses with pg-types 2.2: lib/textParsers.js registers these OIDs.
// int8 and numeric stay strings, interval becomes an object, and only the arrays it
// registers are parsed (numeric[] into numbers); any other array is its literal text.
const pg = {
  scalars: {
    SMALLINT: "number",
    INT: "number",
    INTEGER: "number",
    SERIAL: "number",
    SMALLSERIAL: "number",
    OID: "number",
    REAL: "number",
    FLOAT: "number",
    DOUBLE: "number",
    BOOLEAN: "boolean",
    BOOL: "boolean",
    DATE: "Date",
    TIMESTAMP: "Date",
    TIMESTAMPTZ: "Date",
    INTERVAL,
    BYTEA: "Buffer",
    VARBINARY: "Buffer",
    JSON: "unknown",
    JSONB: "unknown",
  },
  arrays: {
    BOOLEAN: "boolean",
    BOOL: "boolean",
    BYTEA: "Buffer",
    VARBINARY: "Buffer",
    SMALLINT: "number",
    INT: "number",
    INTEGER: "number",
    OID: "number",
    BIGINT: "string",
    REAL: "number",
    FLOAT: "number",
    DOUBLE: "number",
    NUMERIC: "number",
    DECIMAL: "number",
    CHAR: "string",
    BPCHAR: "string",
    VARCHAR: "string",
    TEXT: "string",
    MACADDR: "string",
    INET: "string",
    CIDR: "string",
    UUID: "string",
    MONEY: "string",
    TIME: "string",
    TIMETZ: "string",
    NUMRANGE: "string",
    DATE: "Date",
    TIMESTAMP: "Date",
    TIMESTAMPTZ: "Date",
    INTERVAL,
    JSON: "unknown",
    JSONB: "unknown",
  },
};

// postgres.js 3.4 parses with src/types.js: int2, int4, oid, float4 and float8 as numbers,
// json and jsonb, bool, date and both timestamps as Date, bytea; every array of those, as it
// fetches array types on connect. int8, numeric and interval stay strings.
const scalars = {
  SMALLINT: "number",
  INT: "number",
  INTEGER: "number",
  SERIAL: "number",
  SMALLSERIAL: "number",
  OID: "number",
  REAL: "number",
  FLOAT: "number",
  DOUBLE: "number",
  BOOLEAN: "boolean",
  BOOL: "boolean",
  DATE: "Date",
  TIMESTAMP: "Date",
  TIMESTAMPTZ: "Date",
  BYTEA: "Buffer",
  VARBINARY: "Buffer",
  JSON: "unknown",
  JSONB: "unknown",
};
const postgres = { scalars, arrays: scalars, everyArray: true };

export const drivers = { pg, postgres };

/** The TypeScript type `driver` gives a value of `sqlType`, `overrides` (by SQL type) first. */
export function tsType(sqlType, driver, overrides = {}) {
  if (sqlType == null) return "unknown";
  const name = sqlType.replace(/\(.*\)$/, "").trim();
  if (name in overrides) return overrides[name];
  const element = /^ARRAY<(.*)>$/.exec(name)?.[1];
  if (element === undefined) return driver.scalars[name] ?? "string";
  const inner = element.replace(/\(.*\)$/, "").trim();
  if (/^ARRAY</.test(inner)) return `${tsType(inner, driver, overrides)}[]`;
  if (inner in overrides) return `${wrap(overrides[inner])}[]`;
  if (inner in driver.arrays) return `${wrap(driver.arrays[inner])}[]`;
  return driver.everyArray ? "string[]" : "string";
}

const wrap = (type) => (/[|&]/.test(type) ? `(${type})` : type);
