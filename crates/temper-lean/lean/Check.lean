import Lean
import Lean.Replay
import Lean.Compiler.ImplementedByAttr

open Lean Lean.Meta

partial def audit (env trusted : Environment) (name : Name)
    (seen : NameSet := {}) : IO NameSet := do
  if seen.contains name then return seen
  let seen := seen.insert name
  let some ci := env.find? name | throw <| IO.userError s!"missing declaration: {name}"
  if [`IO, `BaseIO, `EIO, `Task].any (fun root => root.isPrefixOf name) then
    throw <| IO.userError s!"unsupported ambient runtime capability: {name}"
  if let .axiomInfo _ := ci then
    unless name == `propext || name == `Classical.choice || name == `Quot.sound do
      throw <| IO.userError s!"unsupported axiom: {name}"
  if trusted.contains name then return seen
  if ci.isUnsafe || ci.isPartial then
    throw <| IO.userError s!"unsafe or partial declaration: {name}"
  if (Lean.Compiler.getImplementedBy? env name).isSome || (getExternAttrData? env name).isSome then
    throw <| IO.userError s!"unchecked native substitution: {name}"
  let mut seen := seen
  for dep in ci.getUsedConstantsAsSet do seen ← audit env trusted dep seen
  return seen

unsafe def main (args : List String) : IO UInt32 := do
  initSearchPath (← findSysroot)
  let mode := args.head!
  let obligations := (args.drop 1).map String.toName
  let base ← importModules #[{ module := `Init }] {} (loadExts := false)
  let spec ← importModules #[{ module := `Spec }] {} (loadExts := false)
  let target ← if mode == "spec" then pure spec
    else importModules #[{ module := `Entry }] {} (loadExts := false)
  let mut additions := {}
  for (name, ci) in target.constants.toList do
    if !base.contains name then additions := additions.insert name ci
  discard <| base.replay additions
  -- Completeness applies to helper proofs too, not only manifest entries.
  let mut audited : NameSet := {}
  for (name, ci) in target.constants.toList do
    if let some moduleIdx := target.getModuleIdxFor? name then
      let owner := target.header.moduleNames[moduleIdx.toNat]!
      -- Lean generates unsafe native recursion workers for safe recursive
      -- definitions. These are compiler artifacts, not logical declarations.
      -- User-supplied unsafe/partial code is excluded by the source boundary;
      -- every logical helper and every required proof is still audited.
      if (owner == `Spec || owner == `Candidate || owner == `Entry) &&
          !ci.isUnsafe && !ci.isPartial then
        audited ← audit target base name audited
  for (name, ci) in spec.constants.toList do
    let some actual := target.find? name | throw <| IO.userError s!"removed contract: {name}"
    unless actual.type == ci.type && actual.value? true == ci.value? true &&
        actual.levelParams == ci.levelParams do
      throw <| IO.userError s!"changed contract: {name}"
    unless Lean.Compiler.getImplementedBy? target name == Lean.Compiler.getImplementedBy? spec name &&
        getExternAttrData? target name == getExternAttrData? spec name do
      throw <| IO.userError s!"changed native semantics: {name}"
  let action : MetaM Unit := do
    if mode != "spec" then
      let programType ← inferType (mkConst `Candidate.program)
      let expectedType ← if mode == "buffers" then
        mkArrow (mkConst `UInt64) (← mkArrow
          (mkApp (mkConst `Array [Level.zero]) (mkConst `ByteArray))
          (mkApp2 (mkConst `Except [Level.zero, Level.zero]) (mkConst `String)
            (mkApp (mkConst `Array [Level.zero]) (mkConst `ByteArray))))
        else mkArrow (mkConst `String) (mkConst `String)
      unless ← isDefEq programType expectedType do
        throwError "program does not have the manifest's ABI type"
      unless ← isDefEq (mkConst `temperEntry) (mkConst `Candidate.program) do
        throwError "native entry point is not the proved program"
    for obligation in obligations do
      let proof := if mode == "spec" then `Contract ++ obligation else `Candidate ++ obligation
      let actual ← inferType (mkConst proof)
      if mode == "spec" then
        unless ← isProp actual do throwError "specification obligation is not a theorem"
      else
        let expected := mkApp (mkConst (`Contract ++ obligation)) (mkConst `Candidate.program)
        unless ← isDefEq actual expected do throwError "incorrect obligation: {proof}"
  discard <| (action.run' {}).toIO { fileName := "temper-check", fileMap := default } { env := target }
  for obligation in obligations do
    let proof := if mode == "spec" then `Contract ++ obligation else `Candidate ++ obligation
    discard <| audit target base proof
  if mode != "spec" then discard <| audit target base `temperEntry
  IO.println "temper-lean:checked"
  return 0
