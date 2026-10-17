import Lean.Data.Json
import SqlTy

/-!
`sqlty-model`: the model's Python type for each SQL type on standard input, one JSON
object per line, `{"dialect": "duckdb", "type": T}`, where `T` is `{"scalar": "INT"}`,
`{"array": T or null, "fixed": bool}`, `{"row": [T]}`, `{"struct": [T]}`, `{"map": [T, T]}`,
`{"union": [T]}`, `"interval"` or `"unmapped"`. Writes the spelling as a JSON string, or
`null`, per line. `tests/test_lean_model.py` compares it with sqlty.
-/

open Lean SqlTy

def dialectOf : String → Dialect
  | "postgres" => .postgres
  | "mysql" => .mysql
  | "sqlite" => .sqlite
  | "clickhouse" => .clickhouse
  | "duckdb" => .duckdb
  | "oracle" => .oracle
  | _ => .generic

partial def sqlTypeOf (j : Json) : Except String SqlType := do
  if let .str s := j then
    return match s with
      | "interval" => .interval
      | _ => .unmapped
  if let .ok (name : String) := j.getObjValAs? String "scalar" then
    return .scalar name
  if let .ok element := j.getObjVal? "array" then
    let fixed ← j.getObjValAs? Bool "fixed"
    let element ← if element.isNull then pure none else some <$> sqlTypeOf element
    return .array element fixed
  let fields (key : String) : Except String (List SqlType) := do
    let items ← j.getObjValAs? (Array Json) key
    items.toList.mapM sqlTypeOf
  if let .ok fs := fields "row" then return .row fs
  if let .ok fs := fields "struct" then return .struct fs
  if let .ok fs := fields "union" then return .union fs
  if let .ok [k, v] := fields "map" then return .map k v
  throw s!"not a SQL type: {j.compress}"

def answer (line : String) : Except String Json := do
  let j ← Json.parse line
  let d ← j.getObjValAs? String "dialect"
  let t ← sqlTypeOf (← j.getObjVal? "type")
  return match python (dialectOf d) t with
    | some p => .str p.render
    | none => .null

partial def loop (stdin : IO.FS.Stream) (stdout : IO.FS.Stream) : IO Unit := do
  let line ← stdin.getLine
  if line.isEmpty then return
  match answer line.trimAsciiEnd.toString with
  | .ok j => stdout.putStrLn j.compress; stdout.flush
  | .error e => throw (IO.userError e)
  loop stdin stdout

def main : IO Unit := do
  loop (← IO.getStdin) (← IO.getStdout)
