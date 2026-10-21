import Spec

/- Generated only after the specification/benchmark proof gate passed.
   Port of the source's full merge-sort and list-based local dictionary plan. -/
namespace Candidate

def program : String → String := Contract.run Contract.referenceDictionary

theorem correct : Contract.correct program := Contract.run_correct _

end Candidate
