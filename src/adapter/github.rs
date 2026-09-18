//! GitHub Pages.
//!
//! There is nothing to negotiate with: Pages serves files and ignores `Accept`.
//! A term IRI *does* resolve here: Pages answers an extensionless path with the
//! matching `.html` file, and redirects a directory to its trailing slash. What
//! it cannot do is resolve to anything but HTML. This module claimed the
//! opposite until a real deployment was measured, where `/demo/Instrument`
//! answered 200 rather than 404. Pair Pages with w3id or a resolver so the
//! identity IRIs reach the other representations, not to make them resolve.

use crate::render::manifest::Manifest;
use std::fmt::Write;

pub fn emit(manifest: &Manifest) -> Vec<(String, String)> {
    let mut notes = String::new();
    let _ = writeln!(
        notes,
        "# What resolves on GitHub Pages, and what it resolves to\n\n\
         Pages serves the files in this build and ignores `Accept` entirely. A term\n\
         IRI does resolve, but only ever to HTML, whatever the client asked for:\n"
    );
    for ns in &manifest.namespaces {
        let Some(example) = ns.terms.first() else {
            // A namespace with no terms has nothing to warn about, and the
            // placeholder this used to print ("Term") named an IRI the site
            // does not mint.
            let _ = writeln!(
                notes,
                "- `{}` mints no terms; only its own document is served.",
                ns.iri_base
            );
            continue;
        };
        // The file, not `<term>.html`. A term that falls back to the
        // directory layout lives at `<term>/index.html`, and naming it
        // `<term>.html` pointed a reader at the *namespace's own* index
        // page: a different resource that answers 200, which is worse than
        // one that 404s. `crate::adapter::term_file_path` is the same
        // arithmetic every other adapter routes with.
        let html = crate::adapter::term_representations(ns)
            .into_iter()
            .find(|rep| rep.media_type == ns.default_type);
        let file = match html {
            Some(rep) => crate::adapter::term_file_path(ns, example, rep),
            None => format!("{}{example}", ns.mount),
        };
        let site_root = manifest.site_root.trim_end_matches('/');
        // A directory is reached through a redirect to its trailing slash, so
        // the example line has to say which shape this term actually is: when
        // the term that sorts first happens to be a directory one, claiming a
        // bare 200 describes a response the host never sends.
        if ns.dir_terms.iter().any(|t| t == example) {
            let _ = writeln!(
                notes,
                "- `{}{example}` answers 301 to `{}{example}/`, and its file is a directory: \
                 `{site_root}{file}`.",
                ns.iri_base, ns.iri_base
            );
        } else {
            let _ = writeln!(
                notes,
                "- `{}{example}` answers 200 with `{site_root}{file}`.",
                ns.iri_base
            );
        }
        // A term that falls back to the directory layout has a differently
        // shaped file, and the example above is whichever term sorts first,
        // which is usually a flat one. Naming them is the whole point of
        // this file: they are the ones a reader would guess wrong.
        for local in ns.dir_terms.iter().filter(|t| *t != example) {
            let file = match html {
                Some(rep) => crate::adapter::term_file_path(ns, local, rep),
                None => format!("{}{local}", ns.mount),
            };
            let _ = writeln!(
                notes,
                "- `{}{local}` answers 301 to `{}{local}/`, and its file is a directory: \
                 `{site_root}{file}`.",
                ns.iri_base, ns.iri_base
            );
        }
    }
    let _ = writeln!(
        notes,
        "\n`Accept: text/turtle` on any of these gets that same HTML page rather\n\
         than the Turtle sibling: the other representations are reachable only by\n\
         naming the file. A path this build mints nothing for answers 404.\n\n\
         Pair this with w3id.org or a `dcmi-ns` resolver, whose adapters are beside\n\
         this one, and point the redirects at these files."
    );

    vec![
        (
            ".nojekyll".to_owned(),
            "# Serve every file as it is; Jekyll would drop directories beginning with an underscore.\n".to_owned(),
        ),
        ("RESOLUTION.md".to_owned(), notes),
    ]
}
