//! Deterministic candidate selection and artifact-bound deployment review.

use crate::{
    manifest::Direction,
    native::ReviewedCandidate,
    toolchain::{Toolchain, VerifiedCandidate, VerifiedSpec},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;

struct Selection {
    best: Option<(String, f64)>,
    reviewed: Option<String>,
}

impl Selection {
    fn select(
        &mut self,
        id: &str,
        score: f64,
        direction: Direction,
        threshold: f64,
    ) -> Result<bool> {
        ensure!(
            score.is_finite() && score > 0.0,
            "benchmark measurement must be positive and finite"
        );
        let improves = self.best.as_ref().is_none_or(|(_, old)| match direction {
            Direction::Minimize => score < old * (1.0 - threshold),
            Direction::Maximize => score > old * (1.0 + threshold),
        });
        if improves {
            self.best = Some((id.into(), score));
        }
        Ok(improves)
    }
    fn review(&mut self, id: &str) {
        self.reviewed = Some(id.into());
    }
    fn can_deploy(&self, id: &str) -> bool {
        self.reviewed.as_deref() == Some(id)
    }
}

/// Recorded outcome for one bounded generation, including rejected candidates.
#[derive(Clone, Debug, Serialize)]
pub struct Attempt {
    /// Parent selected before this generation, absent for initial synthesis.
    pub parent: Option<String>,
    /// Checked artifact, absent when proof checking or compilation failed.
    pub candidate: Option<String>,
    /// Host-observed metric for the fixed benchmark.
    pub measurement: Option<f64>,
    /// Whether this candidate became the next search parent.
    pub accepted: bool,
    /// Diagnostic for a rejected/inconclusive generation.
    pub diagnostic: Option<String>,
}

/// External generators receive only the fixed specification and current parent.
/// App-specific prompts, models, and agents live outside the Temper kernel.
pub trait Proposer {
    /// Return implementation source, or None when search has finished.
    fn propose(
        &mut self,
        spec: &VerifiedSpec,
        parent: Option<&VerifiedCandidate>,
        history: &[Attempt],
    ) -> Result<Option<String>>;
}

/// Search state, independent from the currently deployed implementation.
pub struct Episode {
    spec: VerifiedSpec,
    budget: u32,
    selection: Selection,
    candidates: BTreeMap<String, VerifiedCandidate>,
    history: Vec<Attempt>,
}

impl Episode {
    /// Begin only from a fully verified specification.
    pub fn new(spec: &VerifiedSpec, budget: u32) -> Result<Self> {
        ensure!(budget > 0, "candidate budget must be positive");
        ensure!(
            spec.manifest.objective.is_some(),
            "optimization requires a benchmark objective"
        );
        Ok(Self {
            spec: spec.clone(),
            budget,
            selection: Selection {
                best: None,
                reviewed: None,
            },
            candidates: BTreeMap::new(),
            history: Vec::new(),
        })
    }

    /// Submit a generation. Failed proofs and inconclusive work consume budget
    /// and preserve the last accepted parent. No review is needed for search.
    pub fn submit(&mut self, source: String) -> Result<&Attempt> {
        ensure!(
            self.history.len() < self.budget as usize,
            "candidate budget exhausted"
        );
        let mut attempt = Attempt {
            parent: self.selection.best.as_ref().map(|p| p.0.clone()),
            candidate: None,
            measurement: None,
            accepted: false,
            diagnostic: None,
        };
        let result = Toolchain
            .check(&self.spec, source)
            .and_then(VerifiedCandidate::benchmark);
        match result {
            Ok((candidate, score)) => {
                attempt.candidate = Some(candidate.id.clone());
                attempt.measurement = Some(score);
                let objective = self
                    .spec
                    .manifest
                    .objective
                    .as_ref()
                    .expect("validated objective");
                match self.selection.select(
                    &candidate.id,
                    score,
                    objective.direction,
                    objective.minimum_improvement,
                ) {
                    Ok(accepted) => {
                        attempt.accepted = accepted;
                        if accepted {
                            self.candidates.insert(candidate.id.clone(), candidate);
                        }
                    }
                    Err(error) => attempt.diagnostic = Some(error.to_string()),
                }
            }
            Err(error) => attempt.diagnostic = Some(format!("{error:#}")),
        }
        self.history.push(attempt);
        Ok(self.history.last().expect("just recorded attempt"))
    }

    /// Run an external proposer iteratively, feeding every verified improvement
    /// into the next generation. This method never deploys an artifact.
    pub fn run(&mut self, proposer: &mut impl Proposer) -> Result<()> {
        while self.history.len() < self.budget as usize {
            match proposer.propose(&self.spec, self.best(), &self.history) {
                Ok(Some(source)) => {
                    self.submit(source)?;
                }
                Ok(None) => break,
                Err(error) => self.history.push(Attempt {
                    parent: self.selection.best.as_ref().map(|p| p.0.clone()),
                    candidate: None,
                    measurement: None,
                    accepted: false,
                    diagnostic: Some(format!("proposal failed: {error:#}")),
                }),
            }
        }
        Ok(())
    }

    /// Best verified implementation; available without deployment review.
    pub fn best(&self) -> Option<&VerifiedCandidate> {
        self.selection
            .best
            .as_ref()
            .and_then(|(id, _)| self.candidates.get(id))
    }

    /// Complete lineage and diagnostics, suitable for recording in an app's store.
    pub fn history(&self) -> &[Attempt] {
        &self.history
    }

    /// Explicit review for one retained artifact in this episode's tenant.
    pub fn approve(&mut self, id: &str, reviewer: &str) -> Result<ReviewedCandidate> {
        let candidate = self
            .candidates
            .get(id)
            .context("unknown candidate in this episode")?
            .clone();
        let reviewed =
            ReviewedCandidate::approve(candidate, &self.spec.manifest.tenant, id, reviewer)?;
        self.selection.review(id);
        ensure!(
            self.selection.can_deploy(id),
            "artifact review was not recorded"
        );
        Ok(reviewed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Direction;

    #[test]
    fn seeded_search_keeps_unreviewed_improvements_separate_from_deployment() {
        for seed in 0u64..256 {
            let mut state = Selection {
                best: None,
                reviewed: None,
            };
            let mut random = seed + 1;
            let mut oracle = f64::INFINITY;
            let mut reviewed = None;
            for step in 0..100 {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                let score = (random % 1000 + 1) as f64;
                let id = format!("{seed}-{step}");
                let accepted = state.select(&id, score, Direction::Minimize, 0.0).unwrap();
                assert_eq!(
                    accepted,
                    score < oracle,
                    "seed {seed}, step {step}: selection"
                );
                if accepted {
                    oracle = score;
                    assert_eq!(
                        state.best.as_ref().unwrap().0,
                        id,
                        "seed {seed}: parent must advance without review"
                    );
                    assert!(
                        !state.can_deploy(&id),
                        "seed {seed}: new artifact inherited review"
                    );
                }
                if random % 7 == 0 {
                    let best = state.best.as_ref().unwrap().0.clone();
                    state.review(&best);
                    reviewed = Some(best);
                }
                assert_eq!(
                    state.can_deploy(&id),
                    reviewed.as_deref() == Some(&id),
                    "seed {seed}: deployment review"
                );
            }
        }
    }
}
