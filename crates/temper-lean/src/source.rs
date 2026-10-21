//! The first native ABI accepts pure Lean cores, not user-defined elaborators or
//! module initialization. Kernel proofs alone do not cover those execution hooks.

use anyhow::{Result, ensure};

pub(crate) fn validate(source: &str) -> Result<()> {
    let mut code = String::new();
    let mut chars = source.chars().peekable();
    let mut comment = 0u32;
    let mut string = false;
    while let Some(c) = chars.next() {
        if comment > 0 {
            if c == '/' && chars.peek() == Some(&'-') {
                chars.next();
                comment += 1;
            } else if c == '-' && chars.peek() == Some(&'/') {
                chars.next();
                comment -= 1;
            }
        } else if string {
            if c == '\\' {
                chars.next();
            } else if c == '"' {
                string = false;
                code.push(' ');
            }
        } else if c == '"' {
            ensure!(
                !code.trim_end().ends_with('!'),
                "interpolated Lean syntax is outside the initial pure-source fragment"
            );
            string = true;
            code.push(' ');
        } else if c == '/' && chars.peek() == Some(&'-') {
            chars.next();
            comment = 1;
            code.push(' ');
        } else if c == '-' && chars.peek() == Some(&'-') {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            code.push('\n');
        } else {
            code.push(c);
        }
    }
    ensure!(comment == 0 && !string, "unterminated Lean comment/string");
    ensure!(
        !code.contains('#') && !code.contains("@["),
        "native cores cannot install attributes or executable commands"
    );
    for token in code.split(|c: char| !c.is_alphanumeric() && c != '_') {
        ensure!(
            !matches!(
                token,
                "unsafe"
                    | "partial"
                    | "initialize"
                    | "builtin_initialize"
                    | "attribute"
                    | "set_option"
                    | "macro"
                    | "macro_rules"
                    | "elab_rules"
                    | "notation"
                    | "infix"
                    | "infixl"
                    | "infixr"
                    | "prefix"
                    | "postfix"
                    | "declare_syntax_cat"
                    | "syntax"
                    | "elab"
                    | "run_cmd"
                    | "run_tac"
                    | "run_elab"
                    | "run_meta"
                    | "by_elab"
                    | "Lean"
                    | "IO"
                    | "BaseIO"
                    | "EIO"
                    | "implemented_by"
                    | "extern"
                    | "export"
            ),
            "native core uses unsupported execution/metaprogramming hook: {token}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pure_source_excludes_initializers_and_metaprograms_but_not_comments_or_data() {
        assert!(
            validate("def x := \"IO #eval\" /- /- unsafe -/ initialize -/ -- run_cmd\n").is_ok()
        );
        for source in [
            "initialize x : IO Unit ← pure ()",
            "@[extern \"fake\"] def x := 1",
            "by run_tac do pure ()",
            "#eval 1",
            "set_option debug.skipKernelTC true",
        ] {
            assert!(validate(source).is_err(), "accepted: {source}");
        }
    }
}
