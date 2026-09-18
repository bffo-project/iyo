//! Vercel.
//!
//! `redirects[]` with `has` header rules, which match a substring of `Accept`
//! and cannot rank q-values, so rule order decides. `headers[]` carries the
//! namespace-level `Link` and cache policy; per-term `Link` headers would
//! need a function, which is out of scope for a configuration-only adapter.

use super::term_representations;
use crate::render::manifest::Manifest;
use serde_json::{Value, json};

pub fn emit(manifest: &Manifest) -> Vec<(String, String)> {
    let mut redirects: Vec<Value> = Vec::new();
    let mut headers: Vec<Value> = Vec::new();

    for ns in &manifest.namespaces {
        // A release IRI first, so it is never read as a term.
        for version in &ns.versions {
            redirects.push(json!({
                "source": format!("{}{}", ns.mount, version.segment),
                "destination": format!("{}{}/", ns.mount, version.segment),
                "statusCode": 308,
            }));
        }

        // A dynamic segment matches anything, including `Format.ttl` and a
        // reserved segment, so the pattern excludes a dot and the terms that
        // need a different target get their own rule first. Without this a
        // request for a sibling is redirected to `Format.ttl.md`, and
        // `/ontology/shapes` is redirected to itself.
        for local in &ns.dir_terms {
            for rep in term_representations(ns) {
                if rep.media_type == ns.default_type {
                    continue;
                }
                let suffix = rep.suffix.clone().unwrap_or_default();
                redirects.push(json!({
                    "source": format!("{}{local}", ns.mount),
                    "has": [{ "type": "header", "key": "accept",
                              "value": format!(".*{}.*", regex_escape(&rep.media_type)) }],
                    "destination": format!("{}{local}/index{suffix}", ns.mount),
                    "statusCode": ns.status_code,
                }));
            }
            redirects.push(json!({
                "source": format!("{}{local}", ns.mount),
                "destination": format!("{}{local}/", ns.mount),
                "statusCode": ns.status_code,
            }));
        }

        let dir = ns.layout == "dir";
        for rep in term_representations(ns) {
            if rep.media_type == ns.default_type {
                continue;
            }
            let suffix = rep.suffix.clone().unwrap_or_default();
            let destination = if dir {
                format!("{}:local/index{suffix}", ns.mount)
            } else {
                format!("{}:local{suffix}", ns.mount)
            };
            redirects.push(json!({
                "source": format!("{}:local([^./]+)", ns.mount),
                "has": [{ "type": "header", "key": "accept",
                          "value": format!(".*{}.*", regex_escape(&rep.media_type)) }],
                "destination": destination,
                "statusCode": ns.status_code,
            }));
        }

        for version in &ns.versions {
            headers.push(json!({
                "source": format!("{}{}/(.*)", ns.mount, version.segment),
                "headers": [
                    { "key": "Cache-Control", "value": ns.snapshot_cache_control },
                    { "key": "Access-Control-Allow-Origin", "value": "*" },
                ],
            }));
        }
        headers.push(json!({
            "source": format!("{}(.*)", ns.mount),
            "headers": [
                { "key": "Vary", "value": "Accept" },
                { "key": "Cache-Control", "value": ns.cache_control },
                { "key": "Access-Control-Allow-Origin", "value": "*" },
                { "key": "Link", "value": format!("<{}>; rel=\"describedby\"; type=\"text/plain\"", ns.llms_txt) },
            ],
        }));
    }

    let config = json!({
        "$schema": "https://openapi.vercel.sh/vercel.json",
        "cleanUrls": true,
        "trailingSlash": false,
        "redirects": redirects,
        "headers": headers,
    });
    vec![(
        "vercel.json".to_owned(),
        serde_json::to_string_pretty(&config).unwrap_or_default(),
    )]
}

/// Vercel matches `has.value` as a regular expression, so a media type's own
/// punctuation has to be escaped or `application/ld+json` matches far more
/// than it should.
fn regex_escape(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\' => {
                format!("\\{c}")
            }
            other => other.to_string(),
        })
        .collect()
}
