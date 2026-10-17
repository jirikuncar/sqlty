namespace SqlTy

/-- The dialects whose mapping differs: `generic` stands for any other. -/
inductive Dialect where
  | postgres | mysql | sqlite | clickhouse | duckdb | oracle | generic
  deriving DecidableEq, Repr

end SqlTy
