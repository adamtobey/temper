import Std

/- Generated from raw-lean/raw_spec.md, compaction_spec.md and the port request.
   This file is the frozen executable format and semantic contract, not a candidate.
   RAW v1 is uncompressed; sizes and dictionary offsets are payload-relative.
   Binary revision: UInt64 output count and Array ByteArray inputs/outputs.
   The high-level row/format obligations are unchanged; the reference reader
   avoids whole-tail length scans and uses indexed input dictionaries. -/
namespace Contract

abbrev Bytes := List UInt8

structure Row where
  org : UInt32
  metric : UInt64
  timestamp : UInt32
  valueBits : UInt64
  tags : List String
  deriving DecidableEq, Repr

def KeyLE (a b : Row) : Prop :=
  a.org.toNat < b.org.toNat ∨
    (a.org.toNat = b.org.toNat ∧ a.metric.toNat ≤ b.metric.toNat)

instance (a b : Row) : Decidable (KeyLE a b) := by unfold KeyLE; infer_instance

def keyLE (a b : Row) : Bool := decide (KeyLE a b)

theorem key_trans {a b c : Row} (ab : KeyLE a b) (bc : KeyLE b c) : KeyLE a c := by
  unfold KeyLE at *
  omega

theorem key_total (a b : Row) : KeyLE a b ∨ KeyLE b a := by
  unfold KeyLE
  omega

def sortedRows (rows : List Row) : List Row := rows.mergeSort keyLE

theorem sorted_perm (rows : List Row) : (sortedRows rows).Perm rows :=
  List.mergeSort_perm _ _

theorem sorted_ordered (rows : List Row) : (sortedRows rows).Pairwise KeyLE := by
  have h := List.pairwise_mergeSort (le := keyLE)
    (fun a b c ab bc => by
      simp only [keyLE, decide_eq_true_eq] at *
      exact key_trans ab bc)
    (fun a b => by simpa only [keyLE, Bool.or_eq_true, decide_eq_true_eq] using key_total a b)
    rows
  simpa only [keyLE, decide_eq_true_eq] using h

def splitEvenly : Nat → List α → List (List α)
  | 0, _ => []
  | n + 1, rows =>
    let size := (rows.length + n) / (n + 1)
    rows.take size :: splitEvenly n (rows.drop size)

theorem split_count (n : Nat) (rows : List α) : (splitEvenly n rows).length = n := by
  induction n generalizing rows with
  | zero => rfl
  | succ n ih => simp [splitEvenly, ih]

theorem split_flatten (n : Nat) (rows : List α) :
    (splitEvenly (n + 1) rows).flatten = rows := by
  induction n generalizing rows with
  | zero => simp [splitEvenly]
  | succ n ih =>
    let size := (rows.length + (n + 1)) / (n + 2)
    change rows.take size ++ (splitEvenly (n + 1) (rows.drop size)).flatten = rows
    rw [ih, List.take_append_drop]

def uniqueStrings : List String → List String
  | [] => []
  | tag :: tags => if tag ∈ tags then uniqueStrings tags else tag :: uniqueStrings tags

theorem unique_mem (needle : String) (tags : List String) :
    needle ∈ uniqueStrings tags ↔ needle ∈ tags := by
  induction tags with
  | nil => simp [uniqueStrings]
  | cons tag tags ih => simp only [uniqueStrings]; split <;> simp_all

theorem unique_nodup (tags : List String) : (uniqueStrings tags).Nodup := by
  induction tags with
  | nil => simp [uniqueStrings]
  | cons tag tags ih => simp only [uniqueStrings]; split <;> simp_all [unique_mem]

abbrev Tags (rows : List Row) := rows.flatMap Row.tags

/-- Any dictionary algorithm is allowed; IDs and dictionary order are file-local. -/
structure DictionaryBuilder where
  build : List Row → List String
  unique : ∀ rows, (build rows).Nodup
  contents : ∀ rows tag, tag ∈ build rows ↔ tag ∈ Tags rows

def referenceDictionary : DictionaryBuilder :=
  ⟨fun rows => uniqueStrings (Tags rows), fun _ => unique_nodup _, fun _ _ => unique_mem _ _⟩

def varint (n : Nat) : Bytes :=
  if n < 128 then [UInt8.ofNat n]
  else UInt8.ofNat (n % 128 + 128) :: varint (n / 128)
termination_by n
decreasing_by omega

def littleEndian (width n : Nat) : Bytes :=
  (List.range width).map fun i => UInt8.ofNat (n / 256 ^ i % 256)

def tagIndex (tag : String) : List String → Nat
  | [] => 0
  | s :: ss => if s = tag then 0 else tagIndex tag ss + 1

theorem tagIndex_resolves {tag : String} {dict : List String} (h : tag ∈ dict) :
    dict[tagIndex tag dict]? = some tag := by
  induction dict with
  | nil => simp at h
  | cons s ss ih =>
    simp only [tagIndex]
    split
    · next eq => subst s; rfl
    · next ne =>
      have ht : tag ∈ ss := by simpa [List.mem_cons, Ne.symm ne] using h
      simpa using ih ht

def rowBytes (dict : List String) (row : Row) : Bytes :=
  littleEndian 4 row.org.toNat ++ littleEndian 8 row.metric.toNat ++
  littleEndian 4 row.timestamp.toNat ++ littleEndian 8 row.valueBits.toNat ++
  varint row.tags.length ++ row.tags.flatMap (fun tag => varint (tagIndex tag dict))

structure Output where
  rows : List Row
  dictionary : List String

def header (out : Output) : List Nat :=
  let data := out.rows.flatMap (rowBytes out.dictionary)
  let dict := out.dictionary.flatMap (fun tag => tag.toUTF8.data.toList ++ [10])
  [1, data.length + dict.length, data.length + dict.length,
    out.rows.length, out.dictionary.length, data.length]

/-- The RAW byte specification: six ULEB128 fields, fixed-width rows, local IDs,
    then exactly the newline-terminated UTF-8 dictionary. No other bytes. -/
def rawBytes (out : Output) : Bytes :=
  (header out).flatMap varint ++ out.rows.flatMap (rowBytes out.dictionary) ++
    out.dictionary.flatMap (fun tag => tag.toUTF8.data.toList ++ [10])

def Clean (rows : List Row) : Prop :=
  ∀ row ∈ rows, ∀ tag ∈ row.tags, tag.contains '\n' = false

instance (rows : List Row) : Decidable (Clean rows) := by unfold Clean; infer_instance

def FormatValid (out : Output) : Prop :=
  out.dictionary.Nodup ∧
  (∀ tag ∈ out.dictionary, tag.contains '\n' = false) ∧
  (∀ row ∈ out.rows, ∀ tag ∈ row.tags,
    out.dictionary[tagIndex tag out.dictionary]? = some tag)

def readVarintAux : Nat → Bytes → Option (Nat × Bytes)
  | 0, _ => none
  | _ + 1, [] => none
  | fuel + 1, byte :: rest =>
    if byte.toNat < 128 then some (byte.toNat, rest)
    else do
      let (high, rest) ← readVarintAux fuel rest
      if high = 0 then none else some (byte.toNat % 128 + 128 * high, rest)

def readVarint (bytes : Bytes) : Option (Nat × Bytes) := do
  let (n, rest) ← readVarintAux 10 bytes
  if n < 2^64 then some (n, rest) else none

def takeExact : Nat → Bytes → Option (Bytes × Bytes)
  | 0, bytes => some ([], bytes)
  | _ + 1, [] => none
  | n + 1, byte :: rest => do
    let (front, rest) ← takeExact n rest
    pure (byte :: front, rest)

def readFixed (width : Nat) (bytes : Bytes) : Option (Nat × Bytes) := do
  let (front, rest) ← takeExact width bytes
  pure (((front.zipIdx.map (fun (byte, i) => byte.toNat * 256^i)).sum), rest)

def readMany (read : Bytes → Option (α × Bytes)) (count : Nat) (bytes : Bytes) :
    Option (List α × Bytes) := loop count bytes []
where
  loop : Nat → Bytes → List α → Option (List α × Bytes)
    | 0, bytes, acc => some (acc.reverse, bytes)
    | n + 1, bytes, acc => do
      let (item, rest) ← read bytes
      loop n rest (item :: acc)

def readEntry (bytes : Bytes) : Option (String × Bytes) := do
  let entry := bytes.takeWhile (· != 10)
  let rest := bytes.drop entry.length
  if rest.isEmpty then none else do
    let tag ← String.fromUTF8? entry.toByteArray
    pure (tag, rest.drop 1)

def readRow (dict : Array String) (bytes : Bytes) : Option (Row × Bytes) := do
  let (org, bytes) ← readFixed 4 bytes
  let (metric, bytes) ← readFixed 8 bytes
  let (timestamp, bytes) ← readFixed 4 bytes
  let (value, bytes) ← readFixed 8 bytes
  let (count, bytes) ← readVarint bytes
  let (ids, bytes) ← readMany readVarint count bytes
  let tags ← ids.mapM fun id => dict[id]?
  pure (⟨UInt32.ofNat org, UInt64.ofNat metric, UInt32.ofNat timestamp,
    UInt64.ofNat value, tags⟩, bytes)

/-- Strict reader: v1, uncompressed sizes, exact row/dictionary boundaries,
    canonical bounded varints, UTF-8, unique dictionary, and in-bounds IDs. -/
def readRaw (bytes : Bytes) : Option (List Row) := do
  let (fields, payload) ← readMany readVarint 6 bytes
  match fields with
  | [version, compressed, uncompressed, count, entries, offset] =>
    if version != 1 || compressed != payload.length || uncompressed != payload.length ||
        offset > payload.length || count > offset / 25 || entries > payload.length - offset then none
    else do
      let (dict, tail) ← readMany readEntry entries (payload.drop offset)
      if !tail.isEmpty || (Std.TreeSet.ofList dict).size != dict.length then none else do
        let (rows, tail) ← readMany (readRow dict.toArray) count (payload.take offset)
        if tail.isEmpty then some rows else none
  | _ => none

structure Request where
  count : Nat
  rows : List Row
  positive : 0 < count
  clean : Clean rows

/-- Binary inputs are already loaded; no generation, file I/O or hex transport. -/
def request (parameter : UInt64) (files : Array ByteArray) : Option Request := do
  let count := parameter.toNat
  if h : 0 < count ∧ count ≤ 1024 then do
    if files.isEmpty then none else do
      let rows ← files.toList.mapM fun file => readRaw file.data.toList
      let rows := rows.flatten
      if clean : Clean rows then some ⟨count, rows, h.1, clean⟩ else none
  else none

def render (outputs : List Output) : Array ByteArray :=
  (outputs.map (fun out => (rawBytes out).toByteArray)).toArray

def plans (builder : DictionaryBuilder) (req : Request) : List Output :=
  (splitEvenly req.count (sortedRows req.rows)).map fun rows => ⟨rows, builder.build rows⟩

/-- Balance is the fixed recursive ceiling split; this also fixes empty outputs. -/
def ValidCompaction (req : Request) (outputs : List Output) : Prop :=
  (outputs.map Output.rows).flatten.Perm req.rows ∧
  (outputs.map Output.rows).flatten.Pairwise KeyLE ∧
  outputs.length = req.count ∧
  (outputs.map Output.rows).map List.length =
    (splitEvenly req.count (sortedRows req.rows)).map List.length ∧
  ∀ out ∈ outputs, FormatValid out

/-- Totality is required: a valid request must produce valid files, not an error.
    Output bytes must be the canonical RAW encoding of the conserved rows. -/
abbrev Program := UInt64 → Array ByteArray → Except String (Array ByteArray)

def correct (program : Program) : Prop := ∀ parameter files,
  match request parameter files with
  | none => program parameter files = .error "invalid RAW compaction request"
  | some req => ∃ outputs, program parameter files = .ok (render outputs) ∧ ValidCompaction req outputs

def run (builder : DictionaryBuilder) (parameter : UInt64) (files : Array ByteArray) :
    Except String (Array ByteArray) :=
  match request parameter files with
  | none => .error "invalid RAW compaction request"
  | some req => .ok (render (plans builder req))

theorem plans_valid (builder : DictionaryBuilder) (req : Request) :
    ValidCompaction req (plans builder req) := by
  have flat : (splitEvenly req.count (sortedRows req.rows)).flatten = sortedRows req.rows := by
    cases hc : req.count with
    | zero => have := req.positive; omega
    | succ n => exact split_flatten n _
  have rows : ((plans builder req).map Output.rows).flatten = sortedRows req.rows := by
    simpa [plans, List.map_map, Function.comp_def] using flat
  refine ⟨?_, ?_, ?_, ?_, ?_⟩
  · rw [rows]; exact sorted_perm _
  · rw [rows]; exact sorted_ordered _
  · simp [plans, split_count]
  · simp [plans, List.map_map]
  · intro out mem
    obtain ⟨chunk, hc, rfl⟩ := List.mem_map.mp mem
    refine ⟨builder.unique chunk, ?_, ?_⟩
    · intro tag ht
      have ht := (builder.contents chunk tag).mp ht
      obtain ⟨row, hr, ht⟩ := List.mem_flatMap.mp ht
      have hs : row ∈ sortedRows req.rows := flat ▸ List.mem_flatten.mpr ⟨chunk, hc, hr⟩
      exact req.clean row ((sorted_perm _).mem_iff.mp hs) tag ht
    · intro row hr tag ht
      apply tagIndex_resolves
      exact (builder.contents chunk tag).mpr (List.mem_flatMap.mpr ⟨row, hr, ht⟩)

theorem run_correct (builder : DictionaryBuilder) : correct (run builder) := by
  intro parameter files
  cases h : request parameter files with
  | none => simp [run, h]
  | some req => exact ⟨plans builder req, by simp [run, h], plans_valid builder req⟩

/-- The requested set equality follows from the stronger multiplicity theorem. -/
theorem row_set_equality (req : Request) (outputs : List Output)
    (h : ValidCompaction req outputs) (row : Row) :
    row ∈ (outputs.map Output.rows).flatten ↔ row ∈ req.rows := h.1.mem_iff

theorem format_header (out : Output) : (header out).length = 6 := rfl

/-- A fully proved model exists before any candidate is generated. -/
theorem consistent : ∃ program, correct program := ⟨run referenceDictionary, run_correct _⟩

end Contract
