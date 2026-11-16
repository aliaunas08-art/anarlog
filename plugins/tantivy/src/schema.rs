use tantivy::TantivyDocument;
use tantivy::schema::{
    DateOptions, DateTimePrecision, FAST, FacetOptions, Field, STORED, STRING, Schema,
    TextFieldIndexing, TextOptions, Value,
};

use crate::SearchDocument;

pub struct SchemaFields {
    pub id: Field,
    pub doc_type: Field,
    pub language: Field,
    pub title: Field,
    pub content: Field,
    pub created_at: Field,
    pub facets: Field,
    pub workspace_id: Field,
    pub session_id: Field,
}

/// Stored in `workspace_id` for documents whose row has no workspace yet
/// (`''`); such rows belong to the local workspace (C1), so a local scope
/// matches this value too. An empty raw term is avoided on purpose.
pub const UNASSIGNED_WORKSPACE: &str = "_unassigned";

pub fn workspace_term_value(workspace_id: &str) -> &str {
    if workspace_id.is_empty() {
        UNASSIGNED_WORKSPACE
    } else {
        workspace_id
    }
}

pub fn build_schema() -> Schema {
    let mut schema_builder = Schema::builder();
    schema_builder.add_text_field("id", STRING | STORED);
    schema_builder.add_text_field("doc_type", STRING | STORED);
    schema_builder.add_text_field("language", STRING | STORED);

    let text_indexing = TextFieldIndexing::default()
        .set_tokenizer("multilang")
        .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions);
    let text_options = TextOptions::default()
        .set_indexing_options(text_indexing)
        .set_stored();

    schema_builder.add_text_field("title", text_options.clone());
    schema_builder.add_text_field("content", text_options);
    schema_builder.add_date_field(
        "created_at",
        DateOptions::from(FAST | STORED).set_precision(DateTimePrecision::Milliseconds),
    );
    schema_builder.add_facet_field("facets", FacetOptions::default());
    // ASPC C1.2 / B4.1: every search is filtered by workspace, and "This
    // meeting" scope by session.
    schema_builder.add_text_field("workspace_id", STRING | STORED);
    schema_builder.add_text_field("session_id", STRING | STORED);
    schema_builder.build()
}

pub fn get_fields(schema: &Schema) -> SchemaFields {
    SchemaFields {
        id: schema.get_field("id").unwrap(),
        doc_type: schema.get_field("doc_type").unwrap(),
        language: schema.get_field("language").unwrap(),
        title: schema.get_field("title").unwrap(),
        content: schema.get_field("content").unwrap(),
        created_at: schema.get_field("created_at").unwrap(),
        facets: schema.get_field("facets").unwrap(),
        workspace_id: schema.get_field("workspace_id").unwrap(),
        session_id: schema.get_field("session_id").unwrap(),
    }
}

pub fn extract_search_document(
    _schema: &Schema,
    fields: &SchemaFields,
    doc: &TantivyDocument,
) -> Option<SearchDocument> {
    let id = doc.get_first(fields.id)?.as_str()?.to_string();
    let doc_type = doc.get_first(fields.doc_type)?.as_str()?.to_string();
    let language = doc
        .get_first(fields.language)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let title = doc.get_first(fields.title)?.as_str()?.to_string();
    let content = doc.get_first(fields.content)?.as_str()?.to_string();
    let created_at = doc
        .get_first(fields.created_at)?
        .as_datetime()?
        .into_timestamp_millis();

    let facets: Vec<String> = doc
        .get_all(fields.facets)
        .filter_map(|v| v.as_facet().map(|f| f.to_string()))
        .collect();

    let workspace_id = doc
        .get_first(fields.workspace_id)
        .and_then(|v| v.as_str())
        .filter(|v| *v != UNASSIGNED_WORKSPACE)
        .unwrap_or_default()
        .to_string();
    let session_id = doc
        .get_first(fields.session_id)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    Some(SearchDocument {
        id,
        doc_type,
        language,
        title,
        content,
        created_at,
        facets,
        workspace_id,
        session_id,
    })
}

/// The Tantivy document for a search document.
pub fn to_tantivy_document(fields: &SchemaFields, document: &SearchDocument) -> TantivyDocument {
    let mut doc = TantivyDocument::new();
    doc.add_text(fields.id, &document.id);
    doc.add_text(fields.doc_type, &document.doc_type);
    doc.add_text(fields.language, document.language.as_deref().unwrap_or(""));
    doc.add_text(fields.title, &document.title);
    doc.add_text(fields.content, &document.content);
    doc.add_date(
        fields.created_at,
        tantivy::DateTime::from_timestamp_millis(document.created_at),
    );
    for facet_path in &document.facets {
        if let Ok(facet) = tantivy::schema::Facet::from_text(facet_path) {
            doc.add_facet(fields.facets, facet);
        }
    }
    doc.add_text(
        fields.workspace_id,
        workspace_term_value(&document.workspace_id),
    );
    doc.add_text(fields.session_id, &document.session_id);
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::DateTime;

    #[test]
    fn test_build_schema_stores_created_at_as_date_field() {
        let schema = build_schema();
        let created_at = schema.get_field("created_at").unwrap();
        let entry = schema.get_field_entry(created_at);

        assert!(entry.field_type().is_date());
        assert!(entry.is_fast());
        assert!(entry.is_stored());
    }

    #[test]
    fn test_extract_search_document_reads_created_at_date_field_as_epoch_millis() {
        let schema = build_schema();
        let fields = get_fields(&schema);
        let mut doc = TantivyDocument::new();

        doc.add_text(fields.id, "doc-1");
        doc.add_text(fields.doc_type, "session");
        doc.add_text(fields.language, "en");
        doc.add_text(fields.title, "Title");
        doc.add_text(fields.content, "Content");
        doc.add_date(
            fields.created_at,
            DateTime::from_timestamp_millis(1_710_000_000_123),
        );

        let extracted = extract_search_document(&schema, &fields, &doc).unwrap();
        assert_eq!(extracted.created_at, 1_710_000_000_123);
    }
}
