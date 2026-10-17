import SqlTy.Dialect
import SqlTy.Python
import SqlTy.Table

/-!
The model of `SqlType::python` in rust/crates/sqlty-sql/src/analyzer/sql_type.rs: the
Python type the dialect's driver loads a SQL type's values as. Scalar types come from
`scalarTable`, generated from the Rust code; containers and SQLite's type affinity are
written here, after the Rust code, and `tests/test_lean_model.py` compares the two.
-/

namespace SqlTy

/-- A SQL type, with the structure of its containers; field names do not change the
Python type, so they are left out. -/
inductive SqlType where
  /-- A scalar type by its canonical name, without parameters: `DECIMAL`. -/
  | scalar (name : String)
  /-- `T[]`; `fixed` for a size, `T[3]`. A bare `ARRAY` has no element. -/
  | array (element : Option SqlType) (fixed : Bool)
  | row (fields : List SqlType)
  | struct (fields : List SqlType)
  | map (key value : SqlType)
  | union (fields : List SqlType)
  | interval
  /-- A type sqlparser reads with no Python type: a table type, ... -/
  | unmapped
  deriving Repr

def any : PyType := .name "typing.Any"

/-- A union's members, or the type itself. -/
def members : PyType → List PyType
  | .union ms => ms
  | t => [t]

/-- `acc` with `m` added, unless a member is spelled the same. -/
def insertMember (acc : List PyType) (m : PyType) : List PyType :=
  if (acc.map PyType.render).contains m.render then acc else acc ++ [m]

/-- `parts` joined as a union, each member once, in order (`union` in the Rust code). -/
def unionOf (parts : List PyType) : Option PyType :=
  match (parts.flatMap members).foldl insertMember [] with
  | [] => none
  | [m] => some m
  | m₁ :: m₂ :: ms => some (.union (m₁ :: m₂ :: ms))

def hasPart (s part : String) : Bool := (s.splitOn part).length ≠ 1

/-- SQLite's type affinity for a name it has no mapping for
(https://www.sqlite.org/datatype3.html#determination_of_column_affinity). -/
def affinity (name : String) : Option PyType :=
  let name := name.toUpper
  if name.isEmpty || name == "ANY" then none
  else if hasPart name "INT" then some (.name "int")
  else if hasPart name "CHAR" || hasPart name "CLOB" || hasPart name "TEXT" then some (.name "str")
  else if hasPart name "BLOB" then some (.name "bytes")
  else some (.name "float")

/-- The Python type of a scalar type: the generated table, then SQLite's affinity. -/
def scalar (d : Dialect) (name : String) : Option PyType :=
  match scalarTable.find? (fun e => e.1 == d && e.2.1 == name) with
  | some (_, _, python) => python
  | none => if d = .sqlite then affinity name else none

/-- The Python type the dialect's driver loads values of `t` as; none when sqlty has no
Python type for it. -/
def python (d : Dialect) : SqlType → Option PyType
  | .scalar n => scalar d n
  | .array none fixed => some (if fixed && d == .duckdb then .tupleOf any else .bareList)
  | .array (some e) fixed =>
    if fixed && d == .duckdb then some (.tupleOf ((python d e).getD any))
    else some (match python d e with
      | some item => .list item
      | none => .bareList)
  | .row [] => none
  | .row (f :: fs) => some (.tuple ((f :: fs).attach.map fun g => (python d g.1).getD any))
  | .struct fs =>
    (unionOf (fs.attach.map fun g => (python d g.1).getD any)).map (.dict (.name "str"))
  | .map k v => (python d k).map fun key => .dict key ((python d v).getD any)
  | .union fs => (fs.attach.mapM fun g => python d g.1).bind unionOf
  | .interval => some (.name "datetime.timedelta")
  | .unmapped => none
decreasing_by
  all_goals simp_wf
  all_goals first
    | omega
    | (have := List.sizeOf_lt_of_mem g.2; simp at this ⊢; omega)

end SqlTy
