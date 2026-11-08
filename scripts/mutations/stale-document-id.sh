# Mutation cases for a pane whose documentId outlived its document: a walk that
# re-indexed an edited file deleted the old document, and `open_document` shows
# what is now at the cited path. Run with:
#
#   scripts/mutation-check.sh scripts/mutations/stale-document-id.sh
#
# What is silent when it is wrong here is the old citation laid on the new
# text, or a half-written file shown whole. Every `case_` below has one
# mutation and one test. Expressions use `~` as the delimiter and `\Q…\E` for
# the literal Rust they replace.

TR=src-tauri/src/tree.rs

# S1. The path is never followed: an edited file reads "no longer in the index".
case_ "follow: a gone document does not look at its cited path" \
  $TR \
  's~\Qlet Some(occupant) = cited_occupant(db, root_id, relative_path)? else {\E~let Some(occupant) = None::<PathOccupant> else {~' \
  'let Some(occupant) = None::<PathOccupant> else {' \
  mnema-desktop 'open_document_follows_the_path_when_the_cited_document_is_gone' --test commands

# S2. The status gate asks about the cited id, not the one shown. With the
# cited document gone there is no status to read, so the mutant surfaces as
# the command being rejected (the test's `expect`), not as a half-written file
# shown whole — measured; either way the pane would not say "rebuilding".
case_ "follow: the status gate is asked about the cited document" \
  $TR \
  's~\Qdocument_status(&shown)\E~document_status(document_id)~' \
  'document_status(document_id)' \
  mnema-desktop 'a_gone_document_whose_path_is_being_rebuilt_is_rebuilding' --test commands

# S3. The focus is accepted for any document: the new document reuses the old
# chunk id and the same passage, so the old citation is marked on new text.
case_ "follow: the focus is not checked against the cited document" \
  $TR \
  's~\Qif anchor.document_id == document_id\E~if true~' \
  'if true' \
  mnema-desktop 'open_document_follows_the_path_when_the_cited_document_is_gone' --test commands

# S4. The followed occupant is dropped: the title loses its path and the
# freshness reads `noPath` for a file that is right there.
case_ "follow: the occupant found for the shown document is not used for freshness" \
  $TR \
  's~\Q(Some(occupant), ..) => Some(occupant),\E~(Some(_), ..) => None,~' \
  '(Some(_), ..) => None,' \
  mnema-desktop 'open_document_follows_the_path_when_the_cited_document_is_gone' --test commands

# S5. Freshness is decided for the cited document, not the one shown: a file
# opened at its current text carries a "this path names another document"
# badge beside the "passage is gone" note — two warnings for one fact.
case_ "follow: freshness is decided against the cited document, not the shown one" \
  $TR \
  's~\Qlet freshness = decide_freshness(occupant.as_ref(), &shown);\E~let freshness = decide_freshness(occupant.as_ref(), document_id);~' \
  'let freshness = decide_freshness(occupant.as_ref(), document_id);' \
  mnema-desktop 'open_document_follows_the_path_when_the_cited_document_is_gone' --test commands
