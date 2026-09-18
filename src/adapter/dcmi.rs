//! A `dcmi-ns` resolver entry.
//!
//! The manifest is deliberately a superset of this schema, so the projection
//! is a narrowing and never a translation (`docs/output-convention.md`,
//! "Host adapters compiled from the manifest"). What narrows away is
//! recorded: `dcmi-ns` sends a term to one representation, so there is no
//! per-term suffix, no versioned path and no anchor map until the upstream
//! schema grows them.

use crate::render::manifest::Manifest;
use serde_json::{Value, json};

pub fn emit(manifest: &Manifest) -> Vec<(String, String)> {
    manifest
        .namespaces
        .iter()
        .map(|ns| {
            let representations: Vec<Value> = ns
                .representations
                .iter()
                .map(|rep| {
                    json!({
                        "mediaType": rep.media_type,
                        // `suffix: null` in the manifest is this schema's
                        // `append: "none"`: a namespace-level file only.
                        "append": rep.suffix.clone().map(Value::String).unwrap_or(Value::String("none".to_owned())),
                        "file": rep.namespace_file,
                    })
                })
                .collect();
            let config = json!({
                "id": ns.id,
                "namespace": ns.iri_base,
                "documentBase": ns.doc_base,
                "prefix": ns.resolver_prefix.clone().unwrap_or_else(|| ns.mount.trim_matches('/').to_owned()),
                "defaultMediaType": ns.default_type,
                "statusCode": ns.status_code,
                "maxAge": ns.cache_max_age,
                "resolverType": ns.resolver_type,
                "representations": representations,
                "terms": ns.terms,
                // Not in the upstream schema. See this module's header.
                "dirTerms": ns.dir_terms,
                "reserved": ns.reserved,
            });
            (
                format!("resolver/{}.json", ns.id),
                serde_json::to_string_pretty(&config).unwrap_or_default(),
            )
        })
        .collect()
}
