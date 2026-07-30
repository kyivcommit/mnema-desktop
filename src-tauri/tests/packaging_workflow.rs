//! Two settings in the packaging job whose absence is silent.
//!
//! This is a lint over a configuration file, which normally earns its keep only
//! by accident. It earns it here because both properties below fail the same
//! way: the job stays green and produces nothing, which is the defect this
//! repository has now shipped twice — a `--ignored` test filter that ran zero
//! tests and passed (task 7), and a mutation harness that applied nothing and
//! reported a result (tasks 6 and 8). Nobody notices a bundle job that goes
//! green without a bundle until the day they go looking for the download.
//!
//! Nothing here checks that the workflow *works*. It cannot: a GitHub runner is
//! not reachable from a test, and as of this file no run of `ci.yml` has ever
//! happened. These assert the two lines whose removal is invisible, in the job
//! that has to carry them, and nothing more.
//!
//! # Where this check stops, and why the line is there
//!
//! What it sees: that a handful of exact lines exist, as whole lines with
//! comments stripped, inside the body of the job that has to carry each one.
//! It reads **both** jobs — `bundle` for the two packaging settings, `check`
//! for the four whose loss would not show. Reading only `bundle` was itself an
//! instance of the defect: the `--include-ignored` line this file's own header
//! cites as its reason lives in `check`, and nothing guarded it.
//!
//! What it does not see: **whether those steps run.** A step is satisfied by its
//! text alone, so a decoy defeats this — and the decoy is small. Measured, not
//! predicted: giving the verification step `if: false` and moving
//! `if-no-files-found: error` onto a second, also-disabled `upload-artifact`
//! left every test below green with neither real step doing anything.
//!
//! Closing that needs a YAML parser and an evaluator for `if:` expressions.
//! This workspace has no YAML dependency and takes one on for nothing else, and
//! a lint over a config file does not justify the first — so the line is drawn
//! here on purpose rather than by oversight.
//!
//! The reason the boundary is written down at all: this is the fifth time the
//! same family has been closed in this repository, each time one layer deeper —
//! a substring satisfied by prose (task 6), a whole line satisfied by a comment
//! (task 8), a whole line found in the wrong job (task 9, review round 1), and
//! now a whole line in the right job on a step that never executes. Whoever
//! meets it a sixth time should know which layer they are standing on.

use std::path::Path;

fn workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has no parent")
        .join(".github/workflows/ci.yml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// Whether this line opens a top-level job: exactly two spaces of indent, then
/// something that is not a comment.
///
/// Two spaces is the whole rule, and it is why three spaces are rejected
/// separately — every line *inside* a job is indented deeper, and a `#` at that
/// depth is a comment about a step rather than a sibling job.
fn opens_a_job(line: &str) -> bool {
    line.starts_with("  ")
        && !line.starts_with("   ")
        && !line.trim().is_empty()
        && !line.trim_start().starts_with('#')
}

/// The body of one job, as text.
///
/// The slice is the point of this file. `declares` used to search the whole
/// workflow, and the review broke it in the way that matters: the `bundle` job
/// was gutted — the verification step removed and `if-no-files-found` removed so
/// the upload falls back to `warn` — while both lines were pasted verbatim into
/// a `dead:` job carrying `if: false`. **Both tests passed.** Stripping comments
/// closes that family at the character level; this closes it at the location
/// level, and the second does not follow from the first.
///
/// It is not hypothetical either: the first moment a second job carries an
/// `upload-artifact` — a Linux package, a nightly — a whole-file search is
/// satisfied by whichever one happens to be right.
fn job(workflow: &str, name: &str) -> Option<String> {
    let header = format!("  {name}:");
    let mut lines = workflow
        .lines()
        .skip_while(|line| line.trim_end() != header);
    // Consumes the header itself, and answers None when it was never found —
    // rather than returning the tail of the file from wherever `skip_while`
    // stopped, which for a wrong name is nothing at all and for a missing job
    // would otherwise be an empty body that every assertion fails against for
    // the wrong reason.
    lines.next()?;
    Some(
        lines
            .take_while(|line| !opens_a_job(line))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn named_job(name: &str) -> String {
    let workflow = workflow();
    job(&workflow, name).unwrap_or_else(|| {
        panic!(
            ".github/workflows/ci.yml has no `{name}:` job. The checks below are \
             about lines that have to live in that job; with no such job there is \
             nothing left to be silent about, and a test that quietly passed here \
             would be the very defect it exists to catch."
        )
    })
}

fn bundle_job() -> String {
    named_job("bundle")
}

fn check_job() -> String {
    named_job("check")
}

/// Whether the text declares this exact line.
///
/// Whole lines with any trailing comment removed, NOT a substring search. Both
/// lines below are *discussed* in the comments around them — the upload comment
/// names `if-no-files-found` while explaining what its default does — so "the
/// text appears in the file" and "the setting is present" are different
/// questions here, and a substring search answers the wrong one. That exact
/// defect has now been closed four times in this repository: a README check in
/// task 6, the release-profile check in task 8, and here on both axes.
fn declares(text: &str, line: &str) -> bool {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .any(|l| l == line)
}

#[test]
fn the_job_slice_starts_at_the_job_it_names() {
    // Without this, a `job()` that silently returned the whole file would leave
    // the two tests below passing exactly as they did before the slice existed —
    // the hole closed and reopened inside the fix for it. So: something only the
    // other job has must be absent, and something only this job has present.
    // Both spellings carry the list dash, because that is what the line in the
    // file is. Written without it, the positive assertion fails and the negative
    // one passes against every possible file — which is how the first version of
    // this test was green on its control and red on its subject.
    let bundle = bundle_job();
    assert!(
        !declares(&bundle, "- run: cargo fmt --all -- --check"),
        "the `bundle` job slice contains a line belonging to `check`, so the \
         slice is not slicing. Everything below is then a whole-file search \
         wearing a job's name.\nThe slice reads:\n{bundle}"
    );
    assert!(
        declares(&bundle, "- run: cargo tauri build"),
        "the `bundle` job slice does not contain `cargo tauri build`, so either \
         it starts in the wrong place or the job no longer builds \
         anything.\nThe slice reads:\n{bundle}"
    );

    // The mirror image, and it earns its place for a reason the `bundle` pair
    // cannot: `check` is NOT the last job, so its body has a real end in the
    // real file. This is the only assertion here that exercises `opens_a_job`
    // against ci.yml itself rather than against the document built for it.
    let check = check_job();
    assert!(
        !declares(&check, "- run: cargo tauri build"),
        "the `check` job slice runs on into `bundle`. Its end condition is the \
         one thing the two-job document below cannot prove about the real \
         file.\nThe slice reads:\n{check}"
    );
}

#[test]
fn a_job_body_stops_where_the_next_job_starts() {
    // The real file cannot show this. `bundle` is the LAST job in ci.yml, so its
    // body runs to the end of the file whether the end condition works or not —
    // every clause of `opens_a_job` could be deleted and the test above would
    // still pass. And a second job after `bundle` is not a hypothetical: it is
    // precisely where the review pasted the two missing lines to prove the
    // whole-file search was blind. So the boundary is checked here, on a
    // document built to have one.
    //
    // The line between the header and the body is two spaces and nothing else:
    // invisible in a diff, ordinary in a hand-edited YAML file, and to a rule
    // that only counts indentation it looks exactly like the start of a job. It
    // is written as its own piece so an editor that trims trailing whitespace
    // cannot quietly disarm the case.
    let two_jobs = format!(
        "jobs:\n\
         \x20 bundle:\n\
         \x20 # a note written at job depth, inside the job\n\
         {}\n\
         \x20   steps:\n\
         \x20     - run: scripts/verify-bundle.sh\n\
         \x20 dead:\n\
         \x20   if: false\n\
         \x20   steps:\n\
         \x20     - run: smuggled\n",
        "  "
    );

    let bundle = job(&two_jobs, "bundle").expect("the document has a bundle job");
    assert!(
        declares(&bundle, "- run: scripts/verify-bundle.sh"),
        "the body was cut short before its own steps. A comment or a \
         whitespace-only line at job depth is not the start of a job, and \
         treating it as one truncates the body — which makes every check below \
         fail for a reason that has nothing to do with what it \
         asserts.\nThe slice reads:\n{bundle}"
    );
    assert!(
        !declares(&bundle, "- run: smuggled"),
        "the body ran past its own job into the next one. That is the whole \
         defect this slice exists for: a line living in a disabled job would \
         satisfy a check about the job that ships.\nThe slice reads:\n{bundle}"
    );
    assert!(
        job(&two_jobs, "nonesuch").is_none(),
        "a job name that does not occur answered with a body instead of None. \
         `skip_while` runs off the end and the tail is empty, so the caller \
         would then assert against an empty string and report a missing setting \
         in a job that does not exist."
    );
}

#[test]
fn the_artefact_upload_refuses_to_succeed_with_nothing_to_upload() {
    assert!(
        declares(&bundle_job(), "if-no-files-found: error"),
        "the `bundle` job in .github/workflows/ci.yml does not set \
         `if-no-files-found: error` on the artefact upload. The action's default \
         is `warn`: with no .dmg matching the path — or a .dmg written somewhere \
         else, which is what `--target universal-apple-darwin` does — it uploads \
         nothing, logs a warning and exits 0. The bundle job then passes without \
         a bundle, which is the one thing it exists to prevent. Setting it in \
         some other job does not count, and is why this reads one job only."
    );
}

#[test]
fn the_bundle_job_checks_what_it_built() {
    assert!(
        declares(&bundle_job(), "run: scripts/verify-bundle.sh"),
        "the `bundle` job in .github/workflows/ci.yml does not run \
         scripts/verify-bundle.sh. `cargo tauri build` exiting 0 says a command \
         finished, not that a signed application is inside the image — the first \
         bundle this repository ever produced exited 0 and carried an .app whose \
         signature did not verify. That script is the only place the difference \
         is checked."
    );
}

#[test]
fn the_check_job_keeps_the_lines_whose_loss_would_not_show() {
    // This test exists because of the module doc above it. That doc cites task
    // 7's `--ignored` filter — a step that ran zero tests and passed — as the
    // reason this file exists, and the line that closed it, `--include-ignored`,
    // was itself guarded by nothing: put `--ignored` back and the whole
    // workspace stayed green. The lint named the defect in its own header and
    // did not cover the line that fixed it.
    //
    // The bar is not "reddens nothing here" — that is true of `cargo fmt` too,
    // and deleting it is loud: the step disappears from the run and from the log.
    // The bar is that the run stays GREEN WHILE PROVING LESS, and the loss shows
    // up as no failure anywhere. Removing any of the four below leaves a workflow
    // that passes having checked less than its name says.
    //
    // One of them, `fetch-pdfium.sh`, is here for a different reason and it is
    // worth saying so rather than stretching the bar to cover it: deleting it IS
    // loud — two tests in mnema-extract go red on a missing library. It is
    // guarded because the natural repair for a step failing that way is to delete
    // the tests, and that repair is silent.
    let check = check_job();

    assert!(
        declares(&check, "os: [macos-14, ubuntu-24.04]"),
        "the `check` job no longer builds on both platforms. Dropping an arm of \
         this matrix reddens nothing: the run goes green having compiled the \
         crate list on one target, which is what `ci.yml` argues at length is \
         almost no check at all. Linux is where Tauri is weakest — it builds \
         against a packaged WebKitGTK rather than an OS component — so it is the \
         arm most worth losing and the one whose loss is most invisible.\nThe \
         slice reads:\n{check}"
    );

    assert!(
        declares(
            &check,
            "cargo test -p mnema-secrets --test roundtrip -- --include-ignored"
        ),
        "the `check` job no longer runs the credential-store test with \
         `--include-ignored`. `--ignored` selects ONLY ignored tests, so the day \
         the attribute is removed that step runs zero tests and exits 0 — a green \
         step certifying nothing, in the one place that proves the macOS keychain \
         binding works. Measured in task 7: `0 passed; 14 filtered out`, step \
         green.\nThe slice reads:\n{check}"
    );

    assert!(
        declares(&check, "- run: cargo test --workspace"),
        "the `check` job no longer runs the test suite. Everything else in this \
         file guards a setting inside a job that still runs the tests; without \
         this line the matrix is a build check wearing the name of a test \
         run.\nThe slice reads:\n{check}"
    );

    assert!(
        declares(&check, "run: scripts/fetch-pdfium.sh"),
        "the `check` job no longer vendors Pdfium. The library is not committed, \
         so without it mnema-extract's tests fail on a missing file rather than \
         on anything they are about — and the natural repair for a step failing \
         that way is to delete the tests.\nThe slice reads:\n{check}"
    );
}

/// The job's declared limit in minutes, if it declares one.
///
/// The value is returned rather than compared, because the number is a judgement
/// and its absence is a defect — those are different claims and only the second
/// belongs in a test. Comments are stripped the way `declares` strips them: the
/// limit is explained in prose directly above itself in `ci.yml`, so a substring
/// search for `timeout-minutes` is satisfied by the explanation of why it is
/// there, which is the failure this whole file exists to refuse.
fn declared_timeout(text: &str) -> Option<u32> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .find_map(|line| line.strip_prefix("timeout-minutes:"))
        .and_then(|value| value.trim().parse().ok())
}

#[test]
fn every_job_declares_a_time_limit() {
    // The third member of this file's family, and the one whose absence is the
    // quietest of the three. A missing artefact at least produces a job that
    // ended; a job with no limit that wedges produces **no result at all** —
    // GitHub's default holds the runner for six hours, reporting "in progress"
    // the entire time, which reads as a slow build rather than a stuck one.
    //
    // Not hypothetical, and not about our code: on 2026-07-29 the `apt-get`
    // step in `check` — 51 seconds in the run before it, on the same workflow —
    // hung on a runner, on a commit that touched one markdown file. It was
    // fifteen minutes in and had produced nothing when a person went looking.
    // Nothing in the repository would have ended it.
    //
    // Zero is rejected explicitly. GitHub reads `timeout-minutes: 0` as a job
    // that may never run to completion, so it is the one written value that is
    // worse than the missing line this test is here to catch.
    for name in ["check", "bundle"] {
        let body = named_job(name);
        let limit = declared_timeout(&body).unwrap_or_else(|| {
            panic!(
                "the `{name}` job declares no `timeout-minutes`. GitHub's default \
                 then applies, and a step that hangs holds a runner for six hours \
                 while the job reports as still running — no failure, no artefact, \
                 no signal that anything is wrong. Measured durations are in the \
                 comments beside each limit in ci.yml.\nThe slice reads:\n{body}"
            )
        });
        assert!(
            limit > 0,
            "the `{name}` job declares `timeout-minutes: {limit}`, which GitHub \
             treats as no limit at all — the same outcome as omitting the line, \
             reached by writing one."
        );
    }
}
