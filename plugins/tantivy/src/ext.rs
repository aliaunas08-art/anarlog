use tantivy::collector::{Count, TopDocs};
use tantivy::indexer::IndexWriterOptions;
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, PhraseQuery, Query, QueryParser,
    TermQuery,
};
use tantivy::schema::{Facet, IndexRecordOption};
use tantivy::snippet::SnippetGenerator;
use tantivy::{Index, ReloadPolicy, TantivyDocument, Term};
use tauri_plugin_settings::SettingsPluginExt;

use crate::query::build_created_at_range_query;
use crate::schema::{
    extract_search_document, get_fields, to_tantivy_document, workspace_term_value,
};
use crate::tokenizer::register_tokenizers;
use crate::{
    CollectionConfig, CollectionIndex, HighlightRange, IndexState, SearchDocument, SearchHit,
    SearchRequest, SearchResult, Snippet,
};

fn create_index_writer(index: &Index) -> tantivy::Result<tantivy::IndexWriter> {
    index.writer_with_options(
        IndexWriterOptions::builder()
            .num_worker_threads(1)
            .num_merge_threads(1)
            .build(),
    )
}

pub fn detect_language(text: &str) -> anlg_language::Language {
    anlg_language::detect(text)
}

fn parse_query_parts(query: &str) -> (Vec<&str>, Vec<&str>) {
    let mut phrases = Vec::new();
    let mut regular_terms = Vec::new();
    let mut in_quote = false;
    let mut quote_start = 0;
    let mut current_start = 0;

    let chars: Vec<char> = query.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '"' {
            if in_quote {
                let phrase = &query[quote_start..i];
                if !phrase.trim().is_empty() {
                    phrases.push(phrase.trim());
                }
                in_quote = false;
                current_start = i + 1;
            } else {
                let before = &query[current_start..i];
                for term in before.split_whitespace() {
                    if !term.is_empty() {
                        regular_terms.push(term);
                    }
                }
                in_quote = true;
                quote_start = i + 1;
            }
        }
        i += 1;
    }

    if in_quote {
        let phrase = &query[quote_start..];
        if !phrase.trim().is_empty() {
            phrases.push(phrase.trim());
        }
    } else {
        let remaining = &query[current_start..];
        for term in remaining.split_whitespace() {
            if !term.is_empty() {
                regular_terms.push(term);
            }
        }
    }

    (phrases, regular_terms)
}

pub struct Tantivy<'a, R: tauri::Runtime, M: tauri::Manager<R>> {
    manager: &'a M,
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<'a, R: tauri::Runtime, M: tauri::Manager<R>> Tantivy<'a, R, M> {
    pub async fn register_collection(&self, config: CollectionConfig) -> Result<(), crate::Error> {
        let vault_base = self.manager.app_handle().settings().vault_base()?;
        let index_path = vault_base.join(&config.path).into_std_path_buf();
        let version_path = index_path.join("schema_version");

        std::fs::create_dir_all(&index_path)?;

        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;

        if guard.collections.contains_key(&config.name) {
            tracing::debug!("Collection '{}' already registered", config.name);
            return Ok(());
        }

        let schema = (config.schema_builder)();

        let needs_reindex = if index_path.join("meta.json").exists() {
            let stored_version = std::fs::read_to_string(&version_path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            stored_version != config.schema_version
        } else {
            false
        };

        let index = if index_path.join("meta.json").exists() && !needs_reindex {
            Index::open_in_dir(&index_path)?
        } else {
            if needs_reindex {
                tracing::debug!(
                    "Schema version changed for collection '{}', re-creating index",
                    config.name
                );
                std::fs::remove_dir_all(&index_path)?;
                std::fs::create_dir_all(&index_path)?;
            }
            Index::create_in_dir(&index_path, schema.clone())?
        };

        std::fs::write(&version_path, config.schema_version.to_string())?;

        register_tokenizers(&index);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = create_index_writer(&index)?;

        let collection_index = CollectionIndex {
            schema,
            index,
            reader,
            writer,
        };

        guard
            .collections
            .insert(config.name.clone(), collection_index);

        tracing::debug!(
            "Tantivy collection '{}' registered at {:?} (version: {})",
            config.name,
            index_path,
            config.schema_version
        );
        Ok(())
    }

    fn get_collection_name(collection: Option<String>) -> String {
        collection.unwrap_or_else(|| "default".to_string())
    }

    /// Searches only `scope`'s documents (ASPC C1.2), and only one meeting's
    /// when `request.filters.session_id` is set (B4.1).
    pub async fn search(
        &self,
        scope: &anlg_aspc::WorkspaceScope,
        request: SearchRequest,
    ) -> Result<SearchResult, crate::Error> {
        let collection_name = Self::get_collection_name(request.collection.clone());
        let state = self.manager.state::<IndexState>();
        let guard = state.inner.read().await;

        let collection_index = guard
            .collections
            .get(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;

        search_collection(collection_index, scope, request)
    }

    /// Number of documents in a collection, across all workspaces. For the
    /// indexer's consistency check only; never returns content.
    pub async fn document_count(&self, collection: Option<String>) -> Result<usize, crate::Error> {
        let collection_name = Self::get_collection_name(collection);
        let state = self.manager.state::<IndexState>();
        let guard = state.inner.read().await;
        let collection_index = guard
            .collections
            .get(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;
        Ok(collection_index.reader.searcher().num_docs() as usize)
    }

    pub async fn reindex(&self, collection: Option<String>) -> Result<(), crate::Error> {
        let collection_name = Self::get_collection_name(collection);
        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;

        let collection_index = guard
            .collections
            .get_mut(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;

        let schema = &collection_index.schema;
        let writer = &mut collection_index.writer;

        writer.delete_all_documents()?;

        let fields = get_fields(schema);

        writer.commit()?;

        tracing::debug!(
            "Reindex completed for collection '{}'. Index cleared and ready for new documents. Fields: {:?}",
            collection_name,
            fields.id
        );

        Ok(())
    }

    pub async fn add_document(
        &self,
        collection: Option<String>,
        document: SearchDocument,
    ) -> Result<(), crate::Error> {
        let collection_name = Self::get_collection_name(collection);
        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;

        let collection_index = guard
            .collections
            .get_mut(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;

        let schema = &collection_index.schema;
        let writer = &mut collection_index.writer;
        let fields = get_fields(schema);

        let doc = to_tantivy_document(&fields, &document);

        writer.add_document(doc)?;
        writer.commit()?;

        tracing::debug!(
            "Added document '{}' to collection '{}'",
            document.id,
            collection_name
        );

        Ok(())
    }

    pub async fn update_document(
        &self,
        collection: Option<String>,
        document: SearchDocument,
    ) -> Result<(), crate::Error> {
        let collection_name = Self::get_collection_name(collection);
        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;

        let collection_index = guard
            .collections
            .get_mut(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;

        let schema = &collection_index.schema;
        let writer = &mut collection_index.writer;
        let fields = get_fields(schema);

        let id_term = Term::from_field_text(fields.id, &document.id);
        writer.delete_term(id_term);

        let doc = to_tantivy_document(&fields, &document);

        writer.add_document(doc)?;
        writer.commit()?;

        tracing::debug!(
            "Updated document '{}' in collection '{}'",
            document.id,
            collection_name
        );

        Ok(())
    }

    pub async fn update_documents(
        &self,
        collection: Option<String>,
        documents: Vec<SearchDocument>,
    ) -> Result<(), crate::Error> {
        let collection_name = Self::get_collection_name(collection);
        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;

        let collection_index = guard
            .collections
            .get_mut(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;

        let schema = &collection_index.schema;
        let writer = &mut collection_index.writer;
        let fields = get_fields(schema);

        let count = documents.len();

        for document in documents {
            let id_term = Term::from_field_text(fields.id, &document.id);
            writer.delete_term(id_term);

            let doc = to_tantivy_document(&fields, &document);

            writer.add_document(doc)?;
        }

        writer.commit()?;

        tracing::debug!(
            "Updated {} documents in collection '{}'",
            count,
            collection_name
        );

        Ok(())
    }

    pub async fn apply_document_batch(
        &self,
        collection: Option<String>,
        documents: Vec<SearchDocument>,
        removals: Vec<String>,
    ) -> Result<(), crate::Error> {
        let collection_name = Self::get_collection_name(collection);
        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;

        let collection_index = guard
            .collections
            .get_mut(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;

        let schema = &collection_index.schema;
        let writer = &mut collection_index.writer;
        let fields = get_fields(schema);
        let updated_count = documents.len();
        let removed_count = removals.len();

        for id in removals {
            writer.delete_term(Term::from_field_text(fields.id, &id));
        }

        for document in documents {
            writer.delete_term(Term::from_field_text(fields.id, &document.id));

            let doc = to_tantivy_document(&fields, &document);

            writer.add_document(doc)?;
        }

        writer.commit()?;

        tracing::debug!(
            updated_count,
            removed_count,
            collection = collection_name,
            "applied search document batch"
        );

        Ok(())
    }

    pub async fn remove_document(
        &self,
        collection: Option<String>,
        id: String,
    ) -> Result<(), crate::Error> {
        let collection_name = Self::get_collection_name(collection);
        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;

        let collection_index = guard
            .collections
            .get_mut(&collection_name)
            .ok_or_else(|| crate::Error::CollectionNotFound(collection_name.clone()))?;

        let schema = &collection_index.schema;
        let writer = &mut collection_index.writer;
        let fields = get_fields(schema);

        let id_term = Term::from_field_text(fields.id, &id);
        writer.delete_term(id_term);
        writer.commit()?;

        tracing::debug!(
            "Removed document '{}' from collection '{}'",
            id,
            collection_name
        );

        Ok(())
    }

    /// ASPC D2.2: removes every document of these meetings from every
    /// collection and rewrites the segments, so their text is gone from
    /// `search_index/` and not only marked deleted.
    pub async fn purge_sessions(&self, session_ids: &[String]) -> Result<(), crate::Error> {
        if session_ids.is_empty() {
            return Ok(());
        }
        let state = self.manager.state::<IndexState>();
        let mut guard = state.inner.write().await;
        for collection_index in guard.collections.values_mut() {
            purge_sessions_from_collection(collection_index, session_ids)?;
        }
        Ok(())
    }
}

/// Deletes the meetings' documents (by `session_id` and by `id`), commits,
/// merges all segments so deleted documents are dropped from the files, and
/// removes the files no longer used.
pub(crate) fn purge_sessions_from_collection(
    collection_index: &mut CollectionIndex,
    session_ids: &[String],
) -> Result<(), crate::Error> {
    let fields = get_fields(&collection_index.schema);
    let writer = &mut collection_index.writer;
    for session_id in session_ids {
        writer.delete_term(Term::from_field_text(fields.session_id, session_id));
        writer.delete_term(Term::from_field_text(fields.id, session_id));
    }
    writer.commit()?;
    let segments = collection_index.index.searchable_segment_ids()?;
    if !segments.is_empty() {
        writer.merge(&segments).wait()?;
    }
    writer.garbage_collect_files().wait()?;
    collection_index.reader.reload()?;
    Ok(())
}

pub trait TantivyPluginExt<R: tauri::Runtime> {
    fn tantivy(&self) -> Tantivy<'_, R, Self>
    where
        Self: tauri::Manager<R> + Sized;
}

impl<R: tauri::Runtime, T: tauri::Manager<R>> TantivyPluginExt<R> for T {
    fn tantivy(&self) -> Tantivy<'_, R, Self>
    where
        Self: Sized,
    {
        Tantivy {
            manager: self,
            _runtime: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::get_tokenizer_name_for_language;
    use tantivy::schema::{STORED, Schema, TEXT};

    #[test]
    fn test_detect_language_tokenizer_integration() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let lang = detect_language(text);
        let tokenizer_name = get_tokenizer_name_for_language(&lang);
        assert_eq!(tokenizer_name, "lang_en");
    }

    #[test]
    fn creates_a_minimum_budget_single_threaded_writer() {
        let mut schema = Schema::builder();
        schema.add_text_field("content", TEXT | STORED);
        let index = Index::create_in_ram(schema.build());

        create_index_writer(&index).unwrap();
    }
}

/// The search itself, on one collection: always limited to `scope`
/// (C1.2) and, with `filters.session_id`, to one meeting (B4.1).
pub(crate) fn search_collection(
    collection_index: &CollectionIndex,
    scope: &anlg_aspc::WorkspaceScope,
    request: SearchRequest,
) -> Result<SearchResult, crate::Error> {
    let schema = &collection_index.schema;
    let index = &collection_index.index;
    let reader = &collection_index.reader;

    let fields = get_fields(schema);
    let searcher = reader.searcher();

    let use_fuzzy = request.options.fuzzy.unwrap_or(false);
    let phrase_slop = request.options.phrase_slop.unwrap_or(0);
    let has_query = !request.query.trim().is_empty();

    // Title boost factor (3x) to match Orama's title:3, content:1 behavior
    const TITLE_BOOST: f32 = 3.0;

    let mut combined_query: Box<dyn Query> = if !has_query {
        Box::new(AllQuery)
    } else if use_fuzzy {
        let distance = request.options.distance.unwrap_or(1);

        // Parse query to extract phrases (quoted) and regular terms
        let (phrases, regular_terms) = parse_query_parts(&request.query);

        let mut term_queries: Vec<(Occur, Box<dyn Query>)> = Vec::new();

        // Handle quoted phrases with PhraseQuery
        for phrase in phrases {
            let words: Vec<&str> = phrase.split_whitespace().collect();
            if words.len() > 1 {
                // Create phrase query for title field
                let title_terms: Vec<Term> = words
                    .iter()
                    .map(|w| Term::from_field_text(fields.title, w))
                    .collect();
                let mut title_phrase = PhraseQuery::new(title_terms);
                title_phrase.set_slop(phrase_slop);

                // Create phrase query for content field
                let content_terms: Vec<Term> = words
                    .iter()
                    .map(|w| Term::from_field_text(fields.content, w))
                    .collect();
                let mut content_phrase = PhraseQuery::new(content_terms);
                content_phrase.set_slop(phrase_slop);

                // Boost title matches by 3x
                let boosted_title: Box<dyn Query> =
                    Box::new(BoostQuery::new(Box::new(title_phrase), TITLE_BOOST));
                let content_query: Box<dyn Query> = Box::new(content_phrase);

                // Phrase must match in at least one field (title OR content)
                let phrase_field_query = BooleanQuery::new(vec![
                    (Occur::Should, boosted_title),
                    (Occur::Should, content_query),
                ]);

                term_queries.push((Occur::Must, Box::new(phrase_field_query)));
            } else if !words.is_empty() {
                // Single word "phrase" - treat as regular term
                let word = words[0];
                let title_fuzzy =
                    FuzzyTermQuery::new(Term::from_field_text(fields.title, word), distance, true);
                let content_fuzzy = FuzzyTermQuery::new(
                    Term::from_field_text(fields.content, word),
                    distance,
                    true,
                );

                let boosted_title: Box<dyn Query> =
                    Box::new(BoostQuery::new(Box::new(title_fuzzy), TITLE_BOOST));
                let content_query: Box<dyn Query> = Box::new(content_fuzzy);

                let term_field_query = BooleanQuery::new(vec![
                    (Occur::Should, boosted_title),
                    (Occur::Should, content_query),
                ]);

                term_queries.push((Occur::Must, Box::new(term_field_query)));
            }
        }

        // Handle regular (unquoted) terms with fuzzy matching
        for term in regular_terms {
            let title_fuzzy =
                FuzzyTermQuery::new(Term::from_field_text(fields.title, term), distance, true);
            let content_fuzzy =
                FuzzyTermQuery::new(Term::from_field_text(fields.content, term), distance, true);

            // Boost title matches by 3x
            let boosted_title: Box<dyn Query> =
                Box::new(BoostQuery::new(Box::new(title_fuzzy), TITLE_BOOST));
            let content_query: Box<dyn Query> = Box::new(content_fuzzy);

            // Each term must match in at least one field (title OR content)
            let term_field_query = BooleanQuery::new(vec![
                (Occur::Should, boosted_title),
                (Occur::Should, content_query),
            ]);

            // All terms must be present (Must for each term)
            term_queries.push((Occur::Must, Box::new(term_field_query)));
        }

        Box::new(BooleanQuery::new(term_queries))
    } else {
        let query_parser = QueryParser::for_index(index, vec![fields.title, fields.content]);
        query_parser.parse_query(&request.query)?
    };

    // Apply created_at filter
    if let Some(ref created_at_filter) = request.filters.created_at {
        let range_query = build_created_at_range_query(fields.created_at, created_at_filter);
        if let Some(rq) = range_query {
            combined_query = Box::new(BooleanQuery::new(vec![
                (Occur::Must, combined_query),
                (Occur::Must, rq),
            ]));
        }
    }

    // Workspace filter (always applied; C1.2).
    let mut workspace_terms: Vec<(Occur, Box<dyn Query>)> = vec![(
        Occur::Should,
        Box::new(TermQuery::new(
            Term::from_field_text(fields.workspace_id, scope.workspace_id()),
            IndexRecordOption::Basic,
        )),
    )];
    if scope.includes_unassigned() {
        workspace_terms.push((
            Occur::Should,
            Box::new(TermQuery::new(
                Term::from_field_text(fields.workspace_id, workspace_term_value("")),
                IndexRecordOption::Basic,
            )),
        ));
    }
    combined_query = Box::new(BooleanQuery::new(vec![
        (Occur::Must, combined_query),
        (Occur::Must, Box::new(BooleanQuery::new(workspace_terms))),
    ]));

    // Session filter ("This meeting" scope; B4.1). An empty id matches
    // nothing rather than everything.
    if let Some(ref session_id) = request.filters.session_id {
        let session_query = TermQuery::new(
            Term::from_field_text(fields.session_id, session_id),
            IndexRecordOption::Basic,
        );
        let session_query: Box<dyn Query> = if session_id.is_empty() {
            Box::new(tantivy::query::EmptyQuery)
        } else {
            Box::new(session_query)
        };
        combined_query = Box::new(BooleanQuery::new(vec![
            (Occur::Must, combined_query),
            (Occur::Must, session_query),
        ]));
    }

    // Private meetings (C6.4), resolved by the command in Rust.
    if request.filters.session_id.is_none() && !request.filters.exclude_session_ids.is_empty() {
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, combined_query)];
        for session_id in &request.filters.exclude_session_ids {
            clauses.push((
                Occur::MustNot,
                Box::new(TermQuery::new(
                    Term::from_field_text(fields.session_id, session_id),
                    IndexRecordOption::Basic,
                )),
            ));
        }
        combined_query = Box::new(BooleanQuery::new(clauses));
    }

    // Apply doc_type filter
    if let Some(ref doc_type) = request.filters.doc_type {
        let doc_type_term = Term::from_field_text(fields.doc_type, doc_type);
        let doc_type_query = TermQuery::new(doc_type_term, IndexRecordOption::Basic);
        combined_query = Box::new(BooleanQuery::new(vec![
            (Occur::Must, combined_query),
            (Occur::Must, Box::new(doc_type_query)),
        ]));
    }

    // Apply facet filter
    if let Some(ref facet_path) = request.filters.facet
        && let Ok(facet) = Facet::from_text(facet_path)
    {
        let facet_term = Term::from_facet(fields.facets, &facet);
        let facet_query = TermQuery::new(facet_term, IndexRecordOption::Basic);
        combined_query = Box::new(BooleanQuery::new(vec![
            (Occur::Must, combined_query),
            (Occur::Must, Box::new(facet_query)),
        ]));
    }

    // Use tuple collector to get both top docs and total count
    let (top_docs, count) = searcher.search(
        &combined_query,
        &(TopDocs::with_limit(request.limit), Count),
    )?;

    let generate_snippets = request.options.snippets.unwrap_or(false);
    let snippet_max_chars = request.options.snippet_max_chars.unwrap_or(150);

    let (title_snippet_gen, content_snippet_gen) = if generate_snippets {
        let mut title_gen = SnippetGenerator::create(&searcher, &*combined_query, fields.title)?;
        title_gen.set_max_num_chars(snippet_max_chars);

        let mut content_gen =
            SnippetGenerator::create(&searcher, &*combined_query, fields.content)?;
        content_gen.set_max_num_chars(snippet_max_chars);

        (Some(title_gen), Some(content_gen))
    } else {
        (None, None)
    };

    let mut hits = Vec::new();
    for (score, doc_address) in top_docs {
        let retrieved_doc: TantivyDocument = searcher.doc(doc_address)?;

        if let Some(search_doc) = extract_search_document(schema, &fields, &retrieved_doc) {
            let title_snippet = title_snippet_gen.as_ref().map(|generator| {
                let snippet = generator.snippet_from_doc(&retrieved_doc);
                Snippet {
                    fragment: snippet.fragment().to_string(),
                    highlights: snippet
                        .highlighted()
                        .iter()
                        .map(|range| HighlightRange {
                            start: range.start,
                            end: range.end,
                        })
                        .collect(),
                }
            });

            let content_snippet = content_snippet_gen.as_ref().map(|generator| {
                let snippet = generator.snippet_from_doc(&retrieved_doc);
                Snippet {
                    fragment: snippet.fragment().to_string(),
                    highlights: snippet
                        .highlighted()
                        .iter()
                        .map(|range| HighlightRange {
                            start: range.start,
                            end: range.end,
                        })
                        .collect(),
                }
            });

            hits.push(SearchHit {
                score,
                document: search_doc,
                title_snippet,
                content_snippet,
            });
        }
    }

    Ok(SearchResult { hits, count })
}

#[cfg(test)]
mod scoped_search_tests {
    use super::*;
    use crate::SearchFilters;
    use anlg_aspc::WorkspaceScope;

    fn document(id: &str, workspace_id: &str, session_id: &str, marker: &str) -> SearchDocument {
        SearchDocument {
            id: id.to_string(),
            doc_type: "session".to_string(),
            language: None,
            title: format!("Meeting {id}"),
            content: format!("shared words {marker}"),
            created_at: 1_700_000_000_000,
            facets: Vec::new(),
            workspace_id: workspace_id.to_string(),
            session_id: session_id.to_string(),
        }
    }

    fn collection(documents: &[SearchDocument]) -> CollectionIndex {
        let schema = crate::build_schema();
        let index = Index::create_in_ram(schema.clone());
        register_tokenizers(&index);
        let mut writer = create_index_writer(&index).unwrap();
        let fields = get_fields(&schema);
        for document in documents {
            writer
                .add_document(to_tantivy_document(&fields, document))
                .unwrap();
        }
        writer.commit().unwrap();
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .unwrap();
        CollectionIndex {
            schema,
            index,
            reader,
            writer,
        }
    }

    fn ids(
        collection: &CollectionIndex,
        scope: &WorkspaceScope,
        query: &str,
        session: Option<&str>,
    ) -> Vec<String> {
        let mut ids: Vec<String> = search_collection(
            collection,
            scope,
            SearchRequest {
                query: query.to_string(),
                collection: None,
                filters: SearchFilters {
                    session_id: session.map(str::to_string),
                    ..SearchFilters::default()
                },
                limit: 100,
                options: Default::default(),
            },
        )
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| hit.document.id)
        .collect();
        ids.sort();
        ids
    }

    /// D2.2: a purged meeting's words are gone from the index files, not
    /// only marked deleted; other meetings stay searchable.
    #[test]
    fn purged_sessions_leave_no_bytes_in_the_index_directory() {
        let dir = tempfile::tempdir().unwrap();
        let schema = crate::build_schema();
        let index = Index::create_in_dir(dir.path(), schema.clone()).unwrap();
        register_tokenizers(&index);
        let mut writer = create_index_writer(&index).unwrap();
        let fields = get_fields(&schema);
        for document in [
            document("gone", "w1", "gone", "zebrapurgemarker"),
            document("gone-doc", "w1", "gone", "zebrapurgemarker"),
            document("kept", "w1", "kept", "keptwordmarker"),
        ] {
            writer
                .add_document(to_tantivy_document(&fields, &document))
                .unwrap();
        }
        writer.commit().unwrap();
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .unwrap();
        let mut collection = CollectionIndex {
            schema,
            index,
            reader,
            writer,
        };
        let contains = |needle: &[u8]| {
            std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .any(|entry| {
                    std::fs::read(entry.path())
                        .map(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
                        .unwrap_or(false)
                })
        };
        assert!(contains(b"zebrapurgemarker"), "fixture must be on disk");

        purge_sessions_from_collection(&mut collection, &["gone".to_string()]).unwrap();

        assert!(!contains(b"zebrapurgemarker"));
        let scope = WorkspaceScope::local("w1").unwrap();
        assert!(ids(&collection, &scope, "zebrapurgemarker", None).is_empty());
        assert_eq!(
            ids(&collection, &scope, "keptwordmarker", None),
            vec!["kept"]
        );
    }

    /// C6.4: excluded (Private) meetings drop out of an all-meetings search
    /// but are still found by a "This meeting" search on themselves.
    #[test]
    fn excluded_sessions_drop_out_unless_scoped_to_them() {
        let collection = collection(&[
            document("open", "w1", "open", "markerone"),
            document("secret", "w1", "secret", "markerone"),
        ]);
        let scope = WorkspaceScope::local("w1").unwrap();
        let search = |session: Option<&str>| {
            let mut ids: Vec<String> = search_collection(
                &collection,
                &scope,
                SearchRequest {
                    query: "markerone".to_string(),
                    collection: None,
                    filters: SearchFilters {
                        session_id: session.map(str::to_string),
                        exclude_session_ids: vec!["secret".to_string()],
                        ..SearchFilters::default()
                    },
                    limit: 100,
                    options: Default::default(),
                },
            )
            .unwrap()
            .hits
            .into_iter()
            .map(|hit| hit.document.id)
            .collect();
            ids.sort();
            ids
        };
        assert_eq!(search(None), vec!["open".to_string()]);
        assert_eq!(search(Some("secret")), vec!["secret".to_string()]);
    }

    /// C1.4 at the search layer: workspace 1 never sees workspace 2.
    #[test]
    fn search_returns_only_the_scoped_workspace() {
        let collection = collection(&[
            document("w1-a", "w1", "w1-a", "markerone"),
            document("w1-unassigned", "", "w1-unassigned", "markerone"),
            document("w2-a", "w2", "w2-a", "markertwo"),
        ]);
        let local = WorkspaceScope::local("w1").unwrap();
        assert_eq!(
            ids(&collection, &local, "", None),
            ["w1-a", "w1-unassigned"]
        );
        assert_eq!(
            ids(&collection, &local, "shared", None),
            ["w1-a", "w1-unassigned"]
        );
        assert!(ids(&collection, &local, "markertwo", None).is_empty());

        let exact = WorkspaceScope::new("w1").unwrap();
        assert_eq!(ids(&collection, &exact, "shared", None), ["w1-a"]);
        let other = WorkspaceScope::new("w2").unwrap();
        assert_eq!(ids(&collection, &other, "", None), ["w2-a"]);
        // The unassigned sentinel is not reported back as a workspace.
        let hit = search_collection(
            &collection,
            &local,
            SearchRequest {
                query: "w1-unassigned".to_string(),
                collection: None,
                filters: SearchFilters::default(),
                limit: 10,
                options: Default::default(),
            },
        )
        .unwrap();
        assert!(
            hit.hits
                .iter()
                .all(|hit| hit.document.workspace_id != crate::UNASSIGNED_WORKSPACE)
        );
    }

    /// B4.2 at the search layer: "This meeting" scope returns only that
    /// meeting, even when the query asks for another meeting's marker.
    #[test]
    fn session_filter_limits_results_to_one_meeting() {
        let collection = collection(&[
            document("meeting-a", "w1", "meeting-a", "markera"),
            document("meeting-b", "w1", "meeting-b", "markerb"),
            document("other-ws", "w2", "meeting-a", "markera"),
        ]);
        let scope = WorkspaceScope::local("w1").unwrap();
        assert_eq!(
            ids(&collection, &scope, "shared", Some("meeting-a")),
            ["meeting-a"]
        );
        assert!(ids(&collection, &scope, "markerb", Some("meeting-a")).is_empty());
        // Same session id in another workspace is still excluded.
        assert_eq!(
            ids(&collection, &scope, "markera", Some("meeting-a")),
            ["meeting-a"]
        );
        // An empty session id matches nothing, not everything.
        assert!(ids(&collection, &scope, "", Some("")).is_empty());
    }
}
