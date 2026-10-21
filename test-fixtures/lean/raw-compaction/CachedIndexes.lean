import Spec

/- Second optimization: one proved tag→ID index per output file, shared by all
   rows. This removes repeated linear dictionary scans during serialization. -/
namespace Candidate

def dictionary (rows : List Contract.Row) : List String :=
  (Std.TreeSet.ofList (Contract.Tags rows)).toList

theorem dictionary_unique (rows : List Contract.Row) : (dictionary rows).Nodup := by
  change (Std.TreeSet.ofList (Contract.Tags rows) compare).toList.Pairwise (· ≠ ·)
  apply (Std.TreeSet.distinct_toList (t := Std.TreeSet.ofList (Contract.Tags rows) compare)).imp
  intro a b ne eq
  subst b
  exact ne (by simp)

theorem dictionary_contents (rows : List Contract.Row) (tag : String) :
    tag ∈ dictionary rows ↔ tag ∈ Contract.Tags rows := by
  simp [dictionary, Std.TreeSet.mem_ofList]

def builder : Contract.DictionaryBuilder :=
  ⟨dictionary, dictionary_unique, dictionary_contents⟩

def indexes : List String → Nat → Std.TreeMap String Nat
  | [], _ => ∅
  | tag :: rest, start => (indexes rest (start + 1)).insert tag start

theorem indexes_correct {dict : List String} {tag : String} (h : tag ∈ dict) (start : Nat) :
    (indexes dict start).getD tag 0 = start + Contract.tagIndex tag dict := by
  induction dict generalizing start with
  | nil => simp at h
  | cons head rest ih =>
    by_cases eq : head = tag
    · subst head; simp [indexes, Contract.tagIndex]
    · have ht : tag ∈ rest := by simpa [List.mem_cons, Ne.symm eq] using h
      simp [indexes, Std.TreeMap.getD_insert, eq, Contract.tagIndex, ih ht]
      omega

theorem flatMap_eq (items : List α) (f g : α → List β)
    (h : ∀ item ∈ items, f item = g item) : items.flatMap f = items.flatMap g := by
  induction items with
  | nil => rfl
  | cons item rest ih =>
    simp only [List.flatMap_cons]
    rw [h item (by simp), ih (fun x hx => h x (by simp [hx]))]

def rowBytes (index : Std.TreeMap String Nat) (row : Contract.Row) : Contract.Bytes :=
  Contract.littleEndian 4 row.org.toNat ++ Contract.littleEndian 8 row.metric.toNat ++
  Contract.littleEndian 4 row.timestamp.toNat ++ Contract.littleEndian 8 row.valueBits.toNat ++
  Contract.varint row.tags.length ++ row.tags.flatMap (fun tag => Contract.varint (index.getD tag 0))

theorem rowBytes_eq (dict : List String) (row : Contract.Row)
    (h : ∀ tag ∈ row.tags, tag ∈ dict) :
    rowBytes (indexes dict 0) row = Contract.rowBytes dict row := by
  unfold rowBytes Contract.rowBytes
  congr 1
  apply flatMap_eq
  intro tag ht
  simp [indexes_correct (h tag ht)]

def rawBytes (out : Contract.Output) : Contract.Bytes :=
  let index := indexes out.dictionary 0
  let data := out.rows.flatMap (rowBytes index)
  let dict := out.dictionary.flatMap (fun tag => tag.toUTF8.data.toList ++ [10])
  let size := data.length + dict.length
  [1, size, size, out.rows.length, out.dictionary.length, data.length].flatMap Contract.varint ++ data ++ dict

theorem rawBytes_eq (out : Contract.Output)
    (h : ∀ row ∈ out.rows, ∀ tag ∈ row.tags, tag ∈ out.dictionary) :
    rawBytes out = Contract.rawBytes out := by
  have data : out.rows.flatMap (rowBytes (indexes out.dictionary 0)) =
      out.rows.flatMap (Contract.rowBytes out.dictionary) := by
    apply flatMap_eq
    intro row hr
    exact rowBytes_eq _ _ (h row hr)
  simp only [rawBytes, data, Contract.rawBytes, Contract.header]

def render (outputs : List Contract.Output) : String :=
  String.intercalate "\n" ("ok" :: outputs.map (fun out => Contract.hex (rawBytes out)))

theorem render_eq (req : Contract.Request) :
    render (Contract.plans builder req) = Contract.render (Contract.plans builder req) := by
  unfold render Contract.render
  congr 2
  apply List.map_congr_left
  intro out ho
  obtain ⟨chunk, hc, rfl⟩ := List.mem_map.mp ho
  rw [rawBytes_eq]
  intro row hr tag ht
  exact (builder.contents chunk tag).mpr (List.mem_flatMap.mpr ⟨row, hr, ht⟩)

def program (input : String) : String :=
  match Contract.request input with
  | none => "error"
  | some req => render (Contract.plans builder req)

theorem program_eq : program = Contract.run builder := by
  funext input
  unfold program Contract.run
  cases Contract.request input with
  | none => rfl
  | some req => exact render_eq req

theorem correct : Contract.correct program := by
  rw [program_eq]
  exact Contract.run_correct _

end Candidate
