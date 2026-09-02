//! Short excerpts of VibeSSH's own documentation, selected by topic.
//!
//! **What this deliberately is not.** No vector store, no embeddings, no
//! chunk-overlap tuning - that was ruled out for this version, and rightly:
//! the corpus is four documents that ship inside the binary and change only
//! when someone edits them in this repository. Keyword scoring over
//! heading-delimited sections answers "what does VibeSSH call this, and
//! what does its manual say about it" well enough to be useful, and it
//! costs no index, no model and no network.
//!
//! **What it is designed for.** `KnowledgeSource` is a trait with one
//! method, and `ai_service` holds a `&dyn KnowledgeSource`. Replacing this
//! with an embeddings-backed implementation later is a new type in this
//! module and one line where the service is constructed - no caller
//! changes, because no caller knows how a snippet was found.
//!
//! The documents are embedded with `include_str!` rather than read from
//! disk, because a packaged desktop app does not ship its repository. That
//! also means the corpus cannot drift from the build: an answer citing a
//! behaviour is citing the manual for the binary that is running.

/// One retrieved passage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSnippet {
    /// The heading this passage sits under.
    pub title: String,
    /// Which document it came from, so the model can attribute it and the
    /// user can go and read the rest.
    pub source: String,
    pub body: String,
}

impl KnowledgeSnippet {
    /// How the snippet is rendered into the prompt.
    pub fn to_prompt_text(&self) -> String {
        format!("[{} - {}]\n{}", self.source, self.title, self.body)
    }
}

/// Where documentation passages come from.
pub trait KnowledgeSource: Send + Sync {
    /// The `limit` passages most relevant to `query`, best first. An empty
    /// result is normal and is not an error - it means "nothing in the
    /// manual is about this", which the system prompt already tells the
    /// model how to handle.
    fn search(&self, query: &str, limit: usize) -> Vec<KnowledgeSnippet>;
}

/// The longest passage worth attaching. Past this a section stops being an
/// excerpt and starts being most of the prompt, crowding out the collected
/// context, which is the more specific and more useful of the two.
const MAX_SNIPPET_CHARS: usize = 1200;

/// Words carrying no topical signal, in both languages this app ships. A
/// query is mostly these, and scoring on them makes every section look
/// equally relevant.
const STOPWORDS: &[&str] = &[
    // English
    "the", "and", "for", "with", "why", "how", "what", "does", "not", "are", "was", "can", "you", "this", "that", "from", "have", "has",
    "but", "its", "it's", "when", "where", "there", "then", "than", "get", "got", "any", "all", "into", "out", "off", "own",
    // Polish
    "nie", "jest", "sie", "się", "jak", "czy", "dla", "ale", "tak", "juz", "już", "moze", "może", "gdzie", "kiedy", "dlaczego", "mam",
    "mi", "to", "co", "na", "do", "za", "od", "po", "we", "przy", "oraz", "lub", "albo", "bo", "ten", "ta", "te", "tego", "jego",
];

/// Polish domain words mapped onto the English term the documentation
/// actually uses.
///
/// A stopgap, and an honest one: the manual is written in English and the
/// user interface is available in Polish, so a Polish question would
/// otherwise score zero against every section and silently attach nothing.
/// Only the vocabulary of this app's own domain is here - this is not a
/// translator, and it is the first thing an embeddings-backed
/// implementation would make unnecessary.
const POLISH_TERMS: &[(&str, &str)] = &[
    ("aplikacja", "application"),
    ("aplikacji", "application"),
    ("aplikacje", "application"),
    ("kontener", "container"),
    ("kontenera", "container"),
    ("serwer", "server"),
    ("serwera", "server"),
    ("siec", "network"),
    ("sieć", "network"),
    ("sieci", "network"),
    ("zapora", "firewall"),
    ("zapory", "firewall"),
    ("baza", "database"),
    ("bazy", "database"),
    ("bazie", "database"),
    ("dysk", "disk"),
    ("dysku", "disk"),
    ("pamiec", "memory"),
    ("pamięć", "memory"),
    ("pamieci", "memory"),
    ("pamięci", "memory"),
    ("blad", "error"),
    ("błąd", "error"),
    ("bledy", "error"),
    ("błędy", "error"),
    ("kopia", "backup"),
    ("kopie", "backup"),
    ("zapasowa", "backup"),
    ("uprawnienia", "permissions"),
    ("haslo", "password"),
    ("hasło", "password"),
    ("plik", "file"),
    ("pliki", "file"),
    ("logi", "logs"),
    ("dziennik", "logs"),
    ("port", "port"),
    ("porty", "port"),
    ("obraz", "blueprint"),
    ("obrazy", "blueprint"),
    ("szablon", "blueprint"),
    ("wezel", "node"),
    ("węzeł", "node"),
    ("wezla", "node"),
    ("węzła", "node"),
    ("uruchomic", "start"),
    ("uruchomić", "start"),
    ("startuje", "start"),
    ("polaczenie", "connection"),
    ("połączenie", "connection"),
];

#[derive(Debug, Clone)]
struct Section {
    title: String,
    source: &'static str,
    body: String,
    /// The section's own text, lowercased once at construction. Scoring
    /// runs on every section for every query, and re-lowercasing 150KB of
    /// markdown per keystroke-sized query would be the one genuinely
    /// wasteful thing in an otherwise cheap design.
    haystack: String,
}

/// Keyword scoring over the documents that ship with the app.
pub struct KeywordKnowledgeService {
    sections: Vec<Section>,
}

impl KeywordKnowledgeService {
    /// The documents chosen for this: what a user's question is actually
    /// likely to be about.
    ///
    /// `AUDIT_REPORT.md` and `FIX_PLAN.md` are deliberately absent. They are
    /// internal remediation records - a list of this app's own past
    /// vulnerabilities, half of them phrased as things that once worked -
    /// and feeding them to an inference endpoint would be both a disclosure
    /// and a reliable way to have the model describe fixed bugs as current
    /// behaviour.
    pub fn with_builtin_docs() -> Self {
        let documents: &[(&'static str, &'static str)] = &[
            // The in-app guide, in both languages it is written in.
            //
            // The same files the Guide page renders (`src/guide/guideDocs.ts`
            // loads this directory), so an answer here and a page there
            // cannot describe the app differently - there is one description
            // and two readers of it. Both languages are included on purpose:
            // scoring is by keyword, so a question asked in Polish matches
            // the Polish text without anything having to translate anything.
            //
            // A new topic is two files and two lines here.
            // `every_guide_document_is_in_the_corpus` fails if the second
            // pair is forgotten.
            ("Guide - Applications", include_str!("../../../docs/guide/applications.en.md")),
            ("Poradnik - Aplikacje", include_str!("../../../docs/guide/applications.pl.md")),
            ("Guide - Application files", include_str!("../../../docs/guide/application-files.en.md")),
            ("Poradnik - Pliki aplikacji", include_str!("../../../docs/guide/application-files.pl.md")),
            ("Guide - Ports", include_str!("../../../docs/guide/ports.en.md")),
            ("Poradnik - Porty", include_str!("../../../docs/guide/ports.pl.md")),
            ("Guide - Vibe Network", include_str!("../../../docs/guide/vibe-network.en.md")),
            ("Poradnik - Vibe Network", include_str!("../../../docs/guide/vibe-network.pl.md")),
            ("Guide - Backups", include_str!("../../../docs/guide/backups.en.md")),
            ("Poradnik - Backupy", include_str!("../../../docs/guide/backups.pl.md")),
            ("Applications architecture", include_str!("../../../docs/APPLICATIONS_ARCHITECTURE.md")),
            ("Threat model", include_str!("../../../docs/threat-model.md")),
            ("Agent privileges", include_str!("../../../docs/agent-privileges.md")),
            ("README", include_str!("../../../README.md")),
        ];
        let mut sections = Vec::new();
        for (source, text) in documents {
            sections.extend(split_into_sections(source, text));
        }
        Self { sections }
    }
}

/// Splits a markdown document on its headings.
///
/// Any heading level delimits, not just `##`: the documents here are not
/// consistent about depth, and a passage under an `###` is exactly the
/// granularity this wants. Content before the first heading is kept under
/// the document's own name rather than dropped, because that is where a
/// README puts its summary.
fn split_into_sections(source: &'static str, text: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut title = source.to_string();
    let mut body = String::new();

    let flush = |title: &str, body: &mut String, sections: &mut Vec<Section>| {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            body.clear();
            return;
        }
        let truncated: String = trimmed.chars().take(MAX_SNIPPET_CHARS).collect();
        let truncated = if trimmed.chars().count() > MAX_SNIPPET_CHARS { format!("{truncated}...") } else { truncated };
        sections.push(Section {
            title: title.to_string(),
            source,
            haystack: format!("{title}\n{truncated}").to_lowercase(),
            body: truncated,
        });
        body.clear();
    };

    for line in text.lines() {
        if let Some(heading) = line.trim_start().strip_prefix('#') {
            flush(&title, &mut body, &mut sections);
            title = heading.trim_start_matches('#').trim().to_string();
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    flush(&title, &mut body, &mut sections);
    sections
}

/// Query text to the terms worth scoring on.
///
/// Short tokens go because they are noise; stopwords go for the same
/// reason; Polish domain words are additionally mapped to their English
/// equivalent *in addition to* being kept, so a mixed-language question
/// ("czemu mój Node nie startuje") scores on both halves.
fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for raw in query.split(|c: char| !c.is_alphanumeric()) {
        let word = raw.to_lowercase();
        if word.chars().count() < 3 || STOPWORDS.contains(&word.as_str()) {
            continue;
        }
        if let Some((_, english)) = POLISH_TERMS.iter().find(|(polish, _)| *polish == word) {
            let english = english.to_string();
            if !terms.contains(&english) {
                terms.push(english);
            }
        }
        if !terms.contains(&word) {
            terms.push(word);
        }
    }
    terms
}

impl KnowledgeSource for KeywordKnowledgeService {
    fn search(&self, query: &str, limit: usize) -> Vec<KnowledgeSnippet> {
        let terms = query_terms(query);
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }

        let mut scored: Vec<(usize, &Section)> = self
            .sections
            .iter()
            .filter_map(|section| {
                let title = section.title.to_lowercase();
                let mut score = 0;
                let mut matched = 0;
                for term in &terms {
                    // Capped per term, so one section that happens to
                    // repeat a common word forty times cannot outrank a
                    // section that is actually about the whole question.
                    let hits = section.haystack.matches(term.as_str()).count().min(4);
                    if hits > 0 {
                        matched += 1;
                        score += hits;
                    }
                    if title.contains(term.as_str()) {
                        score += 5;
                    }
                }
                // Covering more of the question beats mentioning one word
                // of it often - the difference between the section about
                // ports on Applications and a section that says "port".
                (matched > 0).then_some((score + matched * 3, section))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, section)| KnowledgeSnippet {
                title: section.title.clone(),
                source: section.source.to_string(),
                body: section.body.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_corpus_actually_loaded() {
        let service = KeywordKnowledgeService::with_builtin_docs();
        assert!(service.sections.len() > 20, "expected the shipped docs to split into many sections");
        assert!(service.sections.iter().all(|s| !s.body.is_empty()));
    }

    #[test]
    fn no_section_exceeds_the_snippet_ceiling() {
        let service = KeywordKnowledgeService::with_builtin_docs();
        for section in &service.sections {
            // The ellipsis added on truncation is the one extra character.
            assert!(section.body.chars().count() <= MAX_SNIPPET_CHARS + 3, "{} is too long", section.title);
        }
    }

    #[test]
    fn a_question_about_ports_finds_something_about_ports() {
        let service = KeywordKnowledgeService::with_builtin_docs();
        let hits = service.search("application port is already in use", 3);
        assert!(!hits.is_empty());
        let text = hits.iter().map(|h| h.to_prompt_text()).collect::<String>().to_lowercase();
        assert!(text.contains("port"));
    }

    /// The reason `POLISH_TERMS` exists: this app's interface is available
    /// in Polish and its manual is not.
    #[test]
    fn a_polish_question_still_reaches_the_english_manual() {
        let service = KeywordKnowledgeService::with_builtin_docs();
        let hits = service.search("dlaczego moja aplikacja nie startuje", 3);
        assert!(!hits.is_empty(), "a Polish question found nothing at all");
    }

    #[test]
    fn a_query_of_nothing_but_noise_returns_nothing_rather_than_everything() {
        let service = KeywordKnowledgeService::with_builtin_docs();
        assert!(service.search("", 3).is_empty());
        assert!(service.search("a to i", 3).is_empty());
        assert!(service.search("czy jak to", 3).is_empty());
    }

    #[test]
    fn the_limit_is_respected() {
        let service = KeywordKnowledgeService::with_builtin_docs();
        assert!(service.search("application docker network port", 2).len() <= 2);
        assert!(service.search("application docker network port", 0).is_empty());
    }

    /// The internal remediation documents must never be reachable - see
    /// `with_builtin_docs`'s own doc comment for why.
    #[test]
    fn the_audit_and_fix_plan_are_not_part_of_the_corpus() {
        let service = KeywordKnowledgeService::with_builtin_docs();
        assert!(service.sections.iter().all(|s| s.source != "AUDIT_REPORT" && s.source != "FIX_PLAN"));
        let hits = service.search("S-018 critical finding remediation audit", 5);
        assert!(hits.iter().all(|h| h.source != "AUDIT_REPORT"));
    }

    #[test]
    fn headings_become_titles_and_preamble_keeps_the_document_name() {
        let sections = split_into_sections("Doc", "intro text\n\n## Ports\nabout ports\n\n### Visibility\nabout visibility\n");
        assert_eq!(sections[0].title, "Doc");
        assert_eq!(sections[0].body, "intro text");
        assert_eq!(sections[1].title, "Ports");
        assert_eq!(sections[2].title, "Visibility");
    }
}

#[cfg(test)]
mod guide_corpus_tests {
    use super::*;

    /// Every guide topic reaches the assistant.
    ///
    /// The Guide page loads `docs/guide` as a directory, so adding a file is
    /// all it takes there. The corpus needs an `include_str!` per file,
    /// because a packaged binary has no repository to read from - which
    /// means the two can drift, and the failure would be silent: the page
    /// documents a feature and the assistant has never heard of it.
    ///
    /// Matched on each file's own `title:` line, which is unique per file
    /// (the two languages of a topic have different titles) and sits in the
    /// first section's body, ahead of any truncation.
    #[test]
    fn every_guide_document_is_in_the_corpus() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/guide");
        let service = KeywordKnowledgeService::with_builtin_docs();
        let corpus: String = service.sections.iter().map(|section| section.body.as_str()).collect::<Vec<_>>().join("\n");

        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("docs/guide should exist") {
            let path = entry.expect("readable entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable guide file");
            let title = text
                .lines()
                .find(|line| line.starts_with("title:"))
                .unwrap_or_else(|| panic!("{} has no `title:` in its frontmatter", path.display()));
            assert!(
                corpus.contains(title),
                "{} is not in the AI corpus - add an `include_str!` for it in `with_builtin_docs`",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "no guide documents were found to check");
    }
}
