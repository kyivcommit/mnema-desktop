# Mutation cases for parallel OCR (D196): the wire to an `--ocr` helper, the
# pipeline that keeps pages in order, the helpers' lifetime. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/parallel-ocr.sh
#
# Only cases whose tests run on the Linux leg are here; the ones that need
# Vision or the worker's helpers (macOS only) are in `parallel-ocr-macos.sh`,
# which nothing runs automatically — run it by hand before a PR.
#
# The wire is a trust boundary inside one machine: a helper reads a header and
# allocates what it names. Every geometry check below is broken on its own,
# because a reader that skips one still refuses the others' cases.

# W1. len is the geometry's own product. Dropped, a header naming more bytes
# than its rows has the helper read pixels that belong to the next frame.
case_ "the wire refuses a len that is not stride * height" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~        && h\.stride\.checked_mul\(u64::from\(h\.height\)\) == Some\(h\.len\)\n~~' \
  '        && h.len <= MAX_IMAGE_BYTES;' \
  mnema-extract 'a_header_that_does_not_add_up_is_refused_before_reading_the_pixels' --test ocr_wire

# W2. A row is at least width * 4 bytes, or Vision reads past it.
case_ "the wire refuses a stride narrower than a BGRA row" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~        && h\.stride >= u64::from\(h\.width\) \* 4\n~~' \
  '        && h.height <= MAX_IMAGE_SIDE' \
  mnema-extract 'a_header_that_does_not_add_up_is_refused_before_reading_the_pixels' --test ocr_wire

# W3. An empty image is not a page.
case_ "the wire refuses a zero-sized image" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~    let fits = h\.width > 0\n        && h\.height > 0\n        && ~    let fits = ~' \
  '    let fits = h.width <= MAX_IMAGE_SIDE' \
  mnema-extract 'a_header_that_does_not_add_up_is_refused_before_reading_the_pixels' --test ocr_wire

# W4. The side ceiling.
case_ "the wire refuses a side past MAX_IMAGE_SIDE" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~        && h\.width <= MAX_IMAGE_SIDE\n~~' \
  '        && h.height <= MAX_IMAGE_SIDE' \
  mnema-extract 'a_header_that_does_not_add_up_is_refused_before_reading_the_pixels' --test ocr_wire

# W5. The byte ceiling, for a geometry whose sides are inside theirs.
case_ "the wire refuses a len past MAX_IMAGE_BYTES" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~\n        && h\.len <= MAX_IMAGE_BYTES;~;~' \
  '== Some(h.len);' \
  mnema-extract 'a_header_that_does_not_add_up_is_refused_before_reading_the_pixels' --test ocr_wire

# W6. Only the rows go out. Sending the whole buffer, the receiver's exact
# length check sees a frame that does not add up — or, worse, the next header
# starts inside the tail.
case_ "the wire sends stride * height bytes, not the whole buffer" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~    w\.write_all\(&image\.bgra\[\.\.len\]\)~    w.write_all(image.bgra)~' \
  '    w.write_all(image.bgra)' \
  mnema-extract 'only_the_rows_the_geometry_names_are_sent' --test ocr_wire

# W7. EOF where a header would start is the end of the work, not a failure:
# otherwise a helper whose worker closed stdin exits as if it had crashed.
case_ "a clean EOF between frames is None" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~    if n == 0 \{\n        return Ok\(None\);~    if n == 0 {\n        return Err(WireError::Io(io::ErrorKind::UnexpectedEof.into()));~' \
  'return Err(WireError::Io(io::ErrorKind::UnexpectedEof.into()));' \
  mnema-extract 'two_requests_in_a_row_arrive_in_order_and_then_a_clean_end' --test ocr_wire

# W8. "No engine on this platform" and "engine unavailable" are different
# answers: the first leaves a scan skipped as before OCR, the second fails it.
case_ "a reply of no engine is not sent as unavailable" \
  crates/mnema-extract/src/ocr_wire.rs \
  's~        Ok\(None\) => Answer::None\(true\),~        Ok(None) => Answer::Unavailable(String::new()),~' \
  'Ok(None) => Answer::Unavailable(String::new()),' \
  mnema-extract 'every_reply_comes_back_as_itself' --test ocr_wire

# H1. A helper whose worker closed stdin has finished, not failed. Exiting 2
# there, every helper of every file would read to the worker as a crash.
case_ "a helper exits 0 on EOF between frames" \
  crates/mnema-extract/src/bin/worker.rs \
  's~            Ok\(None\) => return 0,~            Ok(None) => return 2,~' \
  '            Ok(None) => return 2,' \
  mnema-extract 'a_helper_whose_stdin_closes_before_any_frame_exits_cleanly_and_says_nothing' --test ocr_helper

# H2. Garbage is a failure the worker can see, not a quiet end.
case_ "a helper given garbage does not exit 0" \
  crates/mnema-extract/src/bin/worker.rs \
  's~                eprintln!\("mnema-extract-worker --ocr: \{e\}"\);\n                return 2;~                eprintln!("mnema-extract-worker --ocr: {e}");\n                return 0;~' \
  '                return 0;' \
  mnema-extract 'a_helper_given_garbage_exits_with_failure_instead_of_waiting' --test ocr_helper

# H3. The reply carries the seq it answers; a constant one would put every
# page's text under the first page's number.
case_ "a helper echoes the seq it was sent" \
  crates/mnema-extract/src/bin/worker.rs \
  's~write_reply\(&mut stdout, seq, &reply\)~write_reply(\&mut stdout, 0, \&reply)~' \
  'write_reply(&mut stdout, 0, &reply)' \
  mnema-extract 'each_frame_gets_one_reply_under_its_own_seq_in_order' --test ocr_helper

# H4. The crash hook counts replies; off by one, the fallback test in task 6
# would be testing a helper that never answered at all.
case_ "the crash hook fires after k replies, not before" \
  crates/mnema-extract/src/bin/worker.rs \
  's~        if crash_after == Some\(served\) \{~        if crash_after == Some(served + 1) {~' \
  '        if crash_after == Some(served + 1) {' \
  mnema-extract 'the_crash_hook_kills_the_helper_after_k_replies' --test ocr_helper

# P1. "0" in the environment is no override; taken as one, it would be the
# in-process path for a machine that has eight cores to offer.
case_ "an override of zero is ignored, not clamped to one" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~        \.and_then\(\|v\| v\.parse::<usize>\(\)\.ok\(\)\)\n        \.filter\(\|&n\| n >= 1\)~        .and_then(|v| v.parse::<usize>().ok())~' \
  '        .or(p_cores.filter(|&n| n >= 1))' \
  mnema-extract 'the_environment_overrides_and_a_bad_value_is_ignored' --test ocr_parallel

# P2. A zero from sysctl is no answer.
case_ "a zero P-core count falls through to parallelism" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~        \.or\(p_cores\.filter\(\|&n\| n >= 1\)\)~        .or(p_cores)~' \
  '        .or(p_cores)' \
  mnema-extract 'without_a_p_core_count_half_the_parallelism_is_taken' --test ocr_parallel

# P3. Half the logical parallelism: all of it would count efficiency cores and
# hyperthreads the plateau does not reward.
case_ "without P-cores half the parallelism is taken" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~\.or\(parallelism\.map\(\|n\| n / 2\)\)~.or(parallelism)~' \
  '        .or(parallelism)' \
  mnema-extract 'without_a_p_core_count_half_the_parallelism_is_taken' --test ocr_parallel

# P4. The floor.
case_ "never fewer than one process" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~    chosen\.clamp\(1, MAX_PROCESSES\)~    chosen.min(MAX_PROCESSES)~' \
  '    chosen.min(MAX_PROCESSES)' \
  mnema-extract 'never_fewer_than_one' --test ocr_parallel

# P5. The ceiling.
case_ "never more than MAX_PROCESSES" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~    chosen\.clamp\(1, MAX_PROCESSES\)~    chosen.max(1)~' \
  '    chosen.max(1)' \
  mnema-extract 'performance_cores_decide_when_the_machine_names_them' --test ocr_parallel

# P6. The environment comes first.
case_ "the environment overrides the P-core count" \
  crates/mnema-extract/src/ocr_parallel.rs \
  's~    let chosen = env\n        \.and_then\(\|v\| v\.parse::<usize>\(\)\.ok\(\)\)\n        \.filter\(\|&n\| n >= 1\)\n        \.or\(p_cores\.filter\(\|&n\| n >= 1\)\)~    let chosen = p_cores\n        .filter(|\&n| n >= 1)\n        .or(env.and_then(|v| v.parse::<usize>().ok()).filter(|\&n| n >= 1))~' \
  '    let chosen = p_cores' \
  mnema-extract 'the_environment_overrides_and_a_bad_value_is_ignored' --test ocr_parallel

# Q1. A native page waits behind the scans before it. Handed over at once, it
# overtakes pages still being recognised and the caller sees 1, 4, 7, 2, …
case_ "a native page queues behind pages in flight" \
  crates/mnema-extract/src/pdf.rs \
  's~            order\.queue\.push_back\(Slot::Ready\(page_of\(\n                page_no,\n                &raw,\n                TEXT_SOURCE_NATIVE,\n                None,\n            \)\)\);~            on_page(declared, page_of(page_no, \&raw, TEXT_SOURCE_NATIVE, None));~' \
  'on_page(declared, page_of(page_no, &raw, TEXT_SOURCE_NATIVE, None));' \
  mnema-extract 'pages_answered_out_of_order_reach_the_caller_in_page_order' --test pdf_pipeline

# Q2. The backend's capacity bounds the pages in flight — each one a 13.5 MB
# image and a helper's attention.
case_ "never more pages in flight than capacity" \
  crates/mnema-extract/src/pdf.rs \
  's~            while order\.in_flight >= backend\.capacity\(\)\.max\(1\) \{~            while order.in_flight > backend.capacity().max(1) {~' \
  'while order.in_flight > backend.capacity().max(1) {' \
  mnema-extract 'never_more_pages_in_flight_than_the_backend_holds' --test pdf_pipeline

# Q3. An answer lands on the page it names. Put in the first waiting slot, a
# backend answering out of order files page 3's text under page 2.
case_ "an answer goes to the page it names" \
  crates/mnema-extract/src/pdf.rs \
  's~\.find\(\|slot\| matches!\(slot, Slot::Pending\(n\) if \*n == page_no\)\)~.find(|slot| matches!(slot, Slot::Pending(_)))~' \
  '.find(|slot| matches!(slot, Slot::Pending(_)))' \
  mnema-extract 'pages_answered_out_of_order_reach_the_caller_in_page_order' --test pdf_pipeline

# Q4. The pages still in flight when the loop ends are waited for.
case_ "the last pages in flight are waited for" \
  crates/mnema-extract/src/pdf.rs \
  's~    while order\.in_flight > 0 \{~    while order.in_flight > usize::MAX {~' \
  'while order.in_flight > usize::MAX {' \
  mnema-extract 'pages_answered_out_of_order_reach_the_caller_in_page_order' --test pdf_pipeline

# Q5. A recognised page is held to the same threshold as a native one.
case_ "a recognised page below the threshold is skipped" \
  crates/mnema-extract/src/pdf.rs \
  's~                Slot::Answered\(page_no, Ok\(Some\(read\)\)\)\n                    if has_text_layer\(text_layer_chars\(&read\.text\)\) =>~                Slot::Answered(page_no, Ok(Some(read))) =>~' \
  'Slot::Answered(page_no, Ok(Some(read))) =>' \
  mnema-extract 'pages_with_nothing_recognised_are_skipped_in_page_order' --test pdf_pipeline

# Q6. An engine failure is not "no text on this page": journalled as skipped,
# a scan the engine could not read is remembered as a blank one.
case_ "an engine failure refuses the document, it is not a skip" \
  crates/mnema-extract/src/pdf.rs \
  's~                Slot::Answered\(_, Err\(e\)\) => return Err\(PdfError::Ocr\(e\)\),~                Slot::Answered(n, Err(_)) => self.skipped.push(n),~' \
  'Slot::Answered(n, Err(_)) => self.skipped.push(n),' \
  mnema-extract 'an_engine_failure_stops_at_its_page' --test pdf_pipeline

# Q7. Answers already in are collected before every drain. Without it the
# pages behind an answered one wait for the next textless page — with
# `InProcess`, every native page after a scan waits for the end of the file
# in memory instead of streaming (security review, 2026-11-06).
case_ "answers already in are collected before the queue drains" \
  crates/mnema-extract/src/pdf.rs \
  's~        order\.collect\(backend\)\?;\n~~' \
  '        // next textless page' \
  mnema-extract 'answers_already_in_let_the_pages_behind_them_stream_on' --test pdf_pipeline
