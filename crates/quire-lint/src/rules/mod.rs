//! Independent rule passes. Each pass owns its collector so parallel execution needs no locks.

pub(crate) mod charts;
pub(crate) mod citations;
pub(crate) mod core;
pub(crate) mod evidence;
pub(crate) mod higher_ed;
pub(crate) mod internal;
pub(crate) mod prose;
pub(crate) mod structure;
pub(crate) mod style;
pub(crate) mod tables;

use rayon::prelude::*;

use crate::{context::LintContext, engine::Finding};

type RulePass = fn(&LintContext<'_>) -> Vec<Finding>;

const PASSES: &[RulePass] = &[
    core::check,
    citations::check,
    tables::check,
    charts::check,
    structure::check,
    prose::check,
    higher_ed::check,
    internal::check,
    style::check,
];

pub(crate) fn run(context: &LintContext<'_>, parallel: bool) -> Vec<Finding> {
    if parallel {
        PASSES
            .par_iter()
            .flat_map_iter(|pass| pass(context))
            .collect()
    } else {
        PASSES.iter().flat_map(|pass| pass(context)).collect()
    }
}
