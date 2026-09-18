//! Requests go through `curl` rather than through a Rust HTTP client. A probe
//! needs TLS, and a TLS stack is a large dependency for a diagnostic that
//! runs by hand; `curl` is on every machine this tool targets, is the
//! reference implementation for exactly this kind of measurement, and its
//! `-w` templates give the response fields without parsing raw headers.
//! Requests are batched with `--next`, so one process makes many of them.
//!
//! A batch runs as several `curl` processes rather than one: `--parallel`
//! interleaves `-w` output across requests with no way to tell one reply
//! from another after the fact, so instead the batch is split into chunks,
//! each still chained sequentially with `--next` exactly as a whole batch
//! used to be, and the chunks run concurrently on their own threads. A
//! reply is placed back at its request's own position once its chunk's
//! thread returns, never at the position of whichever chunk finished
//! first, so `fetch`'s callers can keep pairing replies with requests by
//! index regardless of completion order.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{self, Command};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How many requests go into one `fetch` call. Large enough that the
/// per-call overhead disappears, small enough that a command line stays
/// sane.
pub const BATCH: usize = 40;

/// How many of a batch's `curl` processes run at once. Bounded rather than
/// one-process-per-request so a batch of hundreds does not try to open
/// hundreds of sockets at the same origin simultaneously.
pub const CONCURRENCY: usize = 8;

/// The per-request `--max-time` to hand `fetch` for a batch of `batch_len`
/// requests, so `--deadline` bounds the whole run the way its help text
/// says: a batch's slowest chunk chains up to `batch_len.div_ceil(workers)`
/// requests, each allowed the full per-request timeout, so an unclamped
/// batch can run for `chunk_depth × timeout` -- 75s at the defaults, well
/// past a `--deadline` far shorter than that -- before the *next* batch's
/// deadline check ever runs. Clamping per batch instead of lowering
/// `timeout` globally still gives one request its full budget when the
/// deadline has room to spare, and only tightens it as the deadline nears.
/// Never below one second: `--max-time 0` tells curl to wait forever,
/// exactly the opposite of a deadline.
pub fn clamped_timeout(remaining: Duration, batch_len: usize, timeout: u32) -> u32 {
    if batch_len == 0 {
        return timeout;
    }
    let workers = CONCURRENCY.min(batch_len);
    let chunk_depth = batch_len.div_ceil(workers) as u64;
    let budget = (remaining.as_secs() / chunk_depth)
        .max(1)
        .min(u64::from(u32::MAX));
    timeout.min(budget as u32)
}

// Tab separated so a header value containing a comma or a space survives;
// `%header{}` needs curl 7.84 or newer.
const WRITE_OUT: &str = "%{http_code}\t%{content_type}\t%header{vary}\t%header{link}\t%{redirect_url}\t%header{cache-control}\t%header{access-control-allow-origin}\n";

/// One request to make.
#[derive(Debug, Clone)]
pub struct Request {
    pub url: String,
    /// The `Accept` header to send. `None` sends no `Accept` header at all,
    /// which is a different request from sending `*/*`.
    pub accept: Option<String>,
}

/// One response, as `curl` reported it.
#[derive(Debug, Clone, Serialize)]
pub struct Reply {
    pub url: String,
    pub accept: String,
    pub status: u16,
    pub content_type: String,
    pub vary: String,
    pub link: String,
    pub location: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cache_control: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cors: String,
    /// The response body, when the caller asked `fetch` for one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

impl Reply {
    /// Whether the response varies on `Accept`, by token rather than by
    /// substring. `Vary: Accept-Encoding` contains "accept" and means
    /// nothing of the kind: a host that varies only on compression is
    /// exactly the host the convention's header rule is asking about.
    pub fn varies_on_accept(&self) -> bool {
        self.vary
            .split(',')
            .any(|token| token.trim().eq_ignore_ascii_case("accept"))
    }

    /// Whether this response's `Content-Type` is `media_type`, ignoring a
    /// trailing parameter such as `; charset=utf-8` and case. A real server
    /// almost always sends a charset -- this crate's own `serve` included --
    /// so comparing the header byte for byte against a bare media type from
    /// a manifest fails every compliant host it is pointed at.
    pub fn content_type_is(&self, media_type: &str) -> bool {
        self.content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case(media_type)
    }
}

/// Whether `curl` is on `PATH`, so a caller can fail with a clear message
/// before planning any requests around it.
pub fn available() -> bool {
    Command::new("curl")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Parse one line of `curl`'s `-w` report into a `Reply`. `url` and `accept`
/// come from the request, not from `curl`'s output, because `curl` does not
/// echo back the request it made.
fn parse(line: &str, url: &str, accept: &str) -> Reply {
    let mut fields = line.split('\t');
    let status = fields
        .next()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(0);
    Reply {
        url: url.to_owned(),
        accept: accept.to_owned(),
        status,
        content_type: fields.next().unwrap_or_default().trim().to_owned(),
        vary: fields.next().unwrap_or_default().trim().to_owned(),
        link: fields.next().unwrap_or_default().trim().to_owned(),
        location: fields.next().unwrap_or_default().trim().to_owned(),
        cache_control: fields.next().unwrap_or_default().trim().to_owned(),
        cors: fields.next().unwrap_or_default().trim().to_owned(),
        body: None,
    }
}

/// Parse `curl`'s whole report, one line per request in order.
///
/// A request that produced no line at all is a failure to reach the host,
/// and silently dropping it would turn an outage into a clean report, so it
/// is padded in as a zero-status reply instead.
fn parse_all(text: &str, batch: &[Request]) -> Vec<Reply> {
    let mut replies: Vec<Reply> = text
        .lines()
        .zip(batch.iter())
        .map(|(line, request)| {
            parse(
                line,
                &request.url,
                request.accept.as_deref().unwrap_or_default(),
            )
        })
        .collect();
    while replies.len() < batch.len() {
        let request = &batch[replies.len()];
        replies.push(Reply {
            url: request.url.clone(),
            accept: request.accept.clone().unwrap_or_default(),
            status: 0,
            content_type: String::new(),
            vary: String::new(),
            link: String::new(),
            location: String::new(),
            cache_control: String::new(),
            cors: String::new(),
            body: None,
        });
    }
    replies
}

/// A per-request temporary file to receive a response body. Named from the
/// process id and a nonce captured once per `fetch` call, so two calls
/// (sequential or, in principle, concurrent) never collide.
fn body_path(nonce: u128, index: usize) -> PathBuf {
    std::env::temp_dir().join(format!("iyo-http-{}-{nonce}-{index}.body", process::id()))
}

/// Run one batch of requests and parse `curl`'s report, in the batch's own
/// order regardless of which request's `curl` process finishes first.
///
/// Bodies are read only when `want_body` is true. `probe` makes over a
/// thousand requests and has no use for bodies, so its calls keep writing to
/// the null device exactly as before; `conform` asks for bodies and pays the
/// cost of a temporary file per request to get them.
///
/// The batch is split into at most [`CONCURRENCY`] chunks, each run as its
/// own `curl` process (chained internally with `--next`, exactly as a whole
/// batch used to run) on its own thread, so wall-clock time is bounded by
/// the slowest chunk rather than the sum of every request in the batch.
pub fn fetch(batch: &[Request], timeout: u32, want_body: bool) -> Result<Vec<Reply>> {
    if batch.is_empty() {
        return Ok(Vec::new());
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let workers = CONCURRENCY.min(batch.len());
    let chunk_size = batch.len().div_ceil(workers);

    std::thread::scope(|scope| {
        // Each chunk knows its own offset into `batch`, so a reply is
        // placed at `offset + i` once its chunk's thread returns -- not at
        // whatever position the next free slot happens to be. That is what
        // makes the result order independent of which thread finishes
        // first.
        let handles: Vec<_> = batch
            .chunks(chunk_size)
            .scan(0usize, |offset, chunk| {
                let start = *offset;
                *offset += chunk.len();
                Some((start, chunk))
            })
            .map(|(offset, chunk)| {
                scope.spawn(move || {
                    (
                        offset,
                        fetch_chunk(chunk, offset, timeout, want_body, nonce),
                    )
                })
            })
            .collect();

        let mut slots: Vec<Option<Reply>> = (0..batch.len()).map(|_| None).collect();
        for handle in handles {
            let (offset, chunk_replies) = handle.join().expect("curl worker thread panicked");
            let chunk_replies = chunk_replies?;
            for (i, reply) in chunk_replies.into_iter().enumerate() {
                slots[offset + i] = Some(reply);
            }
        }
        Ok(slots
            .into_iter()
            .map(|reply| reply.expect("every batch index is covered by exactly one chunk"))
            .collect())
    })
}

/// Run one chunk of a batch -- in order within the chunk -- as a single
/// `curl` process on the calling thread. `offset` is the chunk's position in
/// the original batch, so its temporary body files (named from it) never
/// collide with another chunk running concurrently on another thread.
fn fetch_chunk(
    chunk: &[Request],
    offset: usize,
    timeout: u32,
    want_body: bool,
    nonce: u128,
) -> Result<Vec<Reply>> {
    if chunk.is_empty() {
        return Ok(Vec::new());
    }

    let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let paths: Vec<PathBuf> = if want_body {
        (0..chunk.len())
            .map(|i| body_path(nonce, offset + i))
            .collect()
    } else {
        Vec::new()
    };
    let outputs: Vec<String> = (0..chunk.len())
        .map(|i| {
            if want_body {
                paths[i].to_string_lossy().into_owned()
            } else {
                null_device.to_owned()
            }
        })
        .collect();

    let outcome = (|| -> Result<Vec<Reply>> {
        let mut command = Command::new("curl");
        let timeout_str = timeout.to_string();
        command.args([
            "-sS",
            "--globoff",
            // Credentials for a protected staging origin can live in
            // ~/.netrc instead of in `--origin`, where they would be in
            // argv and in shell history before anything could redact them
            // (clig.dev G16). Optional: no file, no change in behaviour.
            "--netrc-optional",
            "--max-time",
            &timeout_str,
            "-o",
            &outputs[0],
            "-w",
            WRITE_OUT,
        ]);
        for (index, request) in chunk.iter().enumerate() {
            if index > 0 {
                command.arg("--next");
                // `--next` resets only some options, so the ones that
                // matter are repeated rather than assumed to carry over.
                command.args([
                    "-sS",
                    "--globoff",
                    "--netrc-optional",
                    "--max-time",
                    &timeout_str,
                    "-o",
                    &outputs[index],
                    "-w",
                    WRITE_OUT,
                ]);
            }
            if let Some(accept) = &request.accept {
                command.args(["-H", &format!("Accept: {accept}")]);
            }
            command.arg(&request.url);
        }

        let output = command.output().context("running curl")?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut replies = parse_all(&text, chunk);
        if want_body {
            for (reply, path) in replies.iter_mut().zip(paths.iter()) {
                reply.body = std::fs::read_to_string(path).ok();
            }
        }
        Ok(replies)
    })();

    // Clean up whether or not the run succeeded, so a request that fails
    // mid-chunk does not leave temporary files behind.
    for path in &paths {
        let _ = std::fs::remove_file(path);
    }

    outcome
}

/// Whether stderr is a terminal, so progress output (clig.dev G22) can be
/// limited to where a human is actually watching -- the same idea as
/// `cli::should_color`'s TTY check, applied to stderr instead of stdout so
/// it works the same whether or not `--json` or a pipe is on stdout.
pub fn stderr_is_terminal() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stderr())
}

/// Replace a URL's userinfo password with `***`, keeping the username, so a
/// credential passed via `--origin https://user:pass@host` is still sent on
/// the wire (that is what userinfo is for) but never written back out in a
/// report, a log line or an error message. A URL with no userinfo, or with
/// a username and no password, is returned unchanged.
pub fn redact_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let after_scheme = &url[scheme_end + 3..];
    // The authority ends at the first '/', '?' or '#', or the end of the
    // string; userinfo, when present, is the part of the authority before
    // its last '@'.
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let Some(at) = authority.rfind('@') else {
        return url.to_owned();
    };
    let userinfo = &authority[..at];
    let Some(colon) = userinfo.find(':') else {
        return url.to_owned();
    };
    let user = &userinfo[..colon];
    let at_absolute = scheme_end + 3 + at;
    format!(
        "{}{user}:***{}",
        &url[..scheme_end + 3],
        &url[at_absolute..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{Duration, Instant};

    #[test]
    fn a_reply_is_parsed_from_curls_tab_separated_report() {
        // Tab separated because a header value may contain a comma or a
        // space, and `Link` always does.
        let line = "303\ttext/plain\tAccept\t<a>; rel=\"canonical\"\thttps://x/y.ttl\tpublic, max-age=60\t*";
        let reply = parse(line, "https://x/y", "text/turtle");
        assert_eq!(reply.status, 303);
        assert_eq!(reply.location, "https://x/y.ttl");
        assert_eq!(reply.cache_control, "public, max-age=60");
        assert_eq!(reply.cors, "*");
        assert!(reply.body.is_none());
    }

    #[test]
    fn clamped_timeout_bounds_a_batchs_worst_case_to_the_remaining_deadline() {
        // BATCH=40, CONCURRENCY=8: a full batch's slowest chunk chains 5
        // requests deep. 50s left, clamped to 50/5 = 10s per request, so
        // that chunk's worst case (5 × 10s = 50s) lands on the deadline
        // rather than at 5 × the un-clamped 15s --timeout (75s).
        assert_eq!(
            clamped_timeout(Duration::from_secs(50), 40, 15),
            10,
            "50/5 undercuts the 15s --timeout, so the clamp should apply"
        );
        // The clamp never raises the per-request timeout past what
        // --timeout asked for, even with deadline to spare.
        assert_eq!(
            clamped_timeout(Duration::from_secs(1000), 40, 15),
            15,
            "plenty of deadline left should leave --timeout untouched"
        );
        // Never zero: `--max-time 0` tells curl to wait forever, the
        // opposite of a deadline that has just about run out.
        assert_eq!(
            clamped_timeout(Duration::from_millis(1), 40, 15),
            1,
            "an almost-exhausted deadline must still floor at one second"
        );
        // A batch too small to fill every worker has a shallower chunk
        // depth, so it gets a proportionally larger per-request budget.
        assert_eq!(
            clamped_timeout(Duration::from_secs(100), 8, 15),
            15,
            "one request per worker (chunk_depth 1) needs no clamp at 100s left"
        );
    }

    #[test]
    fn varies_on_accept_is_a_token_match_not_a_substring_match() {
        // "Accept-Encoding" and "Accept-Language" both contain the
        // substring "accept"; neither means the response varies on Accept.
        fn vary(value: &str) -> bool {
            Reply {
                url: String::new(),
                accept: String::new(),
                status: 0,
                content_type: String::new(),
                vary: value.to_owned(),
                link: String::new(),
                location: String::new(),
                cache_control: String::new(),
                cors: String::new(),
                body: None,
            }
            .varies_on_accept()
        }

        assert!(vary("Accept"));
        assert!(vary("accept"));
        assert!(!vary("Accept-Encoding"));
        assert!(vary("Accept-Encoding, Accept"));
        assert!(!vary("Accept-Language"));
        assert!(!vary(""));
    }

    #[test]
    fn content_type_is_ignores_a_charset_parameter_and_case() {
        // A real server almost always sends a charset. Comparing the header
        // byte for byte against a bare media type would fail every
        // compliant host, this crate's own `serve` included.
        fn matches(content_type: &str, media_type: &str) -> bool {
            Reply {
                url: String::new(),
                accept: String::new(),
                status: 0,
                content_type: content_type.to_owned(),
                vary: String::new(),
                link: String::new(),
                location: String::new(),
                cache_control: String::new(),
                cors: String::new(),
                body: None,
            }
            .content_type_is(media_type)
        }

        assert!(matches("text/turtle; charset=utf-8", "text/turtle"));
        assert!(matches("TEXT/HTML", "text/html"));
        assert!(!matches("text/plain", "text/turtle"));
        assert!(!matches("", "text/turtle"));
    }

    #[test]
    fn a_request_that_produced_no_line_is_a_failure_to_reach_the_host() {
        // Silently dropping it would turn an outage into a clean report.
        let batch = vec![Request {
            url: "https://x/y".to_owned(),
            accept: None,
        }];
        let replies = parse_all("", &batch);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].status, 0);
    }

    #[test]
    fn fetch_with_want_body_reads_the_body_back_and_removes_the_temp_file() {
        // Nothing else exercises `want_body: true` -- `probe` never sets it
        // and `conform` does not exist yet -- so this is the only thing
        // that would catch a body silently swallowed or a temp file left
        // behind. `file://` keeps this a filesystem test, not a network
        // one: `curl` supports it natively, with no server on either end.
        if !available() {
            eprintln!("skipped: curl is not on PATH");
            return;
        }

        let mut fixture = std::env::temp_dir();
        fixture.push(format!("iyo-http-fetch-fixture-{}.txt", process::id()));
        std::fs::write(&fixture, "hello from the fixture").expect("write fixture");

        let batch = vec![Request {
            url: format!("file://{}", fixture.display()),
            accept: None,
        }];
        let replies = fetch(&batch, 5, true).expect("fetch");

        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].body.as_deref(), Some("hello from the fixture"));

        // `fetch`'s per-request temp files are named from this process id;
        // anything matching that prefix still around after the call is a
        // leak `fetch`'s cleanup pass should have caught.
        let prefix = format!("iyo-http-{}-", process::id());
        let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .expect("read temp dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp body file(s) left behind: {leftovers:?}"
        );

        let _ = std::fs::remove_file(&fixture);
    }

    /// A minimal HTTP/1.1 server for exercising `fetch`'s concurrency
    /// without a server crate. `routes` maps a request path to the status
    /// code it answers with and whether it sleeps first, so a reply can be
    /// matched back to the request that produced it by status code alone,
    /// regardless of which connection the server happened to finish first.
    fn spawn_status_server(routes: &[(&str, u16, bool)]) -> u16 {
        use std::io::Read;
        use std::net::TcpListener;

        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let n = routes.len();
        let table: Vec<(String, u16, bool)> = routes
            .iter()
            .map(|&(path, status, slow)| (path.to_owned(), status, slow))
            .collect();

        std::thread::spawn(move || {
            for stream in listener.incoming().take(n) {
                let Ok(mut stream) = stream else { continue };
                let table = table.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 1024];
                    let read = stream.read(&mut buf).unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..read]);
                    let path = request.split_whitespace().nth(1).unwrap_or("/");
                    let (status, slow) = table
                        .iter()
                        .find(|(p, _, _)| p == path)
                        .map(|&(_, status, slow)| (status, slow))
                        .unwrap_or((404, false));
                    if slow {
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    let response = format!(
                        "HTTP/1.1 {status} x\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                    );
                    let _ = stream.write_all(response.as_bytes());
                });
            }
        });
        port
    }

    #[test]
    fn a_batch_of_concurrent_replies_comes_back_in_request_order() {
        // 16 requests, above CONCURRENCY (8): workers = min(8, 16) = 8 and
        // chunk_size = 16.div_ceil(8) = 2, so every chunk chains two
        // requests through one curl process. Four requests (the previous
        // version of this test) put exactly one per chunk, so the
        // positional pairing *inside* a chunk -- splitting one curl
        // process's `--next`-chained output back into its replies, in
        // order, which is what `parse_all` actually does -- was never
        // exercised, only the across-chunk offset bookkeeping in `fetch`.
        // Every other request is the slow one, so every chunk pairs a slow
        // reply with a fast one and a broken split or a wrong offset both
        // show up as a status list that is not 200, 201, 202, ....
        //
        // This does not by itself prove the across-chunk placement is
        // offset-based rather than completion-order-based: `fetch` joins
        // its handles in submission order, so a naive
        // `replies.extend(handle.join()...)` with no `offset` at all would
        // reproduce the right order too, purely because join() blocks in
        // the order this loop calls it, not in whichever chunk actually
        // finishes first. `tests/probe.rs::a_correct_site_passes_every_probe`
        // is the test that exercises this at a scale worth trusting: 119
        // requests through 5-deep chunks.
        if !available() {
            eprintln!("skipped: curl is not on PATH");
            return;
        }

        let paths: Vec<String> = (0..16).map(|i| format!("/{i}")).collect();
        let routes: Vec<(&str, u16, bool)> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| (path.as_str(), 200 + i as u16, i % 2 == 0))
            .collect();
        let port = spawn_status_server(&routes);
        let base = format!("http://127.0.0.1:{port}");
        let batch: Vec<Request> = routes
            .iter()
            .map(|(path, _, _)| Request {
                url: format!("{base}{path}"),
                accept: None,
            })
            .collect();

        let started = Instant::now();
        let replies = fetch(&batch, 5, false).expect("fetch");
        let elapsed = started.elapsed();

        let statuses: Vec<u16> = replies.iter().map(|r| r.status).collect();
        let expected: Vec<u16> = (0..16u16).map(|i| 200 + i).collect();
        assert_eq!(
            statuses, expected,
            "a reply must be paired with the request at its own position, \
             not with whichever request's curl process finished first, and \
             not with whichever request shares its chunk"
        );
        let urls: Vec<&str> = replies.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            batch.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            "reply order must match request order"
        );
        // Each of the 8 chunks chains one 400ms request with one instant
        // one, so a correct, concurrent `fetch` finishes close to a single
        // 400ms delay; serialized chunks (the pre-concurrency behaviour)
        // would sum all eight to 3.2s. The bound is left with generous
        // headroom on purpose -- it only has to separate "overlapped" from
        // "serialized", nowhere near a close call, so ordinary CI
        // scheduling noise should not flip it (the previous 700ms bound,
        // barely above the 800ms two *adjacent* slow requests would already
        // sum to, did not have that margin).
        assert!(
            elapsed < Duration::from_millis(1500),
            "took {elapsed:?}; the eight chunks should overlap, not serialize"
        );
    }

    #[test]
    fn redact_url_masks_a_userinfo_password_and_keeps_the_username() {
        assert_eq!(
            redact_url("https://user:hunter2@example.org/x"),
            "https://user:***@example.org/x"
        );
        // No path at all.
        assert_eq!(
            redact_url("https://user:hunter2@example.org"),
            "https://user:***@example.org"
        );
    }

    #[test]
    fn redact_url_leaves_a_url_with_no_credential_alone() {
        assert_eq!(
            redact_url("https://example.org/x?y=1"),
            "https://example.org/x?y=1"
        );
        // A bare username with no password has no secret to mask.
        assert_eq!(
            redact_url("https://user@example.org/"),
            "https://user@example.org/"
        );
        // Not a URL at all: left alone rather than panicking.
        assert_eq!(redact_url("not-a-url"), "not-a-url");
    }
}
