"""Check a running `iyo serve` against tests/hosts/conformance.json, resolved
to concrete cases by `iyo conform --cases`.

The same resolved cases check the Rust resolver in tests/negotiate.rs and the
generated Cloudflare Worker in worker-negotiation.mjs. This one adds the HTTP
layer: the request line, the header parsing, the status line and the Location
header, none of which the other two exercise.

    cargo run -q -- build testdata/mini --out /tmp/iyo-site \
      --base-url https://example.org/
    cargo run -q -- conform /tmp/iyo-site --cases /tmp/cases.json
    cargo run -q -- serve /tmp/iyo-site --port 8788 &
    python3 tests/hosts/serve-negotiation.py http://127.0.0.1:8788
"""
import json, os, pathlib, sys, urllib.request, urllib.error

base = sys.argv[1] if len(sys.argv) > 1 else 'http://127.0.0.1:8788'
cases_path = (
    sys.argv[2] if len(sys.argv) > 2 else os.environ.get('IYO_CASES', '/tmp/cases.json')
)
plan = json.loads(pathlib.Path(cases_path).read_text())

# `tests/negotiate.rs:82` kept this floor precisely so a silently shrunken
# contract fails rather than reporting "0 checks, 0 failed" and exiting 0.
# This harness shipped without one; give it the same floor for the same
# reason.
if len(plan['cases']) < 26:
    print(f'FAIL fewer cases ran than the matrix had: {len(plan["cases"])}')
    sys.exit(1)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """A redirect is the answer under test, not something to follow."""
    def redirect_request(self, *args):
        return None


opener = urllib.request.build_opener(NoRedirect)


def call(path, accept, query):
    url = base + path + ('?' + query if query else '')
    request = urllib.request.Request(url)
    if accept:
        request.add_header('Accept', accept)
    try:
        with opener.open(request) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as e:
        return e.code, e.headers, e.read()


def content_type_is(actual, wanted):
    # Ignoring a trailing "; charset=..." and case, exactly as Rust's
    # Reply::content_type_is: a real server almost always sends a charset,
    # and comparing byte for byte against a bare media type from the
    # manifest would fail every compliant host.
    return (actual or '').split(';')[0].strip().lower() == wanted.lower()


def check_normative(headers, expect):
    # The convention's three normative headers apply to every response this
    # host serves for a namespace, not only a negotiated one: the output
    # convention states the cache values as a rule about *paths*
    # ("Snapshot paths carry ... latest carries ..."), and
    # NamespaceEntry::cache_control's own doc comment reads "what a host
    # should send for this namespace's own files" -- files, not only
    # negotiated term pages. A sibling asked for by name and a namespace's
    # own document are both that namespace's own files, so `file` gets the
    # same check as `serve` and `redirect`. Only `absent` (a 404) is exempt,
    # matching judge.rs's scope exactly.
    vary = [t.strip().lower() for t in (headers.get('Vary') or '').split(',')]
    if 'accept' not in vary:
        return f'no Vary: Accept (got {headers.get("Vary")!r})'
    # Checked against the manifest's own declared value, not merely for
    # presence: `expect['cache_control']` is `entry.cache_control`, already
    # substituted to the immutable snapshot policy inside a release. A
    # presence-only check cannot tell that value from the mutable latest one
    # -- exactly the blind spot CRITICAL 2 closes in judge.rs, and a harness
    # that checked less than the gate it is supposed to stand in for would
    # defeat the point of migrating onto the same resolved cases.
    got_cache_control = headers.get('Cache-Control') or ''
    if got_cache_control != expect['cache_control']:
        got = got_cache_control or '(none)'
        return (
            f'Cache-Control is {got!r}, where the manifest declares '
            f'{expect["cache_control"]!r}'
        )
    if headers.get('Access-Control-Allow-Origin') != '*':
        return 'Access-Control-Allow-Origin is not "*"'
    return None


def check_link(case, headers):
    # The whole Link header, byte for byte, against `expected_link`. Which
    # relations are present says nothing about where they point, which is
    # what caught a release's describedby pointing at its parent's
    # llms.txt (commit 125bf97).
    want = case.get('expected_link')
    if want is None:
        return None
    got = headers.get('Link')
    if got != want:
        return f'Link header:\n      got  {got}\n      want {want}'
    return None


def check_case(case, status, headers, body):
    expect = case['expect']
    kind = expect['kind']
    if kind == 'absent':
        return None if status == 404 else f'expected 404, got {status}'
    if kind == 'file':
        if status != 200:
            return f'expected 200, got {status}'
        if not content_type_is(headers.get('Content-Type'), expect['media_type']):
            return f'expected {expect["media_type"]}, got {headers.get("Content-Type")}'
        return check_normative(headers, expect) or check_link(case, headers)
    if kind == 'serve':
        if status != 200:
            return f'expected 200, got {status}'
        if not content_type_is(headers.get('Content-Type'), expect['media_type']):
            return f'expected {expect["media_type"]}, got {headers.get("Content-Type")}'
        if expect['body_contains'].encode() not in body:
            return f'body does not contain the identity IRI {expect["body_contains"]}'
        return check_normative(headers, expect) or check_link(case, headers)
    if kind == 'redirect':
        if status != expect['status']:
            return f'expected status {expect["status"]}, got {status}'
        if headers.get('Location') != expect['location']:
            return f'expected Location {expect["location"]}, got {headers.get("Location")}'
        return check_normative(headers, expect) or check_link(case, headers)
    raise ValueError(f'unknown expectation kind {kind}')


failures = []
for case in plan['cases']:
    status, headers, body = call(case['path'], case.get('accept'), case.get('query'))
    reason = check_case(case, status, headers, body)
    if reason:
        failures.append(f'{case["name"]} ({case["path"]}): {reason}')
for name in plan['unresolved']:
    failures.append(f'role did not resolve: {name}')

# A path must not climb out of the site. Not part of the negotiation
# contract itself, but a real property of the HTTP layer this harness is the
# only one of the three to exercise.
status, _, _ = call('/../Cargo.toml', 'text/plain', None)
if status != 404:
    failures.append(f'a traversal returned {status}')

for f in failures:
    print('FAIL ' + f)
print(f'{len(plan["unresolved"])} role(s) unresolved')
# `iyo serve` can tell all four expectation kinds apart (it has a real
# filesystem and a real socket), so nothing here is skipped for being
# indistinguishable.
print(f'{len(plan["cases"])} serve checks, 0 skipped as indistinguishable, {len(failures)} failed')
sys.exit(1 if failures or plan['unresolved'] else 0)
