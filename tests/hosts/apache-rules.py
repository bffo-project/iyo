"""Apply a generated .htaccess to a matrix of requests, in Apache's order.

Two questions matter and neither is answerable by reading the file: does every
term IRI resolve to the right target, and does any rule redirect a request to
itself. The second is why this exists. An early version of the adapter
redirected a term to its own page and a nested namespace document to itself,
both infinite redirects, and both files were perfectly well-formed.

The cases are derived from `manifest.json` rather than written down, so this
works on any vocabulary the tool builds, including the fixture its README
tells you to build. Only the subset of directives the adapter emits is
interpreted: RewriteCond on HTTP_ACCEPT and QUERY_STRING, RewriteRule with
optional [R=nnn] and [L].

    cargo run -- build testdata/mini --out /tmp/iyo-site \
      --base-url http://127.0.0.1:8788/ --host apache
    python3 tests/hosts/apache-rules.py /tmp/iyo-site
"""
import json, pathlib, re, sys


def load_rules(path):
    rules, conds = [], []
    for line in pathlib.Path(path).read_text().splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if line.startswith('RewriteCond'):
            _, var, pattern, *flags = line.split(None, 3)
            conds.append((var, pattern.strip(), ' '.join(flags)))
        elif line.startswith('RewriteRule'):
            parts = line.split()
            rules.append((parts[1], parts[2], parts[3] if len(parts) > 3 else '', conds))
            conds = []
        else:
            conds = []
    return rules


def dehome(target, home):
    """A rule's target as a request path, whichever form it was written in."""
    if home and target.startswith(home):
        target = target[len(home):]
    return '/' + target.lstrip('/')


def apply(rules, path, accept='', query=''):
    """The first matching rule wins, as it does under [L]."""
    subject = path.lstrip('/')
    for pattern, target, flags, conds in rules:
        if not all(
            re.search(c, accept if 'HTTP_ACCEPT' in var else query,
                      re.I if 'NC' in cflags else 0)
            for var, c, cflags in conds
        ):
            continue
        m = re.match(pattern, subject)
        if not m:
            continue
        out = target
        for i, g in enumerate(m.groups(), 1):
            out = out.replace(f'${i}', g or '')
        status = re.search(r'R=(\d+)', flags)
        return (int(status.group(1)) if status else 200), out
    return None, None


def origin(url):
    rest = url.split('//', 1)[-1]
    return url[:len(url) - len(rest) + len(rest.split('/', 1)[0])]


def cases_from(manifest):
    """One case per thing a consumer actually requests.

    Each case carries the origin the rules are installed on, because that is
    what decides whether a target is a loop. A redirect host sits at the IRI
    origin and sends the client to the document origin, so a target on the
    document origin is the point, not a mistake.
    """
    out = []
    for ns in manifest['namespaces']:
        mount = ns['mount']
        redirecting = origin(ns['iri_base']) != origin(ns['doc_base'])
        base = ns['doc_base'].rstrip('/') if redirecting else mount.rstrip('/')
        reps = [r for r in ns['representations'] if r['suffix'] is not None]
        default = ns['default_type']
        dirs = set(ns.get('dir_terms', []))
        is_dir = lambda t: ns['layout'] == 'dir' or t in dirs
        # Where these rules are installed: the IRI origin when they redirect
        # onwards, the document origin when they serve.
        home = origin(ns['iri_base']) if redirecting else origin(ns['doc_base'])

        for version in ns.get('versions', []):
            seg = version['segment']
            out.append((f'{mount}{seg}', 'text/html', '', 302, f'{base}/{seg}/', home))

        for term in ns['terms'][:6]:
            for rep in reps:
                suffix = rep['suffix']
                if rep['media_type'] == default:
                    target = (f'{base}/{term}/index.html' if is_dir(term)
                              else f'{base}/{term}.html')
                    if redirecting:
                        target = f'{base}/{term}/' if is_dir(term) else f'{base}/{term}'
                    out.append((f'{mount}{term}', default, '', None, target, home))
                    continue
                target = (f'{base}/{term}/index{suffix}' if is_dir(term)
                          else f'{base}/{term}{suffix}')
                out.append((f'{mount}{term}', rep['media_type'], '', ns['status_code'], target, home))
                # A sibling asked for by name is not a negotiation.
                name = f'{mount}{term}/index{suffix}' if is_dir(term) else f'{mount}{term}{suffix}'
                out.append((name, rep['media_type'], '', None, None, home))

        # A reserved segment is not a term. One that is itself a namespace
        # mount is skipped, because it has a document rule of its own and
        # matching that is correct.
        mounts = {n['mount'] for n in manifest['namespaces']}
        for segment in ns.get('reserved', []):
            if f'{mount}{segment}/' in mounts:
                continue
            out.append((f'{mount}{segment}/', 'text/html', '', None, None, home))
        out.append((f'{mount}NotATermAnywhere', 'text/turtle', '', None, None, home))
    return out


def main():
    target = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else 'dist')
    root = target if target.is_dir() else target.parent
    htaccess = root / 'adapters' / 'apache' / '.htaccess'
    manifest_path = root / 'manifest.json'
    if not htaccess.is_file() or not manifest_path.is_file():
        print(f'need a built site with --host apache; looked in {root}')
        return 2

    rules = load_rules(htaccess)
    manifest = json.loads(manifest_path.read_text())
    cases = cases_from(manifest)

    failures, loops = [], []
    for path, accept, query, want_status, want_target, home in cases:
        status, got = apply(rules, path, accept, query)
        if want_target is None:
            if got is not None:
                failures.append(f'{path} [{accept}] matched a term rule -> {got}')
        elif got != want_target or (want_status and status != want_status):
            failures.append(f'{path} [{accept}] -> {status} {got}, wanted {want_status} {want_target}')
        # A redirect never terminates when the target, requested again, is
        # redirected to itself. Comparing the two paths textually is not
        # enough in either direction: adding a trailing slash is a redirect to
        # a different path and terminates, while a rule that matches both
        # forms and targets the slashed one does not. Only a target on the
        # host these rules are installed on can loop at all.
        if got and status:
            _, again = apply(rules, dehome(got, home), accept, query)
            if again is not None and dehome(again, home) == dehome(got, home):
                loops.append(f'{path} [{accept}] -> {got}')

    for f in failures:
        print('FAIL ' + f)
    for l in loops:
        print('LOOP ' + l)
    print(f'{len(cases)} apache checks, {len(failures) + len(loops)} failed')
    return 1 if failures or loops else 0


if __name__ == '__main__':
    sys.exit(main())
