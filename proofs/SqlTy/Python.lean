namespace SqlTy

/-- A Python type expression, as sqlty spells one. -/
inductive PyType where
  /-- A class: `int`, `datetime.date`. -/
  | name (n : String)
  /-- `list`, of unknown items. -/
  | bareList
  | list (item : PyType)
  /-- `tuple[a, b]`. -/
  | tuple (items : List PyType)
  /-- `tuple[a, ...]`. -/
  | tupleOf (item : PyType)
  | dict (key value : PyType)
  /-- `a | b`. -/
  | union (members : List PyType)
  deriving Repr

/-- The classes a mapped type may name: each importable from the standard library. -/
def stdlibNames : List String :=
  [ "int", "float", "str", "bytes", "bool", "decimal.Decimal",
    "datetime.date", "datetime.time", "datetime.datetime", "datetime.timedelta",
    "uuid.UUID", "ipaddress.IPv4Address", "ipaddress.IPv6Address",
    "ipaddress.IPv4Interface", "ipaddress.IPv6Interface",
    "ipaddress.IPv4Network", "ipaddress.IPv6Network", "typing.Any" ]

def PyType.isUnion : PyType → Bool
  | .union _ => true
  | _ => false

mutual
/-- The spelling sqlty writes. -/
def PyType.render : PyType → String
  | .name n => n
  | .bareList => "list"
  | .list t => "list[" ++ t.render ++ "]"
  | .tuple ts => "tuple[" ++ ", ".intercalate (PyType.renderAll ts) ++ "]"
  | .tupleOf t => "tuple[" ++ t.render ++ ", ...]"
  | .dict k v => "dict[" ++ k.render ++ ", " ++ v.render ++ "]"
  | .union ms => " | ".intercalate (PyType.renderAll ms)

def PyType.renderAll : List PyType → List String
  | [] => []
  | t :: ts => t.render :: PyType.renderAll ts
end

theorem PyType.renderAll_eq : ∀ ts : List PyType, PyType.renderAll ts = ts.map PyType.render
  | [] => rfl
  | t :: ts => by simp [PyType.renderAll, renderAll_eq ts]

/-- A type expression Python accepts and a type checker reads as intended: stdlib classes
only, no empty `tuple[]`, and a union of at least two members spelled differently, none
a union itself. -/
inductive WellFormed : PyType → Prop
  | name {n} : n ∈ stdlibNames → WellFormed (.name n)
  | bareList : WellFormed .bareList
  | list {t} : WellFormed t → WellFormed (.list t)
  | tuple {ts} : ts ≠ [] → (∀ t ∈ ts, WellFormed t) → WellFormed (.tuple ts)
  | tupleOf {t} : WellFormed t → WellFormed (.tupleOf t)
  | dict {k v} : WellFormed k → WellFormed v → WellFormed (.dict k v)
  | union {ms} : 2 ≤ ms.length → (∀ m ∈ ms, WellFormed m) → (∀ m ∈ ms, m.isUnion = false) →
      (ms.map PyType.render).Nodup → WellFormed (.union ms)

mutual
/-- `WellFormed`, decided: what the generated scalar table is checked with. -/
def PyType.wf : PyType → Bool
  | .name n => stdlibNames.contains n
  | .bareList => true
  | .list t => t.wf
  | .tuple ts => !ts.isEmpty && PyType.wfAll ts
  | .tupleOf t => t.wf
  | .dict k v => k.wf && v.wf
  | .union ms => decide (2 ≤ ms.length) && PyType.wfMembers ms && decide (PyType.renderAll ms).Nodup

def PyType.wfAll : List PyType → Bool
  | [] => true
  | t :: ts => t.wf && PyType.wfAll ts

def PyType.wfMembers : List PyType → Bool
  | [] => true
  | t :: ts => t.wf && !t.isUnion && PyType.wfMembers ts
end

mutual
theorem PyType.wf_sound : ∀ t : PyType, t.wf = true → WellFormed t
  | .name n, h => .name (by simpa [PyType.wf] using h)
  | .bareList, _ => .bareList
  | .list t, h => .list (PyType.wf_sound t (by simpa [PyType.wf] using h))
  | .tuple ts, h => by
    simp only [PyType.wf, Bool.and_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true,
      List.isEmpty_eq_false_iff] at h
    exact .tuple h.1 (PyType.wfAll_sound ts h.2)
  | .tupleOf t, h => .tupleOf (PyType.wf_sound t (by simpa [PyType.wf] using h))
  | .dict k v, h => by
    simp only [PyType.wf, Bool.and_eq_true] at h
    exact .dict (PyType.wf_sound k h.1) (PyType.wf_sound v h.2)
  | .union ms, h => by
    simp only [PyType.wf, Bool.and_eq_true, decide_eq_true_eq] at h
    obtain ⟨⟨hl, hm⟩, hd⟩ := h
    have := PyType.wfMembers_sound ms hm
    exact .union hl (fun m hm' => (this m hm').1) (fun m hm' => (this m hm').2)
      (by rwa [← PyType.renderAll_eq])

theorem PyType.wfAll_sound : ∀ ts : List PyType, PyType.wfAll ts = true →
    ∀ t ∈ ts, WellFormed t
  | [], _ => by simp
  | t :: ts, h => by
    simp only [PyType.wfAll, Bool.and_eq_true] at h
    intro u hu
    simp only [List.mem_cons] at hu
    rcases hu with rfl | hu
    · exact PyType.wf_sound u h.1
    · exact PyType.wfAll_sound ts h.2 u hu

theorem PyType.wfMembers_sound : ∀ ms : List PyType, PyType.wfMembers ms = true →
    ∀ m ∈ ms, WellFormed m ∧ m.isUnion = false
  | [], _ => by simp
  | t :: ts, h => by
    simp only [PyType.wfMembers, Bool.and_eq_true, Bool.not_eq_eq_eq_not, Bool.not_true] at h
    intro u hu
    simp only [List.mem_cons] at hu
    rcases hu with rfl | hu
    · exact ⟨PyType.wf_sound u h.1.1, h.1.2⟩
    · exact PyType.wfMembers_sound ts h.2 u hu
end

end SqlTy
