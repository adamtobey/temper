"""Provider-adapter tests use fake inference; Lean itself is checked separately."""
import json
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import model_proposer
import mutate


class Proposers(unittest.TestCase):
    def test_provider_receives_contract_and_failure_feedback(self):
        context = {"source": "frozen-spec", "recent_proposals": [{"diagnostic": "proof-failed"}]}
        command = [sys.executable, "-c", "import sys; p=sys.stdin.read(); assert 'frozen-spec' in p and 'proof-failed' in p; print('```lean\\nimport Spec\\nnamespace Candidate\\nend Candidate\\n```')"]
        self.assertEqual(model_proposer.propose(context, command), "import Spec\nnamespace Candidate\nend Candidate")

    def test_provider_errors_do_not_substitute_canned_candidates(self):
        with self.assertRaises(RuntimeError):
            model_proposer.propose({}, [sys.executable, "-c", "raise SystemExit(2)"])
        with self.assertRaises(ValueError):
            model_proposer.propose({}, [])

    def test_mutator_changes_parent_settings_and_avoids_repetition(self):
        parent = {"dictionary": "descending", "cache": "hash", "codec": "stream"}
        source = mutate.generate(parent)
        context = {"parent": {"source": source}, "history": [{"proposal_header": source.splitlines()[0], "candidate": "verified"}]}
        proposed = mutate.configuration(mutate.propose(context))
        self.assertEqual(sum(parent[k] != proposed[k] for k in parent), 1)
        self.assertNotEqual(proposed, parent)

    def test_finite_family_exhaustion_is_explicit(self):
        history = [{"proposal_header": mutate.PREFIX + json.dumps(dict(zip(mutate.DEFAULT, item))),
                    "candidate": "verified"} for item in mutate.GRID]
        self.assertEqual(mutate.propose({"parent": {"source": mutate.generate(mutate.DEFAULT)}, "history": history}), "")


if __name__ == "__main__":
    unittest.main()
