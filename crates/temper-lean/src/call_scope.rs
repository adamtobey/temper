//! Lean thread contexts cannot be finalized beneath another live native reply.
use anyhow::{Result, ensure};
use std::{cell::Cell, marker::PhantomData, rc::Rc};

thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; }

pub(crate) struct Scope(PhantomData<Rc<()>>);

impl Scope {
    pub(crate) fn enter() -> Result<Self> {
        ACTIVE.with(|active| {
            ensure!(
                !active.get(),
                "reentrant Lean calls/initialization are not supported"
            );
            active.set(true);
            Ok(Self(PhantomData))
        })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(false));
    }
}
