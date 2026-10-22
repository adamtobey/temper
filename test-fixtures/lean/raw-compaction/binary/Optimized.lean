-- raw-search-config {"cache": "hash", "codec": "stream", "dictionary": "ascending"}
import Spec

/- Binary ABI baseline, generated after the new contract gate. Port of the
   previous verified winner, not a new optimization claim.
   Prior optimization: one proved tag→ID index per output file, shared by all
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

def indexes : List String → Nat → Std.HashMap String Nat
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
      simp [indexes, Std.HashMap.getD_insert, eq, Contract.tagIndex, ih ht]
      omega

theorem flatMap_eq (items : List α) (f g : α → List β)
    (h : ∀ item ∈ items, f item = g item) : items.flatMap f = items.flatMap g := by
  induction items with
  | nil => rfl
  | cons item rest ih =>
    simp only [List.flatMap_cons]
    rw [h item (by simp), ih (fun x hx => h x (by simp [hx]))]

def rowBytes (index : Std.HashMap String Nat) (row : Contract.Row) : Contract.Bytes :=
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


theorem push_eq (items : Contract.Bytes) (acc : ByteArray) :
    items.foldl ByteArray.push acc = acc ++ items.toByteArray := by
  induction items generalizing acc with
  | nil => simp
  | cons byte rest ih =>
    rw [List.foldl_cons, ih]
    change acc.push byte ++ rest.toByteArray = acc ++ ([byte] ++ rest).toByteArray
    rw [List.toByteArray_append, ← ByteArray.append_assoc, ByteArray.append_toByteArray_singleton]

theorem foldWrite_eq (items : List α) (write : ByteArray → α → ByteArray)
    (bytes : α → Contract.Bytes)
    (h : ∀ acc item, write acc item = acc ++ (bytes item).toByteArray) (acc : ByteArray) :
    items.foldl write acc = acc ++ (items.flatMap bytes).toByteArray := by
  induction items generalizing acc with
  | nil => simp
  | cons item rest ih =>
    simp only [List.foldl_cons, List.flatMap_cons, List.toByteArray_append]
    rw [ih, h, ByteArray.append_assoc]

def writeRow (index : Std.HashMap String Nat) (acc : ByteArray) (row : Contract.Row) : ByteArray :=
  let fields := Contract.littleEndian 4 row.org.toNat ++ Contract.littleEndian 8 row.metric.toNat ++
    Contract.littleEndian 4 row.timestamp.toNat ++ Contract.littleEndian 8 row.valueBits.toNat ++
    Contract.varint row.tags.length
  row.tags.foldl (fun out tag => (Contract.varint (index.getD tag 0)).foldl ByteArray.push out)
    (fields.foldl ByteArray.push acc)

theorem writeRow_eq (index : Std.HashMap String Nat) (acc : ByteArray) (row : Contract.Row) :
    writeRow index acc row = acc ++ (rowBytes index row).toByteArray := by
  unfold writeRow
  rw [foldWrite_eq _ _ (fun tag => Contract.varint (index.getD tag 0)) (fun a _ => push_eq _ a)]
  simp only [push_eq, rowBytes, List.toByteArray_append, ByteArray.append_assoc]

theorem byteRoundtrip (bytes : ByteArray) : bytes.data.toList.toByteArray = bytes := by
  apply ByteArray.ext
  simp [List.data_toByteArray]

def writeTag (acc : ByteArray) (tag : String) : ByteArray := (acc ++ tag.toUTF8).push 10

theorem writeTag_eq (acc : ByteArray) (tag : String) :
    writeTag acc tag = acc ++ (tag.toUTF8.data.toList ++ [10]).toByteArray := by
  unfold writeTag
  rw [List.toByteArray_append, byteRoundtrip, ← ByteArray.append_assoc,
    ByteArray.append_toByteArray_singleton]

def encoded (out : Contract.Output) : ByteArray :=
  let index := indexes out.dictionary 0
  let data := out.rows.foldl (writeRow index) ByteArray.empty
  let dict := out.dictionary.foldl writeTag ByteArray.empty
  let size := data.size + dict.size
  ([1, size, size, out.rows.length, out.dictionary.length, data.size].flatMap Contract.varint).toByteArray ++ data ++ dict

theorem encoded_eq (out : Contract.Output) : encoded out = (rawBytes out).toByteArray := by
  dsimp only [encoded]
  rw [foldWrite_eq _ _ (rowBytes _) (writeRow_eq _)]
  rw [foldWrite_eq _ _ (fun tag => tag.toUTF8.data.toList ++ [10]) writeTag_eq]
  simp only [ByteArray.empty_append, List.size_toByteArray, rawBytes, List.toByteArray_append]

def render (outputs : List Contract.Output) : Array ByteArray :=
  (outputs.map encoded).toArray

theorem render_eq (req : Contract.Request) :
    render (Contract.plans builder req) = Contract.render (Contract.plans builder req) := by
  unfold render Contract.render
  congr 1
  apply List.map_congr_left
  intro out ho
  obtain ⟨chunk, hc, rfl⟩ := List.mem_map.mp ho
  rw [encoded_eq, rawBytes_eq]
  intro row hr tag ht
  exact (builder.contents chunk tag).mpr (List.mem_flatMap.mpr ⟨row, hr, ht⟩)

def program (parameter : UInt64) (files : Array ByteArray) : Except String (Array ByteArray) :=
  match Contract.request parameter files with
  | none => .error "invalid RAW compaction request"
  | some req => .ok (render (Contract.plans builder req))

theorem program_eq : program = Contract.run builder := by
  funext parameter files
  unfold program Contract.run
  cases Contract.request parameter files with
  | none => rfl
  | some req => exact congrArg Except.ok (render_eq req)

theorem correct : Contract.correct program := by
  rw [program_eq]
  exact Contract.run_correct _

end Candidate
