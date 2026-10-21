import Spec

/- Optimization proposal: replace quadratic duplicate elimination with a balanced
   tree set. Dictionary order is intentionally free in the frozen contract. -/
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

def program : String → String := Contract.run builder

theorem correct : Contract.correct program := Contract.run_correct _

end Candidate
