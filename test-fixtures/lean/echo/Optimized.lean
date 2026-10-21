import Spec

namespace Candidate

def program (input : String) : String := input

theorem preserves : Contract.preserves program := fun _ => rfl

end Candidate
