//! Integration tests for SharePointConnector using a wiremock HTTP server.
//!
//! These tests stand up a local HTTP server, configure a SharePointClient
//! pointing at it, and exercise `list_documents`, `load_document`, and
//! `changes_since` end-to-end without a live SharePoint instance.

use std::sync::Arc;

use rag_connectors::sharepoint::{SharePointConnector, SharePointConnectorConfig, stable_id};
use rag_core::{models::SourceId, traits::Connector};
use sharepoint_mcp::{AuthMode, Config, SharePointClient};
use wiremock::{
    matchers::{method, path_regex},
    Mock, MockServer, ResponseTemplate,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_connector(server: &MockServer) -> SharePointConnector {
    let config = Config {
        site_url: server.uri(),
        auth: AuthMode::None,
        tls_accept_invalid: false,
        timeout_secs: 5,
        read_only: true,
        max_retries: 0,
        max_concurrent: 0,
        audit_log: false,
        max_connections: 0,
        keep_alive_secs: 0,
    };
    let client = SharePointClient::new(config).expect("failed to build SharePointClient");

    SharePointConnector::new(
        SourceId::from_str("sp-test"),
        Arc::new(client),
        SharePointConnectorConfig {
            library_path: "/sites/test/Shared Documents".into(),
            list_title: "Shared Documents".into(),
            include_extensions: vec!["txt".into(), "md".into()],
            max_file_bytes: 0,
        },
    )
}

/// JSON skeleton that SharePoint returns for a recursive file listing.
fn files_response(files: &[(&str, &str, &str, &str)]) -> serde_json::Value {
    // files: (name, server_relative_url, modified, length)
    let items: Vec<_> = files
        .iter()
        .map(|(name, url, modified, len)| {
            serde_json::json!({
                "Name": name,
                "ServerRelativeUrl": url,
                "TimeLastModified": modified,
                "Length": len,
                "CheckOutType": 2,
                "MajorVersion": 1,
                "MinorVersion": 0
            })
        })
        .collect();
    serde_json::json!({ "d": { "results": items } })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_documents_maps_files_to_document_refs() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path_regex("/sites/test/Shared%20Documents.*Files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(files_response(&[
            (
                "readme.txt",
                "/sites/test/Shared Documents/readme.txt",
                "2024-03-01T09:00:00Z",
                "1024",
            ),
            (
                "notes.md",
                "/sites/test/Shared Documents/notes.md",
                "2024-03-02T10:00:00Z",
                "512",
            ),
            // PDF should be filtered out by include_extensions
            (
                "report.pdf",
                "/sites/test/Shared Documents/report.pdf",
                "2024-03-03T11:00:00Z",
                "204800",
            ),
        ])))
        .mount(&server)
        .await;

    let connector = make_connector(&server);
    let docs = connector.list_documents().await.unwrap();

    assert_eq!(docs.len(), 2, "pdf should be filtered by include_extensions");
    assert!(docs.iter().any(|d| d.title == "readme.txt"));
    assert!(docs.iter().any(|d| d.title == "notes.md"));
    assert!(docs.iter().all(|d| d.url.is_some()));

    // Verify stable IDs are deterministic
    let expected_id = stable_id(&server.uri(), "/sites/test/Shared Documents/readme.txt");
    let txt_doc = docs.iter().find(|d| d.title == "readme.txt").unwrap();
    assert_eq!(txt_doc.id, expected_id);
}

#[tokio::test]
async fn list_documents_skips_oversized_files() {
    let server = MockServer::start().await;

    let connector = SharePointConnector::new(
        SourceId::from_str("sp-test"),
        Arc::new(
            SharePointClient::new(Config {
                site_url: server.uri(),
                auth: AuthMode::None,
                tls_accept_invalid: false,
                timeout_secs: 5,
                read_only: true,
                max_retries: 0,
                max_concurrent: 0,
                audit_log: false,
                max_connections: 0,
                keep_alive_secs: 0,
            })
            .unwrap(),
        ),
        SharePointConnectorConfig {
            library_path: "/sites/test/Shared Documents".into(),
            list_title: "Shared Documents".into(),
            include_extensions: vec![],
            max_file_bytes: 100, // tiny limit
        },
    );

    Mock::given(method("GET"))
        .and(path_regex("/sites/test/Shared%20Documents.*Files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(files_response(&[
            (
                "small.txt",
                "/sites/test/Shared Documents/small.txt",
                "2024-01-01T00:00:00Z",
                "50",
            ),
            (
                "large.txt",
                "/sites/test/Shared Documents/large.txt",
                "2024-01-01T00:00:00Z",
                "1000",
            ),
        ])))
        .mount(&server)
        .await;

    let docs = connector.list_documents().await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].title, "small.txt");
}

#[tokio::test]
async fn load_document_decodes_base64_content() {
    let server = MockServer::start().await;

    // SharePoint returns base64-encoded file content.
    let raw = "Hello from SharePoint!";
    let b64 = base64::engine::general_purpose::STANDARD.encode(raw);

    // The client calls /_api/web/GetFileByServerRelativeUrl('{url}')/$value
    Mock::given(method("GET"))
        .and(path_regex(".*\\$value"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&b64))
        .mount(&server)
        .await;

    let connector = make_connector(&server);
    let doc_ref = rag_core::models::DocumentRef {
        id: stable_id(&server.uri(), "/sites/test/Shared Documents/hello.txt"),
        source_id: SourceId::from_str("sp-test"),
        title: "hello.txt".into(),
        url: Some(format!("{}/sites/test/Shared Documents/hello.txt", server.uri())),
        modified_at: None,
        content_type: Some("text/plain".into()),
        size_bytes: Some(raw.len() as u64),
    };

    let doc = connector.load_document(&doc_ref).await.unwrap();
    assert_eq!(doc.content.trim(), raw);
    assert_eq!(doc.metadata.file_type.as_deref(), Some("txt"));
}

#[tokio::test]
async fn load_document_rejects_pdf_without_extension_worker() {
    let server = MockServer::start().await;
    let connector = make_connector(&server);

    let doc_ref = rag_core::models::DocumentRef {
        id: stable_id(&server.uri(), "/sites/test/Shared Documents/report.pdf"),
        source_id: SourceId::from_str("sp-test"),
        title: "report.pdf".into(),
        url: Some(format!("{}/sites/test/Shared Documents/report.pdf", server.uri())),
        modified_at: None,
        content_type: Some("application/pdf".into()),
        size_bytes: Some(102400),
    };

    let err = connector.load_document(&doc_ref).await.unwrap_err();
    assert!(
        err.to_string().contains("extension worker"),
        "expected error mentioning extension worker, got: {err}"
    );
}

#[tokio::test]
async fn changes_since_parses_change_token_and_events() {
    let server = MockServer::start().await;

    let changes_body = serde_json::json!({
        "d": {
            "results": [
                {
                    "ChangeType": 1,
                    "ChangeToken": { "StringValue": "1;3;abc-token-001" },
                    "File": {
                        "Name": "new-file.txt",
                        "ServerRelativeUrl": "/sites/test/Shared Documents/new-file.txt",
                        "TimeLastModified": "2024-04-01T08:00:00Z"
                    }
                },
                {
                    "ChangeType": 2,
                    "ChangeToken": { "StringValue": "1;3;abc-token-002" },
                    "File": {
                        "Name": "updated.txt",
                        "ServerRelativeUrl": "/sites/test/Shared Documents/updated.txt",
                        "TimeLastModified": "2024-04-01T09:00:00Z"
                    }
                },
                {
                    "ChangeType": 3,
                    "ChangeToken": { "StringValue": "1;3;abc-token-003" },
                    "ItemId": 42
                }
            ]
        }
    });

    Mock::given(method("POST"))
        .and(path_regex(".*GetChanges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(changes_body))
        .mount(&server)
        .await;

    let connector = make_connector(&server);
    let (events, new_token) = connector.changes_since(Some("1;3;old-token")).await.unwrap();

    assert_eq!(new_token, "1;3;abc-token-003", "new token should be from last change");
    // Created (txt) + Modified (txt) — both pass the txt/md filter
    // Deleted item 42 — passes (deletions bypass extension filter)
    assert_eq!(events.len(), 3);

    use rag_core::traits::ChangeKind;
    let created = events.iter().find(|e| matches!(e.kind, ChangeKind::Created)).unwrap();
    assert_eq!(created.document_ref.title, "new-file.txt");

    let modified = events.iter().find(|e| matches!(e.kind, ChangeKind::Modified)).unwrap();
    assert_eq!(modified.document_ref.title, "updated.txt");

    let deleted = events.iter().find(|e| matches!(e.kind, ChangeKind::Deleted)).unwrap();
    assert!(deleted.document_ref.url.is_none(), "deleted item has no URL");
}
