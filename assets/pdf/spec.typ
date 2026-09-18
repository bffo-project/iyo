// The PDF of one namespace. Edit this file to theme it; nothing in the tool
// generates Typst markup, so this is the whole surface.
//
// It reads `model.json`, which the build writes beside it. That file is the
// whole input: nothing is passed on the Typst command line, and this file is
// copied verbatim rather than templated, so `{{ }}` here is literal text.
// Keep `document(...)` populated: PDF/A requires a title, and
// `text(lang: ...)` is what carries the language into the tagged structure a
// screen reader follows.

#let model = json("model.json")
#let site = model.site
#let doc = model.document
#let lang = site.at("lang", default: "en")

#set document(
  title: doc.title,
  author: doc.creators.map(c => c.at("name", default: "")).filter(n => n != ""),
  keywords: (doc.at("prefix", default: ""),).filter(k => k != ""),
  description: doc.at("description", default: (value: "")).at("value", default: ""),
)
#set text(lang: lang, size: 10pt)
#set page(
  paper: "a4",
  margin: (x: 2.2cm, y: 2.4cm),
  numbering: "1",
  footer: context [
    #set text(size: 8pt, fill: luma(90))
    #doc.title
    #h(1fr)
    #counter(page).display("1 of 1", both: true)
  ],
)
#set par(justify: false, leading: 0.62em)
#set heading(numbering: none)
#show heading.where(level: 1): it => block(above: 1.6em, below: 0.8em)[
  #set text(size: 17pt, weight: 700)
  #it.body
]
#show heading.where(level: 2): it => block(above: 1.3em, below: 0.6em)[
  #set text(size: 12.5pt, weight: 700)
  #it.body
]
#show heading.where(level: 3): it => block(above: 1em, below: 0.4em)[
  #set text(size: 10.5pt, weight: 600)
  #it.body
]
#show link: set text(fill: rgb("#1F5AA8"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8.5pt)

// A definition list, used for both metadata and per-term facts.
#let facts(rows) = {
  if rows.len() == 0 { return }
  block(above: 0.5em, below: 0.9em, grid(
    columns: (9em, 1fr),
    row-gutter: 0.45em,
    column-gutter: 0.8em,
    ..rows.map(((name, value)) => (text(weight: 600, name), value)).flatten(),
  ))
}

#let iri(value) = raw(value)

// Every IRI this document gives a section of its own. A cross-reference to one
// of these is a reference to a page of this same PDF, so it should move the
// reader there rather than open a browser; a reference to anything else is
// genuinely another document and still belongs on the web.
#let local_iris = doc.sections.map(s => s.terms.map(t => t.iri)).flatten()

// Note for anyone editing: do not name a variable `label` here. `label()` is
// the built-in that turns the IRI into an anchor, and shadowing it is what
// kept this document linking outward for every cross-reference it had.
#let reference(r) = {
  let name = r.at("label", default: r.at("iri", default: ""))
  let target = r.at("iri", default: "")
  if target != "" and local_iris.contains(target) {
    link(label(target), name)
  } else if "url" in r {
    link(r.url, name)
  } else {
    name
  }
}

#let references(list) = list.map(reference).join(", ")

// --- title page ------------------------------------------------------------

#align(center)[
  #block(above: 3em, below: 0.5em, text(size: 22pt, weight: 700, doc.title))
  #if "description" in doc [
    #block(width: 78%, above: 0.8em, text(size: 11pt, doc.description.value))
  ]
  #block(above: 1.2em, iri(doc.namespace))
  #if "version" in doc [
    #block(above: 0.8em, text(fill: luma(70))[Version #doc.version])
  ]
]

#facts((
  ("Namespace", iri(doc.namespace)),
  ..if "prefix" in doc { (("Preferred prefix", raw(doc.prefix)),) } else { () },
  ..if "version_iri" in doc { (("This version", link(doc.version_iri, doc.version_iri)),) } else { () },
  ..if "status" in doc { (("Status", doc.status),) } else { () },
  ..if "modified" in doc { (("Modified", doc.modified),) } else { () },
  ..if "license" in doc { (("Licence", link(doc.license, doc.license)),) } else { () },
  ("Terms", str(doc.term_count)),
  ("Documentation", link(site.base_url, site.base_url)),
))

#if doc.abstract_paragraphs.len() > 0 [
  = About this vocabulary
  #for paragraph in doc.abstract_paragraphs [ #par(paragraph) ]
]

#pagebreak()
#outline(title: [Contents], depth: 2)

// --- terms -----------------------------------------------------------------

#for section in doc.sections [
  #pagebreak()
  = #section.title

  #for term in section.terms [
    == #term.label
    #label(term.iri)

    #facts((
      ("IRI", iri(term.iri)),
      ..if "curie" in term { (("CURIE", raw(term.curie)),) } else { () },
      ("Kind", term.kind),
      ..if term.super_terms.len() > 0 { (("Sub-class of", references(term.super_terms)),) } else { () },
      ..if term.property.domain.len() > 0 { (("Domain", references(term.property.domain)),) } else { () },
      ..if term.property.range.len() > 0 { (("Range", references(term.property.range)),) } else { () },
      ..if term.concept.broader.len() > 0 { (("Broader", references(term.concept.broader)),) } else { () },
      ..if "status" in term { (("Status", term.status),) } else { () },
    ))

    #if term.deprecated [
      #block(fill: luma(240), inset: 8pt, radius: 3pt, width: 100%)[
        *Deprecated.*
        #if term.replaced_by.len() > 0 [ Use #references(term.replaced_by). ]
      ]
    ]

    #if "definition" in term [ #par(term.definition.value) ]

    #for note in term.notes [ #par(text(size: 9pt, note.value)) ]

    // What the shapes require of this term, kept apart from what the
    // vocabulary says it means.
    #for view in term.shapes [
      === Record template: #view.shape.label
      #text(size: 9pt, fill: luma(70))[Applies to #view.targeting.]
      #table(
        columns: (1fr, 1fr, auto),
        stroke: 0.4pt + luma(180),
        inset: 5pt,
        table.header([*Field*], [*Values*], [*Count*]),
        ..view.fields.map(f => (
          [#f.name#if f.required [ #text(size: 8pt)[(required)]]
           #if "description" in f [\ #text(size: 8pt, fill: luma(90), f.description)]],
          [#f.value_type#if f.in_scheme.len() > 0 [ #text(size: 8pt)[from #references(f.in_scheme)]]],
          [#f.at("cardinality", default: "any")],
        )).flatten(),
      )
    ]

    #if term.constraints.len() > 0 [
      #facts(term.constraints.map(c => (
        "Constrained by",
        [#reference(c.shape)#if "cardinality" in c [, #c.cardinality]#if c.required [, required]],
      )))
    ]
  ]
]
