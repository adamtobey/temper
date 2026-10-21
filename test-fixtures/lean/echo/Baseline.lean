import Spec

namespace Candidate

def copy (count : Nat) (input : String) : String :=
  match count with
  | 0 => input
  | count + 1 => copy count (input ++ "")

theorem copy_preserves (count : Nat) (input : String) : copy count input = input := by
  induction count with
  | zero => rfl
  | succ count previous => simpa [copy] using previous

def program (input : String) : String := copy 2000 input

theorem preserves : Contract.preserves program := fun input => copy_preserves 2000 input

end Candidate
