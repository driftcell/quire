//! Static AI-slop and low-evidence prose checks for paragraph blocks.

use std::{collections::BTreeSet, sync::LazyLock};

use aho_corasick::{AhoCorasick, BuildError};
use quire_protocol::{BlockKind, ColumnNode};
use regex::Regex;

use super::{
    evidence::has_quantitative_evidence,
    structure::{action_or_closing_section_exempt, dense_prose_section_exempt},
};
use crate::{
    context::LintContext,
    engine::{Engine, Finding},
    registry::RuleId,
};

mod constants;
use constants::*;

static CHAPTER_GUIDE_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?:本章|本节|下文|后文|接下来)[^。；！？\n]{0,24}(?:先看|(?:从|以|围绕|按照|通过)[^。；！？\n]{0,12}[一二两三四五六七八九十\d]+个?(?:维度|方面|观测面|角度|层面|视角|步骤|部分))|(?:观察|分析|研判|审视)?(?:角度|维度|观测面|层面|视角)有[一二两三四五六七八九十\d]+",
    )
    .ok()
});

struct ClichePattern {
    phrase: &'static str,
    unconditional: bool,
}

static CLICHE_PATTERNS: LazyLock<Vec<ClichePattern>> = LazyLock::new(|| {
    CLICHE_TRANSITIONS
        .iter()
        .chain(PROCESS_AND_DISCLAIMER_CLICHES)
        .chain(EXTREME_JARGON_CLICHES)
        .map(|phrase| ClichePattern {
            phrase,
            unconditional: true,
        })
        .chain(
            EMPTY_JARGON
                .iter()
                .filter(|phrase| **phrase != "生态")
                .map(|phrase| ClichePattern {
                    phrase,
                    unconditional: false,
                }),
        )
        .collect()
});

static CLICHE_AUTOMATON: LazyLock<Result<AhoCorasick, BuildError>> =
    LazyLock::new(|| AhoCorasick::new(CLICHE_PATTERNS.iter().map(|pattern| pattern.phrase)));

mod cliche;
mod density_meta;
pub(crate) mod masking;
mod template_contrast;

pub(super) use masking::*;

pub(crate) fn check(context: &LintContext<'_>) -> Vec<Finding> {
    let mut engine = Engine::new();
    cliche::check_cliche_density(context, &mut engine);
    cliche::check_low_evidence(context, &mut engine);
    template_contrast::check_template_parallel(context, &mut engine);
    template_contrast::check_contrast_and_hedging(context, &mut engine);
    density_meta::check_wall_of_text(context, &mut engine);
    density_meta::check_dense_prose_section(context, &mut engine);
    density_meta::check_editorial_meta_labels(context, &mut engine);
    engine.into_findings()
}
