/- Generated from ADR-0182's native-core interoperability scenario.
   A small complete component: preserve every input exactly, including UTF-8,
   embedded NUL bytes, and caller-supplied state/action envelopes. -/
namespace Contract

abbrev Program := String → String

def preserves (program : Program) : Prop := ∀ input, program input = input

theorem consistent : ∃ program, preserves program := ⟨id, fun _ => rfl⟩

end Contract
