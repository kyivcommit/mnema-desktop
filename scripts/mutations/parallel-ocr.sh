# Mutation cases for parallel OCR (D196): the wire to an `--ocr` helper, the
# pipeline that keeps pages in order, the helpers' lifetime. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/parallel-ocr.sh
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
