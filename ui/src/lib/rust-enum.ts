// Reading a Rust enum's variant names out of its own source text, so a window
// that mirrors one can be pinned against the file that owns it.
//
// 🔴 **Why this is a module and not a local helper.** It was a local helper
// twice — `Models.test.ts` wrote it, `jobs.test.ts` lifted it verbatim
// ("Lifted from `Models.test.ts`'s `rustEnumVariants`"), and the third mirror
// (`SubfolderState`, `ipc.test.ts`) is what made a third copy the obvious next
// move. The two copies had already drifted apart in their messages, and the
// fix that matters — stripping comments BEFORE the brace walk, below — was
// carried into the second copy by hand. One more hand-carry is one more
// chance to not carry it.
//
// ⚠️ **This is HALF of any wire guard built on it, and the half that cannot
// see its own blind spot.** The `rename_all` gap below is the one that matters
// because it fails SILENTLY; it is not the only shape this reader gets wrong.
// Two others are known and both fail LOUDLY at every current caller, which is
// why they are recorded rather than fixed: a tuple variant with more than one
// field yields a phantom entry (`Pair(String, usize)` reads as
// `["Plain", "Pair", "usize", "Last"]`), and a block comment carrying a brace
// throws with a message that does not name the cause. Neither can produce a
// false green here; a third shape that failed silently would be a defect.
// It reads variant NAMES and a caller applies
// serde's `RenameRule::CamelCase` to them; it never reads the enum's
// `#[serde(rename_all = …)]`, so switching that attribute to `snake_case`
// leaves every caller green while the wire spelling changes underneath. The
// other half is a Rust test that serializes each variant and pins the string —
// `job.rs`'s `every_end_reason_has_its_camel_case_spelling_pinned` and
// `tree.rs`'s `the_subfolder_wire_shape_is_camel_case` are the two that exist.
// Neither half closes the gap alone: the pair does.

// The variant names of `pub enum <enumName>` (or `pub(crate) enum`, `pub(in
// path::to) enum`, … — `pub` or a restricted `pub(...)`, never a private
// enum with no `pub` at all — review round 2, Nit D) in `rawSource`, in
// source order and in Rust's own spelling — `camelOf` turns one into its
// wire name.
//
// Throws rather than answering when it cannot answer: an enum it cannot find,
// a body it runs off the end of, a variant it cannot parse a name out of, or
// a variant-level `#[serde(rename = "…")]` it has no way to express.
export function rustEnumVariants(rawSource: string, enumName: string): string[] {
  // 🔴 Comments are stripped BEFORE the depth walk, not after it. The original
  // form walked the raw text and justified itself with "every doc comment
  // brace is a self-balanced pair on its own line" — a description of the
  // comments that happened to be there, not an invariant of Rust. A reviewer
  // added a variant behind a doc comment carrying a lone `}`; the walk stopped
  // at that brace, the body was truncated, and the file reported green having
  // never seen the new variant.
  const source = rawSource.split('\n').map((line) => line.replace(/\/\/.*$/, '')).join('\n');
  // `pub(?:\([^)]*\))?` — bare `pub`, or `pub` with a restricted-visibility
  // parenthetical (`(crate)`, `(super)`, `(in a::b)`, …) — added when
  // `ipc.test.ts` needed to pin `locale.rs`'s `pub(crate) enum LocaleSurface`
  // and had no way to, short of a second hand-rolled parser beside this one
  // (review round 1, Minor 7). `[^)]*` rather than `[\w:]*` so an `in a::b`
  // path's own leading/trailing detail is not this reader's business to
  // enumerate — only the closing paren matters to find the end of it.
  const m = new RegExp(`pub(?:\\([^)]*\\))?\\s+enum\\s+${enumName}\\s*\\{`).exec(source);
  if (!m) throw new Error(`enum ${enumName} not found in the Rust source — has it moved or been renamed?`);
  let depth = 1;
  let i = m.index + m[0].length;
  const start = i;
  while (depth > 0) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}') depth--;
    i++;
    if (i > source.length) throw new Error(`ran off the end of the file looking for the closing brace of ${enumName}`);
  }
  const body = source.slice(start, i - 1);
  // The mirror derives a wire name with serde's `RenameRule::CamelCase` alone
  // and has no way to express an explicit `#[serde(rename = "…")]` on a
  // variant. It cannot be made to guess one, so it says so here rather than
  // deriving a wire name that is silently wrong. `rename_all` is the
  // enum-level rule this mirror already assumes and is deliberately not
  // matched — and it sits above the body in any case.
  if (/#\[serde\([^)]*\brename\s*=/.test(body)) {
    throw new Error(
      `${enumName} now carries an explicit #[serde(rename = "…")] on a variant. This mirror derives ` +
      'wire names with serde\'s CamelCase rule alone and cannot express a rename — teach camelOf ' +
      'about it, or pin that variant\'s wire name in the caller, before trusting this test again.',
    );
  }
  const variants: string[] = [];
  let d = 0;
  let cur = '';
  for (const ch of body) {
    if (ch === '{') d++;
    if (ch === '}') d--;
    if (ch === ',' && d === 0) {
      if (cur.trim()) variants.push(cur.trim());
      cur = '';
    } else {
      cur += ch;
    }
  }
  if (cur.trim()) variants.push(cur.trim());
  return variants.map((v) => {
    const name = /^([A-Za-z0-9_]+)/.exec(v.trim());
    if (!name) throw new Error(`could not parse a variant name out of: ${v}`);
    return name[1];
  });
}

// PascalCase → camelCase the way serde's own `RenameRule::CamelCase` does it
// (`serde_derive::internals::case`): lowercase the first character, leave the
// rest exactly as written — verified against the multi-word variants already
// mirrored in `ipc.ts` (`notOpen`, `nothingToRemove`, `envelopeNotUnderstood`,
// `excludedByAncestor`, `unusableName`), none of which get an interior letter
// touched.
export const camelOf = (pascal: string): string => pascal.charAt(0).toLowerCase() + pascal.slice(1);

// Task 11a (Task 6, deferred). The struct-field sibling of `rustEnumVariants`
// above — a window that mirrors a `#[derive(Serialize)] pub struct` field for
// field needs the same pin an enum's variants get, and for the same reason:
// `ipc.ts` mirrors roughly forty fields across `ScanState`, `ScanReport`,
// `ReadingOutcome` and `RootOutcome` with no field-level guard at all before
// this, so a Rust field renamed, dropped or added reached every TS caller as
// `undefined` with nothing here to say so.
//
// The field NAMES of `pub struct <structName>`, in source order and in
// Rust's own snake_case spelling — `camelOfSnake` below turns one into its
// wire name. Comments are stripped BEFORE the depth walk, for the identical
// reason `rustEnumVariants` gives: a doc comment carrying a brace must not be
// read as one.
//
// Reads only `pub <name>: <Type>,` fields at the struct's own top level. A
// private field is invisible to it, but every field mirrored by `ipc.ts` is
// `pub`, because a private one cannot reach `serde::Serialize` at all. Depth
// is tracked over `{}`, `()` AND `<>` so a field typed `Option<Vec<T>>` does
// not have its own brackets mistaken for the struct's closing brace or its
// own comma mistaken for a field separator; it does NOT distinguish `<`/`>`
// as brackets from `<`/`>` as comparison or shift operators, which is not a
// shape any field type in this codebase takes.
//
// A field-level `#[serde(rename = "…")]` is refused with a thrown error
// rather than answered wrong, the same way `rustEnumVariants` refuses one on
// a variant: this reader derives every wire name with serde's CamelCase rule
// alone, and a silent rename would make the pin compare two lists that both
// look complete while one of them quietly holds the wrong name.
export function rustStructFields(rawSource: string, structName: string): string[] {
  const source = rawSource.split('\n').map((line) => line.replace(/\/\/.*$/, '')).join('\n');
  const m = new RegExp(`pub struct ${structName}\\s*\\{`).exec(source);
  if (!m) {
    throw new Error(`struct ${structName} not found in the Rust source — has it moved or been renamed?`);
  }
  let depth = 1;
  let i = m.index + m[0].length;
  const start = i;
  while (depth > 0) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}') depth--;
    i++;
    if (i > source.length) {
      throw new Error(`ran off the end of the file looking for the closing brace of ${structName}`);
    }
  }
  const body = source.slice(start, i - 1);

  // Task 11a fix round 1 (Minor 1). Without this, a field-level
  // `#[serde(rename = "…")]` reaches `camelOfSnake` unnoticed: this reader
  // derives every wire name with serde's snake_case→camelCase rule alone and
  // has no way to express an explicit rename, so the pin would keep comparing
  // two lists that both look complete and quietly compare the wrong names —
  // a false GREEN, not a thrown error. `rustEnumVariants` refuses the same
  // shape on a variant for the identical reason; this is its field-level
  // twin.
  if (/#\[serde\([^)]*\brename\s*=/.test(body)) {
    throw new Error(
      `${structName} now carries an explicit #[serde(rename = "…")] on a field. This mirror ` +
      'derives wire names with serde\'s CamelCase rule alone and cannot express a rename — teach '
      + 'camelOfSnake about it, or pin that field\'s wire name in the caller, before trusting this '
      + 'test again.',
    );
  }

  const fields: string[] = [];
  let d = 0;
  let cur = '';
  for (const ch of body) {
    if (ch === '{' || ch === '(' || ch === '<') d++;
    if (ch === '}' || ch === ')' || ch === '>') d--;
    if (ch === ',' && d === 0) {
      if (cur.trim()) fields.push(cur.trim());
      cur = '';
    } else {
      cur += ch;
    }
  }
  if (cur.trim()) fields.push(cur.trim());

  return fields.map((f) => {
    const name = /^pub\s+([A-Za-z0-9_]+)\s*:/.exec(f.trim());
    if (!name) throw new Error(`could not parse a "pub <name>: <Type>" field out of: ${f}`);
    return name[1];
  });
}

// snake_case → camelCase the way serde's own `RenameRule::CamelCase` does it
// for a STRUCT FIELD (`serde_derive::internals::case`) — every `_x` becomes an
// uppercase `X`, the underscore dropped. Distinct from `camelOf` above: an
// enum variant is already PascalCase in Rust and this rule only lowercases
// its first letter, but a struct field is snake_case to begin with, so the
// same wire convention is reached by a different transform.
export const camelOfSnake = (snake: string): string =>
  snake.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());

/// The FIELD NAMES of one struct variant of a Rust enum, in wire spelling.
///
/// 🔴 **`rustStructFields` above stops exactly where the `rename_all_fields`
/// hazard lives, and this is the half that closes it.** The four wire enums in
/// `scan_state.rs` carry `#[serde(tag = "kind", rename_all = "camelCase",
/// rename_all_fields = "camelCase")]`, and `rename_all` ALONE renames the
/// variants while leaving every struct-variant field in snake_case — which
/// compiles, serialises, and reaches the window as `root_index` beside a
/// correctly spelled `kind`. That is the trap `ipc.ts`'s own header says the
/// Rust side caught once already, and until this function existed the TS mirror
/// pinned the four plain structs and none of the variants: a Rust rename of
/// `Phase::Reading`'s fields failed `every_snapshot_has_its_wire_shape_pinned`
/// on that side and nothing at all on this one.
///
/// Two checks, not one, because mirroring the names cannot see the attribute
/// that decides how they are spelled:
///
///  1. the enum must still declare `rename_all_fields = "camelCase"` — remove
///     it and every field below goes to the wire in snake_case while this
///     mirror, which derives camelCase from the Rust names either way, goes on
///     agreeing with itself;
///  2. no field may carry an explicit `#[serde(rename = "…")]`, the same
///     refusal [`rustStructFields`] makes and for the same reason: this reader
///     has no way to express one, so it would compare two lists that both look
///     complete.
///
/// Variant fields are not `pub` (a variant's fields never are), which is the
/// one place this parse differs from the struct reader's.
export function rustVariantFields(
  rawSource: string,
  enumName: string,
  variantName: string,
): string[] {
  const source = rawSource.split('\n').map((line) => line.replace(/\/\/.*$/, '')).join('\n');
  const at = new RegExp(`pub enum ${enumName}\\s*\\{`).exec(source);
  if (!at) {
    throw new Error(`enum ${enumName} not found in the Rust source — has it moved or been renamed?`);
  }
  // 🔴 **The enum's OWN `#[serde(…)]`, found by walking back to the nearest one
  // and then proving nothing separates it from the declaration.** A fixed-width
  // window backwards does not work here and was measured not to: `///` doc
  // comments are stripped by the line above, so a few hundred characters before
  // `pub enum Phase` reach into `ScanSnapshot`'s attribute block — which still
  // carries `rename_all_fields`, so deleting `Phase`'s own left this check
  // green. What separates two items is a `}`; attributes and whitespace are all
  // that may sit between an attribute and the declaration it belongs to.
  const before = source.slice(0, at.index);
  const opener = before.lastIndexOf('#[serde(');
  const attributes = opener === -1 ? '' : before.slice(opener);
  if (opener === -1 || /[{}]/.test(attributes.slice(attributes.indexOf(')]') + 2))) {
    throw new Error(
      `${enumName} has no #[serde(…)] attribute of its own — this mirror assumes serde's `
      + 'CamelCase rule, and an enum that no longer declares it is not shaped the way this '
      + 'reader thinks.',
    );
  }
  if (!/rename_all_fields\s*=\s*"camelCase"/.test(attributes.slice(0, attributes.indexOf(')]')))) {
    throw new Error(
      `${enumName} no longer declares rename_all_fields = "camelCase". Its struct-variant fields `
      + 'now reach the window in snake_case, and this mirror derives camelCase from the Rust '
      + 'names either way — so it would go on agreeing with itself while the wire changed shape.',
    );
  }

  const body = braced(source, at.index + at[0].length, `${enumName}`);
  const variant = new RegExp(`(^|[\\s,])${variantName}\\s*\\{`).exec(body);
  if (!variant) {
    throw new Error(
      `${enumName}::${variantName} is not a struct variant in the Rust source — it was renamed, `
      + 'removed, or turned into a unit or tuple variant.',
    );
  }
  const fields = braced(body, variant.index + variant[0].length, `${enumName}::${variantName}`);
  if (/#\[serde\([^)]*\brename\s*=/.test(fields)) {
    throw new Error(
      `${enumName}::${variantName} now carries an explicit #[serde(rename = "…")] on a field. `
      + 'This mirror derives wire names with serde\'s CamelCase rule alone and cannot express a '
      + 'rename — pin that field\'s wire name in the caller before trusting this test again.',
    );
  }

  return splitFields(fields).map((f) => {
    const name = /^([A-Za-z0-9_]+)\s*:/.exec(f.trim());
    if (!name) throw new Error(`could not parse a "<name>: <Type>" field out of: ${f}`);
    return name[1];
  });
}

/// The text between the brace at `from - 1` and the one that closes it.
function braced(source: string, from: number, what: string): string {
  let depth = 1;
  let i = from;
  while (depth > 0) {
    if (source[i] === '{') depth++;
    else if (source[i] === '}') depth--;
    i++;
    if (i > source.length) {
      throw new Error(`ran off the end of the file looking for the closing brace of ${what}`);
    }
  }
  return source.slice(from, i - 1);
}

/// A brace/paren/angle-aware split on top-level commas — a field list's own
/// separator, which a `String` in a `HashMap<K, V>` would otherwise break.
function splitFields(body: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let cur = '';
  for (const ch of body) {
    if (ch === '{' || ch === '(' || ch === '<') depth++;
    if (ch === '}' || ch === ')' || ch === '>') depth--;
    if (ch === ',' && depth === 0) {
      if (cur.trim()) out.push(cur.trim());
      cur = '';
    } else {
      cur += ch;
    }
  }
  if (cur.trim()) out.push(cur.trim());
  return out;
}

/// The value of a `pub const <name>: &str = "…";` in a Rust source.
///
/// The third shape this module reads, after enum variants and struct fields,
/// and it exists for a name that is not a TYPE at all: an EVENT name, which
/// crosses the boundary as a bare string on both sides with no compiler between
/// them. Rename one half and both halves still build.
///
/// Deliberately anchored on `pub const` and on the `&str` type: a `const` that
/// is not public is not something the other side could be pinning against, and
/// one whose type changed is one this reader would be quoting out of context.
export function rustStrConst(rawSource: string, constName: string): string {
  const source = rawSource.split('\n').map((line) => line.replace(/\/\/.*$/, '')).join('\n');
  const m = new RegExp(`pub const ${constName}\\s*:\\s*&str\\s*=\\s*"([^"]*)"\\s*;`).exec(source);
  if (!m) {
    throw new Error(
      `pub const ${constName}: &str = "…"; not found in the Rust source — it was renamed, `
      + 'removed, made private, or given another type, and this mirror is now pinning nothing.',
    );
  }
  return m[1];
}
