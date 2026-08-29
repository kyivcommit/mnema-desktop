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
// see its own blind spot.** It reads variant NAMES and a caller applies
// serde's `RenameRule::CamelCase` to them; it never reads the enum's
// `#[serde(rename_all = …)]`, so switching that attribute to `snake_case`
// leaves every caller green while the wire spelling changes underneath. The
// other half is a Rust test that serializes each variant and pins the string —
// `job.rs`'s `every_end_reason_has_its_camel_case_spelling_pinned` and
// `tree.rs`'s `the_subfolder_wire_shape_is_camel_case` are the two that exist.
// Neither half closes the gap alone: the pair does.

// The variant names of `pub enum <enumName>` in `rawSource`, in source order
// and in Rust's own spelling — `camelOf` turns one into its wire name.
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
  const m = new RegExp(`pub enum ${enumName}\\s*\\{`).exec(source);
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
