//! GitHub Pages.
//!
//! There is nothing to negotiate with: Pages serves files. The adapter exists
//! so that the option is honest rather than absent, and so that the two files
//! Pages does need are not forgotten. An IRI without an extension will not
//! resolve here, which is why the README says to pair this with w3id or a
//! resolver rather than treating it as a deployment on its own.

use crate::render::manifest::Manifest;
use std::fmt::Write;

pub fn emit(manifest: &Manifest) -> Vec<(String, String)> {
    let mut notes = String::new();
    let _ = writeln!(
        notes,
        "# What will and will not resolve on GitHub Pages\n\n\
         Pages serves the files in this build and negotiates nothing. Concretely:\n"
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
        let _ = writeln!(
            notes,
            "- `{}{example}` does **not** resolve; `{site_root}{file}` does.",
            ns.iri_base
        );
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
                "- `{}{local}` does **not** resolve, and its file is a directory: \
                 `{site_root}{file}`.",
                ns.iri_base
            );
        }
    }
    let _ = writeln!(
        notes,
        "\nPair this with w3id.org or a `dcmi-ns` resolver, whose adapters are beside\n\
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
