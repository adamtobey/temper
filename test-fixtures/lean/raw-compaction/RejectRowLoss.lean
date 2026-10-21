import Spec

/- Deliberate negative control. Returning an error for every request must fail
   the frozen total-correctness obligation, even though it cannot emit bad rows. -/
namespace Candidate

def program (_ : String) : String := "error"

theorem correct : Contract.correct program := Contract.run_correct Contract.referenceDictionary

end Candidate
