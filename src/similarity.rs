//! Similarity scoring for duplicate detection.
//!
//! Pure functions only — no DB access, so every rule here is unit-testable in
//! isolation. `db::find_similar_entries` supplies the document frequencies and
//! candidate rows; this module decides how similar two entries are and whether
//! that similarity is strong enough to refuse an add.
//!
//! # Why two independent signals instead of one blended score
//!
//! Title and keyword agreement fail in different ways, so blending them into a
//! single number makes misfires impossible to attribute. They are kept separate
//! and each gets its own threshold, which keeps `explain`-style output honest:
//! a hit can always be traced to the signal that produced it.
//!
//! # Why keyword overlap needs IDF
//!
//! Keywords are auto-extracted (up to `keywords::MAX_AUTO_KEYWORDS`) when the
//! caller supplies none, so generic terms like `test`, `main` or `update` end up
//! on a large fraction of entries. Some sharing is even deliberate — the
//! `conversation-log` convention tags every `context` entry. Raw overlap
//! therefore says almost nothing: in a real 48-entry base, a typical entry
//! shared at least one keyword with 24-38 others. Weighting each shared term by
//! inverse document frequency makes common terms cheap and rare terms decisive,
//! and it adapts to whatever vocabulary a given knowledge base drifts into
//! without a hand-maintained stop list.

use std::collections::{HashMap, HashSet};

/// Title similarity at or above this blocks the add even when the normalized
/// titles are not exactly equal.
///
/// Set this close to 1.0 on purpose: a block is the only outcome the caller
/// cannot recover from without `--force`.
///
/// A single character edit costs roughly three trigrams, so the band above 0.90
/// widens with title length — a long title may differ by one character, a short
/// one has to be all but exactly equal. Measured:
///
/// | Difference | Score | |
/// | --- | --- | --- |
/// | trailing punctuation only | 1.00 | block (exact after `norm`) |
/// | one character appended to a 29-char title | 0.96 | block |
/// | one-character typo in a 60-char title | 0.92 | block |
/// | one character appended to a 10-char title | 0.88 | warn |
/// | one-character typo in a 33-char title | 0.81 | warn |
/// | words reordered | 0.78 | warn |
/// | a real qualifier added ("… (follow-up)") | 0.67 | warn |
///
/// Meaningful edits stay below the line and only warn, which is the point.
///
/// Callers describing a block must therefore say "the same *or an all but
/// identical* title", not "the same title". [`reason`] keeps the two
/// distinguishable: [`Reason::SameTitle`] for exact equality after
/// normalization, [`Reason::SimilarTitle`] for this band.
pub const TITLE_BLOCK: f64 = 0.90;

/// Title similarity at or above this is reported as possibly related.
///
/// Centered in a measured plateau rather than tuned to a boundary. On a real
/// 49-entry base the best per-entry title score was 0.70 for the two genuinely
/// related entries and at most 0.39 for everything else, and any threshold in
/// `0.40..=0.70` produces an identical verdict on every entry. The noise at the
/// bottom of that band is titles sharing one generic word ("Sync Workflow" vs
/// "Export Workflow" vs "Release Workflow", 0.32-0.38), so a threshold just above
/// 0.39 would sit one hundredth above measured noise. 0.50 keeps margin on both
/// sides, which matters because the constant is a prior, not a fitted value:
/// the base contained no true accidental duplicate to calibrate against.
pub const TITLE_WARN: f64 = 0.50;

/// IDF-weighted keyword similarity at or above this is reported as possibly
/// related. Never blocks on its own — keyword sets collide for structural
/// reasons (see module docs) that no threshold can fully separate.
pub const KW_WARN: f64 = 0.60;

/// Keywords held by more than this fraction of all entries carry no weight at
/// all. IDF already decays them, but on a small base (N in the dozens) the decay
/// is gentle enough that a handful of near-universal tags can still add up, so
/// they are dropped outright rather than merely discounted.
///
/// On a nearly empty base the cap is aggressive: with three entries any keyword
/// on even one of them exceeds a quarter, so keyword similarity collapses to
/// zero and titles carry the decision alone. That is the right trade — there is
/// no vocabulary to speak of yet, so any keyword overlap would be noise.
pub const DF_RATIO_CAP: f64 = 0.25;

/// Keyword similarity against a `shared`-source entry is multiplied by this.
///
/// Entries imported from markdown via `lk sync` inherit the keyword set of the
/// whole source file, so every entry from one file carries identical keywords no
/// matter what it is about. Observed in a real base: two entries about entirely
/// different subjects ("Embedded Commands への新コマンド追加方法" and "Search
/// Logging 機能") shared all 26 of their keywords purely because they came from
/// the same file. Keyword agreement with such an entry is close to meaningless,
/// so it is halved rather than trusted.
pub const SHARED_KW_DAMPING: f64 = 0.5;

/// Category whose entries are never blocked — see [`classify`].
const APPEND_ONLY_CATEGORY: &str = "context";

/// How strongly a candidate matches, and what the caller should do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Not similar enough to mention.
    None,
    /// Report it, but complete the add. The overwhelming majority of real hits
    /// land here, and stopping on them is what made duplicate detection feel
    /// like it fired on everything.
    Warn,
    /// Refuse the add unless forced.
    Block,
}

/// Which signal produced a hit. Surfaced to callers so an agent (or a human)
/// can tell "you already wrote this" from "this is adjacent to that".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Identical after normalization.
    SameTitle,
    /// Title similarity carried the hit.
    SimilarTitle,
    /// Keyword agreement carried the hit.
    SimilarKeywords,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::SameTitle => "same-title",
            Reason::SimilarTitle => "similar-title",
            Reason::SimilarKeywords => "similar-keywords",
        }
    }
}

/// Fold a title down to the characters that carry meaning, so that formatting
/// differences never register as topical ones.
///
/// Full-width ASCII is folded to half-width and `U+3000` is treated as space,
/// then everything non-alphanumeric (spaces, punctuation, symbols) is dropped
/// and the rest lowercased. `char::is_alphanumeric` keeps kanji, kana and Latin
/// alike, so this works the same for Japanese and English titles.
///
/// Not full NFKC: half-width katakana is *not* folded to full-width (that needs
/// a voiced-mark composition table). The cost of that gap is bounded — such a
/// pair misses [`Tier::Block`] and lands in [`Tier::Warn`] instead, so the entry
/// is still added and still reported. Erring toward Warn is the safe direction.
pub fn norm(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            // Full-width ASCII (！..～) sits exactly 0xFEE0 above its half-width twin.
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Character trigrams of an already-normalized string.
///
/// Character n-grams rather than word tokens because titles here are routinely
/// Japanese, which has no whitespace to tokenize on. Trigrams also tolerate
/// reordering and partial rewording, where an edit distance would not.
///
/// Strings shorter than three characters have no trigram, so they are
/// represented by themselves: two such titles score 1.0 when equal and 0.0
/// otherwise. Exact equality is already handled by the caller, so the only
/// effect is that very short titles never match anything else — which is right,
/// since there is nothing left to compare.
fn trigrams(normalized: &str) -> HashSet<String> {
    let chars: Vec<char> = normalized.chars().collect();
    if chars.len() < 3 {
        return if chars.is_empty() {
            HashSet::new()
        } else {
            HashSet::from([normalized.to_string()])
        };
    }
    chars.windows(3).map(|w| w.iter().collect()).collect()
}

/// Jaccard overlap of the two titles' character trigrams, in `0.0..=1.0`.
pub fn title_sim(a: &str, b: &str) -> f64 {
    let (ta, tb) = (trigrams(&norm(a)), trigrams(&norm(b)));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = (ta.len() + tb.len()) as f64 - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Inverse document frequency of a keyword held by `df` of `n` entries.
///
/// Smoothed (`n + 1` over `df + 0.5`) because knowledge bases are small: at
/// N in the dozens an unsmoothed ratio makes single-occurrence keywords swing
/// wildly from one add to the next. `df == 0` — a keyword new to the base — is
/// legal and yields the largest weight, which correctly reads as evidence that
/// the incoming entry is novel.
pub fn idf(df: usize, n: usize) -> f64 {
    ((n as f64 + 1.0) / (df as f64 + 0.5)).ln().max(0.0)
}

/// IDF-weighted Jaccard similarity of two keyword sets.
///
/// `df` maps a keyword as the DB stores it — normalized, see `db::normalize_keyword` —
/// to the number of entries holding it, and `n` is the total entry count; both come from
/// the DB. Keywords above [`DF_RATIO_CAP`] are excluded from numerator and denominator
/// alike, so a pile of near-universal tags neither creates nor dilutes a match.
pub fn kw_sim(a: &[String], b: &[String], df: &HashMap<String, usize>, n: usize) -> f64 {
    let normalized = |ks: &[String]| -> HashSet<String> {
        // The same normalization the keywords are stored under: lowercasing alone
        // would read a decomposed spelling as a different keyword and report no
        // overlap where a user sees the same word.
        ks.iter().map(|k| crate::db::normalize_keyword(k)).collect()
    };
    let (sa, sb) = (normalized(a), normalized(b));
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }

    let mut shared_weight = 0.0;
    let mut union_weight = 0.0;
    for k in sa.union(&sb) {
        let d = df.get(k).copied().unwrap_or(0);
        if n > 0 && (d as f64 / n as f64) > DF_RATIO_CAP {
            continue;
        }
        let w = idf(d, n);
        union_weight += w;
        if sa.contains(k) && sb.contains(k) {
            shared_weight += w;
        }
    }

    if union_weight <= 0.0 {
        0.0
    } else {
        shared_weight / union_weight
    }
}

/// Decide the tier for one candidate.
///
/// `title_exact` is normalized-title equality, computed by the caller (which
/// already has both normalized forms in hand). `category` is the *incoming*
/// entry's category.
///
/// `context` entries are never blocked: they are append-only session logs, so
/// several genuinely distinct ones legitimately share a title and nearly all of
/// their keywords (the `conversation-log` convention guarantees the latter).
/// Refusing those would block the one category that is expected to accumulate.
pub fn classify(title_sim: f64, kw_sim: f64, title_exact: bool, category: &str) -> Tier {
    let blocking = title_exact || title_sim >= TITLE_BLOCK;
    if blocking && category != APPEND_ONLY_CATEGORY {
        return Tier::Block;
    }
    if blocking || title_sim >= TITLE_WARN || kw_sim >= KW_WARN {
        return Tier::Warn;
    }
    Tier::None
}

/// Which signal to credit for a hit, given the same inputs as [`classify`].
///
/// A blocking title score is always credited to the title, even when keyword
/// agreement happens to score higher: [`classify`] only ever blocks on the title,
/// and callers explain a block as "an entry with this title already exists". A
/// `similar-keywords` label on a refused add would contradict that message.
pub fn reason(title_sim: f64, kw_sim: f64, title_exact: bool) -> Reason {
    if title_exact {
        Reason::SameTitle
    } else if kw_sim >= KW_WARN && kw_sim > title_sim && title_sim < TITLE_BLOCK {
        Reason::SimilarKeywords
    } else {
        Reason::SimilarTitle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn df_of(pairs: &[(&str, usize)]) -> HashMap<String, usize> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    fn kws(ks: &[&str]) -> Vec<String> {
        ks.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn norm_folds_case_width_and_punctuation() {
        assert_eq!(norm("OAuth Flow"), norm("oauth  flow"));
        assert_eq!(norm("OAuth Flow"), norm("OAuth-Flow!"));
        // Full-width ASCII and the ideographic space fold to their ASCII twins.
        assert_eq!(norm("ＯＡｕｔｈ　Ｆｌｏｗ"), norm("OAuth Flow"));
        // Japanese survives intact apart from the dropped punctuation.
        assert_eq!(norm("重複判定の、改善"), "重複判定の改善");
    }

    #[test]
    fn title_sim_is_symmetric_and_bounded() {
        let a = "add の重複判定を改善する";
        let b = "重複判定の改善";
        let ab = title_sim(a, b);
        assert!((ab - title_sim(b, a)).abs() < f64::EPSILON, "symmetric");
        assert!((0.0..=1.0).contains(&ab), "bounded, got {ab}");
        assert_eq!(title_sim(a, a), 1.0);
    }

    #[test]
    fn title_sim_ranks_rewording_above_unrelated() {
        let base = "Duplicate detection threshold";
        let reworded = "Duplicate detection thresholds";
        let unrelated = "Homebrew formula release flow";
        assert!(
            title_sim(base, reworded) > title_sim(base, unrelated),
            "a near-rewording must outrank an unrelated title"
        );
        assert!(title_sim(base, unrelated) < TITLE_WARN);
    }

    #[test]
    fn idf_decays_with_document_frequency() {
        let n = 48;
        assert!(idf(1, n) > idf(10, n), "rare terms outweigh common ones");
        assert!(idf(0, n) > idf(1, n), "an unseen term is the strongest");
        assert!(idf(48, n) >= 0.0, "never negative");
    }

    #[test]
    fn kw_sim_drops_keywords_over_the_df_cap() {
        let n = 48;
        // At N=48 the cap excludes df > 12, so these two are weightless.
        let df = df_of(&[("knowledge", 14), ("commands", 13)]);
        assert_eq!(
            kw_sim(
                &kws(&["knowledge", "commands"]),
                &kws(&["knowledge", "commands"]),
                &df,
                n
            ),
            0.0,
            "keywords above the df cap must carry no weight, even when identical"
        );
    }

    /// The headline regression, in the shape it actually takes on real data.
    ///
    /// The df cap alone is not what saves us: on a 48-entry base it only excludes
    /// df > 12, so mid-frequency noise like `search` (12) or `main` (11) still
    /// scores. What defeats it is the IDF-weighted denominator — the keywords
    /// unique to each side are rare, so they dominate the union and crush the
    /// ratio. Two entries sharing nothing but generic terms land far below
    /// `KW_WARN`, which is precisely the case that used to flag every add.
    #[test]
    fn kw_sim_ignores_generic_only_overlap_on_realistic_keyword_sets() {
        let n = 48;
        let mut df = df_of(&[
            ("search", 12),
            ("main", 11),
            ("sync", 11),
            ("test", 11),
            ("cargo", 10),
        ]);
        // Ten topic-specific keywords per side, each unique to the base.
        let mut a = kws(&["search", "main", "sync", "test", "cargo"]);
        let mut b = a.clone();
        for i in 0..10 {
            let (ka, kb) = (format!("only_a_{i}"), format!("only_b_{i}"));
            df.insert(ka.clone(), 1);
            df.insert(kb.clone(), 1);
            a.push(ka);
            b.push(kb);
        }

        let score = kw_sim(&a, &b, &df, n);
        assert!(
            score < KW_WARN,
            "generic-only overlap must stay well under the warn threshold, got {score}"
        );
        assert_eq!(
            classify(0.1, score, false, ""),
            Tier::None,
            "generic-only overlap must not be reported at all"
        );
    }

    /// Honest limit of the metric: if *every* keyword on both sides is shared,
    /// Jaccard is 1.0 no matter how weak the terms are, because the union and the
    /// intersection are the same set. This is tolerable only because keyword
    /// agreement can never block (see `classify`) — the worst outcome is one
    /// spurious `possibly_related` line next to a successful add.
    #[test]
    fn kw_sim_saturates_on_identical_sets_but_cannot_block() {
        let n = 48;
        let df = df_of(&[("search", 12), ("main", 11)]);
        let identical = kws(&["search", "main"]);
        assert_eq!(kw_sim(&identical, &identical, &df, n), 1.0);
        assert_eq!(
            classify(0.0, 1.0, false, ""),
            Tier::Warn,
            "a saturated keyword score must still only warn"
        );
    }

    #[test]
    fn kw_sim_rewards_rare_shared_keywords() {
        let n = 48;
        let df = df_of(&[("find_similar_entries", 1), ("idf", 1), ("update", 13)]);
        let shared_rare = kw_sim(
            &kws(&["find_similar_entries", "idf"]),
            &kws(&["find_similar_entries", "idf"]),
            &df,
            n,
        );
        assert!(
            shared_rare >= KW_WARN,
            "two rare keywords in common is a real signal, got {shared_rare}"
        );
    }

    #[test]
    fn kw_sim_is_diluted_by_novel_keywords() {
        let n = 48;
        let df = df_of(&[("oauth", 1), ("pkce", 1), ("homebrew", 1), ("cron", 1)]);
        let one_in_common = kw_sim(
            &kws(&["oauth", "pkce"]),
            &kws(&["oauth", "homebrew", "cron"]),
            &df,
            n,
        );
        let all_in_common = kw_sim(&kws(&["oauth", "pkce"]), &kws(&["oauth", "pkce"]), &df, n);
        assert!(
            one_in_common < all_in_common,
            "keywords unique to one side must lower the score"
        );
        assert!(one_in_common < KW_WARN, "got {one_in_common}");
    }

    #[test]
    fn empty_keyword_sets_never_match() {
        let df = df_of(&[("oauth", 1)]);
        assert_eq!(kw_sim(&[], &kws(&["oauth"]), &df, 48), 0.0);
        assert_eq!(kw_sim(&kws(&["oauth"]), &[], &df, 48), 0.0);
    }

    #[test]
    fn classify_blocks_only_on_near_identical_titles() {
        assert_eq!(classify(1.0, 0.0, true, ""), Tier::Block);
        assert_eq!(classify(0.95, 0.0, false, ""), Tier::Block);
        // Just under the block threshold degrades to a warning, not a refusal.
        assert_eq!(classify(0.89, 0.0, false, ""), Tier::Warn);
        assert_eq!(classify(TITLE_WARN, 0.0, false, ""), Tier::Warn);
        // Titles sharing only a generic word land here and must stay silent.
        assert_eq!(classify(0.39, 0.0, false, ""), Tier::None);
        assert_eq!(classify(0.39, KW_WARN - 0.01, false, ""), Tier::None);
        // Keyword agreement alone never blocks, however strong.
        assert_eq!(classify(0.0, 1.0, false, ""), Tier::Warn);
    }

    #[test]
    fn classify_never_blocks_append_only_context_entries() {
        assert_eq!(
            classify(1.0, 1.0, true, "context"),
            Tier::Warn,
            "session logs legitimately repeat titles and keywords"
        );
    }

    #[test]
    fn reason_credits_the_dominant_signal() {
        assert_eq!(reason(1.0, 1.0, true), Reason::SameTitle);
        assert_eq!(reason(0.6, 0.1, false), Reason::SimilarTitle);
        assert_eq!(reason(0.1, 0.8, false), Reason::SimilarKeywords);
    }

    /// A blocking hit is always explained by the title, since that is the only
    /// thing that blocks — even when the keyword score is the larger number.
    #[test]
    fn reason_credits_the_title_whenever_the_title_blocks() {
        assert_eq!(classify(0.92, 1.0, false, ""), Tier::Block);
        assert_eq!(reason(0.92, 1.0, false), Reason::SimilarTitle);
    }

    /// Pins the block/warn boundary to concrete title pairs, so the table in
    /// `TITLE_BLOCK`'s docs cannot drift away from what the code does. A single
    /// character edit costs ~3 trigrams, so tolerance grows with title length:
    /// only near-exact repeats are refused, and every meaningful edit warns.
    #[test]
    fn title_block_band_holds_only_all_but_identical_titles() {
        let blocks = [
            ("Release Workflow", "Release Workflow."),
            (
                "Duplicate detection threshold",
                "Duplicate detection thresholds",
            ),
            (
                "ADR: user-scope markdown export and sync for dotfiles workflow",
                "ADR: user-scope markdown export and sync for dotfiles workflaw",
            ),
        ];
        for (a, b) in blocks {
            let s = title_sim(a, b);
            assert!(
                s >= TITLE_BLOCK,
                "{a:?} vs {b:?} scored {s:.3}, expected block"
            );
        }

        let warns = [
            ("OAuth Flow", "OAuth Flows"),
            (
                "ADR: user-scope markdown export/sync",
                "ADR: user-scope markdown export/sinc",
            ),
            ("Export Workflow Sync", "Sync Export Workflow"),
            ("export group name bug", "export group name bug (follow-up)"),
        ];
        for (a, b) in warns {
            let s = title_sim(a, b);
            assert!(
                (TITLE_WARN..TITLE_BLOCK).contains(&s),
                "{a:?} vs {b:?} scored {s:.3}, expected warn"
            );
        }
    }
}
