//! Rule modules. Each module owns one diagnostic family and reports through the shared engine.

pub(crate) mod charts;
pub(crate) mod citations;
pub(crate) mod core;
pub(crate) mod prose;
pub(crate) mod structure;
pub(crate) mod style;
pub(crate) mod tables;

use crate::engine::Engine;

pub(crate) fn run(engine: &mut Engine<'_>) {
    let document = engine.document;
    core::check(document, engine);
    citations::check(document, engine);
    tables::check(document, engine);
    charts::check(document, engine);
    structure::check(document, engine);
    prose::check(document, engine);
    style::check(document, engine);
}
