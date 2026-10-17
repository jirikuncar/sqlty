import SqlTy.Mapping

/-!
What holds of the mapping for every SQL type, nested containers included, and of the
scalar types each dialect knows.
-/

namespace SqlTy

/-- Every scalar type the Rust code maps has a well-formed Python type. -/
theorem scalarTable_wf : ∀ e ∈ scalarTable, ∀ p, e.2.2 = some p → WellFormed p := by
  have h : scalarTable.all (fun e => e.2.2.all PyType.wf) = true := by decide +kernel
  intro e he p hp
  have := List.all_eq_true.mp h e he
  rw [hp] at this
  exact PyType.wf_sound p (by simpa using this)

theorem affinity_wf {name : String} {p : PyType} (h : affinity name = some p) :
    WellFormed p := by
  have key : ∀ x, some (PyType.name x) = some p → x ∈ stdlibNames → WellFormed p := by
    intro x hx hm; cases hx; exact .name hm
  unfold affinity at h
  dsimp only at h
  by_cases h1 : (name.toUpper.isEmpty || name.toUpper == "ANY") = true
  · rw [ite_eq_left h1] at h; contradiction
  rw [ite_eq_right h1] at h
  by_cases h2 : hasPart name.toUpper "INT" = true
  · rw [ite_eq_left h2] at h; exact key _ h (by decide)
  rw [ite_eq_right h2] at h
  by_cases h3 : (hasPart name.toUpper "CHAR" || hasPart name.toUpper "CLOB" || hasPart name.toUpper "TEXT") = true
  · rw [ite_eq_left h3] at h; exact key _ h (by decide)
  rw [ite_eq_right h3] at h
  by_cases h4 : hasPart name.toUpper "BLOB" = true
  · rw [ite_eq_left h4] at h; exact key _ h (by decide)
  rw [ite_eq_right h4] at h; exact key _ h (by decide)

theorem scalar_wf {d : Dialect} {n : String} {p : PyType} (h : scalar d n = some p) :
    WellFormed p := by
  unfold scalar at h
  split at h
  · rename_i e he
    exact scalarTable_wf _ (List.mem_of_find?_eq_some he) p h
  · split at h
    · exact affinity_wf h
    · contradiction

theorem any_wf : WellFormed any := .name (by decide)

theorem getD_wf {o : Option PyType} (h : ∀ p, o = some p → WellFormed p) :
    WellFormed (o.getD any) := by
  cases o with
  | none => exact any_wf
  | some p => exact h p rfl

theorem members_wf {p : PyType} (h : WellFormed p) :
    ∀ m ∈ members p, WellFormed m ∧ m.isUnion = false := by
  cases h with
  | union _ hw hu _ => intro m hm; exact ⟨hw m hm, hu m hm⟩
  | name hn => intro m hm; simp [members] at hm; subst hm; exact ⟨.name hn, rfl⟩
  | bareList => intro m hm; simp [members] at hm; subst hm; exact ⟨.bareList, rfl⟩
  | list h => intro m hm; simp [members] at hm; subst hm; exact ⟨.list h, rfl⟩
  | tuple hne h => intro m hm; simp [members] at hm; subst hm; exact ⟨.tuple hne h, rfl⟩
  | tupleOf h => intro m hm; simp [members] at hm; subst hm; exact ⟨.tupleOf h, rfl⟩
  | dict hk hv => intro m hm; simp [members] at hm; subst hm; exact ⟨.dict hk hv, rfl⟩

theorem foldl_insertMember {Q : PyType → Prop} :
    ∀ (l acc : List PyType), (acc.map PyType.render).Nodup → (∀ x ∈ acc, Q x) →
      (∀ x ∈ l, Q x) →
      ((l.foldl insertMember acc).map PyType.render).Nodup ∧
        ∀ x ∈ l.foldl insertMember acc, Q x
  | [], acc, hd, ha, _ => ⟨hd, ha⟩
  | m :: l, acc, hd, ha, hl => by
    simp only [List.foldl_cons]
    apply foldl_insertMember l
    · unfold insertMember
      split
      · exact hd
      · rename_i hc
        simp only [List.map_append, List.map_cons, List.map_nil]
        refine List.nodup_append.mpr ⟨hd, by simp, ?_⟩
        intro a ha' b hb
        simp at hb; subst hb
        intro heq; subst heq
        exact hc (by simpa using ha')
    · intro x hx
      unfold insertMember at hx
      split at hx
      · exact ha x hx
      · simp at hx
        rcases hx with hx | rfl
        · exact ha x hx
        · exact hl _ (by simp)
    · intro x hx; exact hl x (by simp [hx])

theorem unionOf_wf {parts : List PyType} {u : PyType} (hp : ∀ p ∈ parts, WellFormed p)
    (h : unionOf parts = some u) : WellFormed u := by
  have hq : ∀ x ∈ parts.flatMap members, WellFormed x ∧ x.isUnion = false := by
    intro x hx
    simp only [List.mem_flatMap] at hx
    obtain ⟨p, hp', hx⟩ := hx
    exact members_wf (hp p hp') x hx
  obtain ⟨hd, hall⟩ := foldl_insertMember (Q := fun x => WellFormed x ∧ x.isUnion = false)
    (parts.flatMap members) [] (by simp) (by simp) hq
  unfold unionOf at h
  split at h
  · contradiction
  · rename_i m heq
    simp at h; subst h
    exact (hall _ (by rw [heq]; simp)).1
  · rename_i m₁ m₂ ms heq
    simp at h; subst h
    rw [heq] at hd hall
    exact .union (by simp) (fun x hx => (hall x hx).1) (fun x hx => (hall x hx).2) hd

theorem mapM_some {α β : Type} {f : α → Option β} :
    ∀ {l : List α} {ms : List β}, l.mapM f = some ms → ∀ m ∈ ms, ∃ x ∈ l, f x = some m
  | [], ms, h => by simp at h; subst h; simp
  | x :: l, ms, h => by
    simp only [List.mapM_cons, Option.bind_eq_bind, Option.pure_def] at h
    cases hx : f x with
    | none => simp [hx] at h
    | some y =>
      cases hl : l.mapM f with
      | none => simp [hx, hl] at h
      | some ys =>
        simp [hx, hl] at h; subst h
        intro m hm
        simp at hm
        rcases hm with rfl | hm
        · exact ⟨x, by simp, hx⟩
        · obtain ⟨z, hz, hfz⟩ := mapM_some hl m hm
          exact ⟨z, by simp [hz], hfz⟩

/-- The mapping only ever gives well-formed Python types: whatever the nesting of arrays,
rows, structs, maps and unions, and in every dialect. -/
theorem python_wf (d : Dialect) : ∀ (t : SqlType) (p : PyType), python d t = some p →
    WellFormed p
  | .scalar n, p, h => by simp only [python] at h; exact scalar_wf h
  | .array none fixed, p, h => by
    simp only [python, Option.some.injEq] at h; subst h
    split
    · exact .tupleOf any_wf
    · exact .bareList
  | .array (some e) fixed, p, h => by
    simp only [python] at h
    split at h
    · simp at h; subst h
      exact .tupleOf (getD_wf (python_wf d e))
    · simp at h; subst h
      split
      · rename_i item hi
        exact .list (python_wf d e item hi)
      · exact .bareList
  | .row [], p, h => by simp [python] at h
  | .row (f :: fs), p, h => by
    simp only [python, Option.some.injEq] at h; subst h
    refine .tuple (by simp) ?_
    intro t ht
    simp only [List.mem_map] at ht
    obtain ⟨g, _, rfl⟩ := ht
    exact getD_wf (python_wf d g.1)
  | .struct fs, p, h => by
    simp only [python, Option.map_eq_some_iff] at h
    obtain ⟨u, hu, rfl⟩ := h
    refine .dict (.name (by decide)) (unionOf_wf ?_ hu)
    intro t ht
    simp only [List.mem_map] at ht
    obtain ⟨g, _, rfl⟩ := ht
    exact getD_wf (python_wf d g.1)
  | .map k v, p, h => by
    simp only [python, Option.map_eq_some_iff] at h
    obtain ⟨key, hk, rfl⟩ := h
    exact .dict (python_wf d k key hk) (getD_wf (python_wf d v))
  | .union fs, p, h => by
    simp only [python, Option.bind_eq_some_iff] at h
    obtain ⟨ms, hms, hu⟩ := h
    refine unionOf_wf ?_ hu
    intro m hm
    obtain ⟨g, _, hg⟩ := mapM_some hms m hm
    exact python_wf d g.1 m hg
  | .interval, p, h => by simp [python] at h; subst h; exact .name (by decide)
  | .unmapped, p, h => by simp [python] at h
termination_by t _ _ => sizeOf t
decreasing_by
  all_goals simp_wf
  all_goals first
    | omega
    | (have := List.sizeOf_lt_of_mem g.2; simp at this ⊢; omega)

/-! ## What each dialect loads

Stated on the spelling, which is what sqlty writes in a row type.
-/

/-- Integer types load as `int` in every dialect: Python's `int` has no bound, so no value
is lost. -/
theorem integers_are_int : ∀ d, ∀ n ∈ ["SMALLINT", "INT", "BIGINT", "TINYINT", "MEDIUMINT",
    "SERIAL", "BIGSERIAL", "UBIGINT", "INT128", "UINT256"], (scalar d n).map PyType.render = some "int" := by
  intro d; cases d <;> decide +kernel

/-- Exact decimals load as `decimal.Decimal`, keeping their precision, in every dialect
whose driver returns one: SQLite's and Oracle's return floats. -/
theorem decimals_keep_precision : ∀ d, d ≠ .sqlite → d ≠ .oracle →
    ∀ n ∈ ["DECIMAL", "UDECIMAL", "BIGDECIMAL", "DECFLOAT"],
      (scalar d n).map PyType.render = some "decimal.Decimal" := by
  intro d; cases d <;> decide +kernel

/-- Dates and timestamps load as `datetime` values, except in SQLite, which has no date
storage class: `sqlite3` returns the stored text. -/
theorem dates_are_datetime : ∀ d, d ≠ .sqlite →
    (scalar d "DATE").map PyType.render = some "datetime.date" ∧
      (scalar d "TIMESTAMP").map PyType.render = some "datetime.datetime" ∧
      (scalar d "TIMESTAMPTZ").map PyType.render = some "datetime.datetime" := by
  intro d; cases d <;> decide +kernel

theorem sqlite_dates_are_text : ∀ n ∈ ["DATE", "TIME", "TIMESTAMP", "DATETIME"],
    (scalar .sqlite n).map PyType.render = some "str" := by
  decide +kernel

/-- A MySQL `TIME` is a duration, up to 838 hours either way, so it loads as a
`datetime.timedelta`, as an interval does in every dialect. -/
theorem durations_are_timedelta : ∀ d,
    (python d .interval).map PyType.render = some "datetime.timedelta" ∧
      (scalar .mysql "TIME").map PyType.render = some "datetime.timedelta" := by
  intro d; exact ⟨by simp [python, PyType.render], by decide +kernel⟩

end SqlTy
