//! Central rule registry: one source of truth for codes, names, severities and ignore policy.

use quire_protocol::Severity;

/// Stable identifier for one static lint rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RuleId {
    DuplicateBlockId,
    BlockIdUuidV4,
    HeadingLevelRange,
    TableSchemaInvalid,
    TableUnknownRowKey,
    TableUnknownWidthOverride,
    DanglingSourceId,
    UnusedSource,
    NumericClaimWithoutCitation,
    DuplicateSourceUrl,
    SourceTitleEmpty,
    SourceUrlUnsafe,
    MalformedCitationSyntax,
    LowAuthoritySourceDomain,
    TableColumnTooNarrow,
    TableEmptyColumn,
    TableConstantColumn,
    TableTooWideForA4,
    TableNumericUnitInconsistent,
    TableSummaryRowMismatch,
    EchartsDataMissing,
    EchartsJsFunction,
    EchartsOptionMissing,
    EchartsOptionNotObject,
    EchartsSeriesTypeInvalid,
    EchartsIndexOutOfRange,
    EchartsCoordinateMissing,
    EchartsMatrixMissing,
    EchartsFallbackEmpty,
    EchartsHeightRange,
    EchartsDuplicateInnerTitle,
    EchartsInnerTitleStripped,
    EchartsToolboxStripped,
    EchartsEntityColorInconsistent,
    EchartsPaletteInconsistent,
    EchartsDenseLabelsOverlap,
    EchartsLineEndLabelStyle,
    EchartsIntegerAxisMinInterval,
    PlaceholderTextLeak,
    HeadingLevelJump,
    HeadingEmptySection,
    HeadingNumberingMismatch,
    DanglingVisualRef,
    UnclosedInlineSyntax,
    EmptyTextBlock,
    EmptyHeading,
    EmptyDocument,
    ManualTitleNumbering,
    ClichePhraseDensity,
    LowEvidenceProse,
    TemplateParallelProse,
    ContrastAndHedgingTics,
    ConsecutiveVisualsWithoutProse,
    WallOfTextParagraph,
    BoringOrCountingHeading,
    DenseProseRunNeedsVisual,
    DuplicateNotePrefix,
    BlindOrdinalOrAcademicJargon,
    VisualMissingUnitOrScopeNote,
    HigherEdPolicyTermFormat,
    InternalIdentifierLeak,
    CssSyntaxError,
    CustomHtmlExternalResource,
    CustomHtmlFixedOverflow,
    DuplicateHtmlTitle,
    CustomHtmlFallbackEmpty,
    CustomHtmlEmpty,
    ThemeTokenName,
    ThemeTokenValue,
    ThemePresetEmpty,
    TocDepthRange,
    TocTitleEmpty,
    CoverTitleEmpty,
    CoverVariantEmpty,
}

/// Registry metadata for one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleMeta {
    pub id: RuleId,
    pub code: &'static str,
    pub name: &'static str,
    pub default_severity: Severity,
    pub suppressible: bool,
}

macro_rules! define_rules {
    ($( $variant:ident = $code:literal, $name:literal, $severity:ident, $suppressible:literal; )+) => {
        $(
            // Const names intentionally mirror enum variants inside the registry macro.
            #[allow(non_upper_case_globals)]
            const $variant: RuleMeta = RuleMeta {
                id: RuleId::$variant,
                code: $code,
                name: $name,
                default_severity: Severity::$severity,
                suppressible: $suppressible,
            };
        )+

        /// Every registered rule in code order.
        pub const RULES: &[RuleMeta] = &[$($variant),+];

        impl RuleId {
            /// Metadata for this rule.
            pub const fn meta(self) -> &'static RuleMeta {
                match self {
                    $(Self::$variant => &$variant),+
                }
            }

            /// Stable `QRxxx` code.
            pub const fn code(self) -> &'static str {
                self.meta().code
            }

            /// Kebab-case semantic name.
            pub const fn name(self) -> &'static str {
                self.meta().name
            }
        }
    };
}

define_rules! {
    DuplicateBlockId = "QR001", "duplicate-block-id", Error, false;
    BlockIdUuidV4 = "QR003", "block-id-uuid-v4", Error, false;
    HeadingLevelRange = "QR004", "heading-level-range", Error, false;
    TableSchemaInvalid = "QR005", "table-schema-invalid", Error, false;
    TableUnknownRowKey = "QR006", "table-unknown-row-key", Warning, false;
    TableUnknownWidthOverride = "QR007", "table-unknown-width-override", Warning, false;

    DanglingSourceId = "QR101", "dangling-source-id", Error, false;
    UnusedSource = "QR102", "unused-source", Warning, true;
    NumericClaimWithoutCitation = "QR103", "numeric-claim-without-citation", Info, true;
    DuplicateSourceUrl = "QR104", "duplicate-source-url", Warning, true;
    SourceTitleEmpty = "QR105", "source-title-empty", Warning, true;
    SourceUrlUnsafe = "QR106", "source-url-unsafe", Warning, true;
    MalformedCitationSyntax = "QR107", "malformed-citation-syntax", Error, false;
    LowAuthoritySourceDomain = "QR108", "low-authority-source-domain", Warning, true;

    TableColumnTooNarrow = "QR201", "table-column-too-narrow", Warning, true;
    TableEmptyColumn = "QR202", "table-empty-column", Warning, true;
    TableConstantColumn = "QR203", "table-constant-column", Warning, true;
    TableTooWideForA4 = "QR204", "table-too-wide-for-a4", Warning, true;
    TableNumericUnitInconsistent = "QR205", "table-numeric-unit-inconsistent", Warning, true;
    TableSummaryRowMismatch = "QR206", "table-summary-row-mismatch", Warning, true;

    EchartsDataMissing = "QR301", "echarts-data-missing", Error, false;
    EchartsJsFunction = "QR302", "echarts-js-function", Error, false;
    EchartsOptionMissing = "QR303", "echarts-option-missing", Error, false;
    EchartsOptionNotObject = "QR304", "echarts-option-not-object", Error, false;
    EchartsSeriesTypeInvalid = "QR305", "echarts-series-type-invalid", Error, true;
    EchartsIndexOutOfRange = "QR306", "echarts-index-out-of-range", Error, true;
    EchartsCoordinateMissing = "QR307", "echarts-coordinate-missing", Error, true;
    EchartsMatrixMissing = "QR308", "echarts-matrix-missing", Error, true;
    EchartsFallbackEmpty = "QR309", "echarts-fallback-empty", Error, true;
    EchartsHeightRange = "QR310", "echarts-height-range", Warning, true;
    EchartsDuplicateInnerTitle = "QR311", "echarts-duplicate-inner-title", Warning, true;
    EchartsInnerTitleStripped = "QR312", "echarts-inner-title-stripped", Warning, true;
    EchartsToolboxStripped = "QR313", "echarts-toolbox-stripped", Warning, true;
    EchartsEntityColorInconsistent = "QR314", "echarts-entity-color-inconsistent", Warning, true;
    EchartsPaletteInconsistent = "QR315", "echarts-palette-inconsistent", Warning, true;
    EchartsDenseLabelsOverlap = "QR316", "echarts-dense-labels-overlap", Warning, true;
    EchartsLineEndLabelStyle = "QR317", "echarts-line-end-label-style", Warning, true;
    EchartsIntegerAxisMinInterval = "QR318", "echarts-integer-axis-min-interval", Warning, true;

    PlaceholderTextLeak = "QR401", "placeholder-text-leak", Error, true;
    HeadingLevelJump = "QR402", "heading-level-jump", Warning, true;
    HeadingEmptySection = "QR403", "heading-empty-section", Warning, true;
    HeadingNumberingMismatch = "QR404", "heading-numbering-mismatch", Warning, true;
    DanglingVisualRef = "QR406", "dangling-visual-ref", Warning, true;
    UnclosedInlineSyntax = "QR407", "unclosed-inline-syntax", Warning, true;
    EmptyTextBlock = "QR408", "empty-text-block", Warning, true;
    EmptyHeading = "QR409", "empty-heading", Warning, true;
    EmptyDocument = "QR410", "empty-document", Warning, true;
    ManualTitleNumbering = "QR411", "manual-title-numbering", Warning, true;
    ClichePhraseDensity = "QR412", "cliche-phrase-density", Warning, true;
    LowEvidenceProse = "QR413", "low-evidence-prose", Info, true;
    TemplateParallelProse = "QR414", "template-parallel-prose", Warning, true;
    ContrastAndHedgingTics = "QR415", "contrast-and-hedging-tics", Warning, true;
    ConsecutiveVisualsWithoutProse = "QR416", "consecutive-visuals-without-prose", Warning, true;
    WallOfTextParagraph = "QR417", "wall-of-text-paragraph", Warning, true;
    BoringOrCountingHeading = "QR418", "boring-or-counting-heading", Warning, true;
    DenseProseRunNeedsVisual = "QR419", "dense-prose-run-needs-visual", Warning, true;
    DuplicateNotePrefix = "QR420", "duplicate-note-prefix", Warning, true;
    BlindOrdinalOrAcademicJargon = "QR421", "blind-ordinal-or-academic-jargon", Warning, true;
    VisualMissingUnitOrScopeNote = "QR422", "visual-missing-unit-or-scope-note", Info, true;
    HigherEdPolicyTermFormat = "QR423", "higher-ed-policy-term-format", Warning, true;
    InternalIdentifierLeak = "QR424", "internal-identifier-leak", Error, false;

    CssSyntaxError = "QR501", "css-syntax-error", Error, false;
    CustomHtmlExternalResource = "QR502", "custom-html-external-resource", Warning, true;
    CustomHtmlFixedOverflow = "QR503", "custom-html-fixed-overflow", Warning, true;
    DuplicateHtmlTitle = "QR504", "duplicate-html-title", Warning, true;
    CustomHtmlFallbackEmpty = "QR505", "custom-html-fallback-empty", Error, true;
    CustomHtmlEmpty = "QR506", "custom-html-empty", Warning, true;
    ThemeTokenName = "QR507", "theme-token-name", Warning, true;
    ThemeTokenValue = "QR508", "theme-token-value", Warning, true;
    ThemePresetEmpty = "QR509", "theme-preset-empty", Warning, true;
    TocDepthRange = "QR510", "toc-depth-range", Error, true;
    TocTitleEmpty = "QR511", "toc-title-empty", Warning, true;
    CoverTitleEmpty = "QR512", "cover-title-empty", Warning, true;
    CoverVariantEmpty = "QR513", "cover-variant-empty", Warning, true;
}

impl RuleId {
    /// All registered rules in code order.
    pub const fn all() -> &'static [RuleMeta] {
        RULES
    }
}

/// Match one user-supplied token against a rule. Tokens accept an exact code (`QR202`),
/// an exact semantic name (`table-empty-column`) or a code category prefix (`QR2`).
pub fn rule_matches_token(meta: &RuleMeta, token: &str) -> bool {
    let token = token.trim().to_ascii_lowercase();
    if token.is_empty() {
        return false;
    }
    let code = meta.code.to_ascii_lowercase();
    let name = meta.name.to_ascii_lowercase();
    token == code || token == name || (token.starts_with("qr") && code.starts_with(&token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_and_names_are_unique_and_well_formed() {
        let mut codes = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut previous_code = "";
        for rule in RULES {
            assert!(rule.code.starts_with("QR"), "{}", rule.code);
            assert_eq!(rule.code.len(), 5, "{}", rule.code);
            assert!(rule.code[2..].chars().all(|ch| ch.is_ascii_digit()));
            assert!(codes.insert(rule.code), "duplicate code {}", rule.code);
            assert!(names.insert(rule.name), "duplicate name {}", rule.name);
            assert!(
                rule.name
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'),
                "{}",
                rule.name
            );
            assert!(rule.code > previous_code, "registry must stay code-sorted");
            previous_code = rule.code;
            assert_eq!(rule.id.meta().code, rule.code);
            assert_eq!(rule.id.meta().name, rule.name);
        }
    }

    #[test]
    fn core_and_safety_rules_are_not_suppressible() {
        for code in [
            "QR001", "QR003", "QR004", "QR005", "QR006", "QR007", "QR101", "QR107", "QR301",
            "QR302", "QR303", "QR304", "QR501",
        ] {
            let rule = RULES.iter().find(|rule| rule.code == code);
            assert!(
                rule.is_some_and(|rule| !rule.suppressible),
                "{code} must be registered and not suppressible"
            );
        }
    }

    #[test]
    fn retired_rule_codes_are_not_registered() {
        for code in ["QR002", "QR008", "QR405"] {
            assert!(
                !RULES.iter().any(|rule| rule.code == code),
                "retired code {code} must not be reused"
            );
        }
    }

    #[test]
    fn token_matching_supports_code_name_and_prefix() {
        let empty_column = RuleId::TableEmptyColumn.meta();
        assert!(rule_matches_token(empty_column, "QR202"));
        assert!(rule_matches_token(empty_column, "qr202"));
        assert!(rule_matches_token(empty_column, "table-empty-column"));
        assert!(rule_matches_token(empty_column, "QR2"));
        assert!(rule_matches_token(empty_column, "QR20"));
        assert!(!rule_matches_token(empty_column, "QR3"));
        assert!(!rule_matches_token(empty_column, "table-column-too-narrow"));
        assert!(!rule_matches_token(empty_column, ""));
    }
}
