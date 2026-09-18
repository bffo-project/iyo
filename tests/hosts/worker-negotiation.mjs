// Check the generated Cloudflare Worker against tests/hosts/conformance.json,
// resolved to concrete cases by `iyo conform --cases`.
//
// The same resolved cases check the Rust resolver in tests/negotiate.rs and a
// running `iyo serve` in serve-negotiation.py. Measuring all three against one
// written contract, rather than against each other, means two implementations
// agreeing on a wrong answer still fails.
//
//   cargo run -q -- build testdata/mini --out /tmp/iyo-site \
//     --base-url https://example.org/ --host cloudflare
//   cargo run -q -- conform /tmp/iyo-site --cases /tmp/cases.json
//   cp /tmp/iyo-site/adapters/cloudflare/worker.js tests/hosts/worker.mjs
//   IYO_SITE=/tmp/iyo-site node tests/hosts/worker-negotiation.mjs
import worker from './worker.mjs';
import { readFileSync, readdirSync, statSync } from 'node:fs';

const SITE = process.env.IYO_SITE || '/tmp/iyo-site';
const CASES_PATH = process.env.IYO_CASES || '/tmp/cases.json';

// Every path the build actually wrote, so the asset stub below can tell a
// `file` case from an `absent` one. A stub that answers 200 for everything
// cannot make that distinction, which is exactly what the negative perimeter
// checks: an application's catch-all
// route also answers 200 for anything.
const present = new Set();
(function walk(dir, prefix) {
  for (const entry of readdirSync(dir)) {
    const full = `${dir}/${entry}`;
    if (statSync(full).isDirectory()) walk(full, `${prefix}${entry}/`);
    else present.add(`${prefix}${entry}`);
  }
})(SITE, '');

function mediaTypeOf(path) {
  const ext = path.includes('.') ? path.slice(path.lastIndexOf('.')) : '';
  return (
    {
      '.html': 'text/html; charset=utf-8',
      '.md': 'text/markdown; charset=utf-8',
      '.ttl': 'text/turtle; charset=utf-8',
      '.jsonld': 'application/ld+json; charset=utf-8',
      '.json': 'application/json; charset=utf-8',
      '.txt': 'text/plain; charset=utf-8',
    }[ext] || 'application/octet-stream'
  );
}

// A real hit returns the file's own bytes, not a placeholder, because a
// `serve` expectation's whole point is that the body names the subject's
// identity IRI: an application catch-all answers 200 text/html for
// every unknown path too, and only the body tells them apart.
const env = {
  ASSETS: {
    async fetch(req) {
      const url = typeof req === 'string' ? req : req.url || String(req);
      const path = new URL(url).pathname.replace(/^\//, '');
      const real = [path, `${path}index.html`, `${path}/index.html`].find((c) => present.has(c));
      if (!real) {
        return new Response('not found', { status: 404, headers: { 'content-type': 'text/plain' } });
      }
      return new Response(readFileSync(`${SITE}/${real}`, 'utf8'), {
        status: 200,
        headers: { 'content-type': mediaTypeOf(real) },
      });
    },
  },
};

// Ignoring a trailing `; charset=…` and case, exactly as
// `Reply::content_type_is` does in Rust: a real server almost always sends a
// charset, and comparing byte for byte against a bare media type from the
// manifest would fail every compliant host.
function contentTypeIs(actual, wanted) {
  return (actual || '')
    .split(';')[0]
    .trim()
    .toLowerCase() === wanted.toLowerCase();
}

async function call(c) {
  const headers = c.accept ? { accept: c.accept } : {};
  const url = 'https://example.org' + c.path + (c.query ? '?' + c.query : '');
  const res = await worker.fetch(new Request(url, { headers }), env);
  const body = await res.text();
  return { res, body };
}

// The convention's three normative headers apply to every response this host
// serves for a namespace, not only a negotiated one: the output
// convention states the cache values as a rule about *paths* ("Snapshot
// paths carry ... latest carries ..."), and `NamespaceEntry::cache_control`'s
// own doc comment reads "what a host should send for this namespace's own
// files" -- files, not only negotiated term pages. A sibling asked for by
// name and a namespace's own document are both that namespace's own files,
// so `file` gets the same check as `serve` and `redirect`. Only `absent`
// (a 404) is exempt, matching `judge.rs`'s scope exactly: nothing here is
// this namespace's file, and the header rule has nothing to say about a
// response that is not one.
function checkNormative(res, e) {
  const vary = (res.headers.get('vary') || '')
    .split(',')
    .map((t) => t.trim().toLowerCase());
  if (!vary.includes('accept')) {
    return `no Vary: Accept (got ${JSON.stringify(res.headers.get('vary'))})`;
  }
  // Checked against the manifest's own declared value, not merely for
  // presence: `e.cache_control` is `entry.cache_control`, already
  // substituted to the immutable snapshot policy inside a release. A
  // presence-only check cannot tell that value from the mutable latest one
  // -- the blind spot CRITICAL 2 closes in judge.rs, which this harness
  // must match rather than check less than.
  const gotCacheControl = res.headers.get('cache-control') || '';
  if (gotCacheControl !== e.cache_control) {
    const got = gotCacheControl || '(none)';
    return `Cache-Control is ${JSON.stringify(got)}, where the manifest declares ${JSON.stringify(e.cache_control)}`;
  }
  if (res.headers.get('access-control-allow-origin') !== '*') {
    return 'Access-Control-Allow-Origin is not "*"';
  }
  return null;
}

// The whole `Link` header, byte for byte, against `expected_link`. Which
// relations are present says nothing about where they point, which is what
// caught a release's `describedby` pointing at its parent's `llms.txt`
// (commit 125bf97).
function checkLink(c, res) {
  if (c.expected_link == null) return null;
  const got = res.headers.get('link');
  if (got !== c.expected_link) {
    return `Link header:\n      got  ${got}\n      want ${c.expected_link}`;
  }
  return null;
}

function checkCase(c, res, body) {
  const e = c.expect;
  switch (e.kind) {
    case 'absent':
      return res.status === 404 ? null : `expected 404, got ${res.status}`;
    case 'file':
      if (res.status !== 200) return `expected 200, got ${res.status}`;
      if (!contentTypeIs(res.headers.get('content-type'), e.media_type)) {
        return `expected ${e.media_type}, got ${res.headers.get('content-type')}`;
      }
      return checkNormative(res, e) || checkLink(c, res);
    case 'serve':
      if (res.status !== 200) return `expected 200, got ${res.status}`;
      if (!contentTypeIs(res.headers.get('content-type'), e.media_type)) {
        return `expected ${e.media_type}, got ${res.headers.get('content-type')}`;
      }
      if (!body.includes(e.body_contains)) {
        return `body does not contain the identity IRI ${e.body_contains}`;
      }
      return checkNormative(res, e) || checkLink(c, res);
    case 'redirect':
      if (res.status !== e.status) return `expected status ${e.status}, got ${res.status}`;
      if (res.headers.get('location') !== e.location) {
        return `expected Location ${e.location}, got ${res.headers.get('location')}`;
      }
      return checkNormative(res, e) || checkLink(c, res);
    default:
      throw new Error('unknown expectation kind ' + e.kind);
  }
}

const plan = JSON.parse(readFileSync(CASES_PATH, 'utf8'));

// `tests/negotiate.rs:82` kept this floor precisely so a silently shrunken
// contract fails rather than reporting "0 checks, 0 failed" and exiting 0.
// This harness shipped without one; give it the same floor for the same
// reason.
if (plan.cases.length < 26) {
  console.log(`FAIL fewer cases ran than the matrix had: ${plan.cases.length}`);
  process.exit(1);
}

const failures = [];
for (const c of plan.cases) {
  const { res, body } = await call(c);
  const reason = checkCase(c, res, body);
  if (reason) failures.push(`${c.name} (${c.path}): ${reason}`);
}
for (const name of plan.unresolved) failures.push(`role did not resolve: ${name}`);

for (const f of failures) console.log('FAIL ' + f);
console.log(`${plan.unresolved.length} role(s) unresolved`);
// Once the asset stub above knows the built tree, the generated Worker can
// tell all four expectation kinds apart, so nothing here is skipped for
// being indistinguishable.
console.log(
  `${plan.cases.length} worker checks, 0 skipped as indistinguishable, ${failures.length} failed`,
);
process.exit(failures.length || plan.unresolved.length ? 1 : 0);
