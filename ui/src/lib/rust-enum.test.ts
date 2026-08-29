import { expect, test } from 'vitest';
import { camelOf, rustEnumVariants } from './rust-enum';

// The case the reader exists for, and the one it was once wrong about: a
// variant hidden behind a doc comment that carries a lone `}`. Before comments
// were stripped first, the brace walk stopped inside the comment, the body was
// truncated, and the callers reported green having never seen `Second`.
test('the reader sees a variant hidden behind a doc comment carrying a lone brace', () => {
  const fixture = `
/// A doc comment with a lone } in it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Sample {
    First,
    /// A brace } here truncated the body before comments were stripped first.
    Second,
}
`;
  expect(rustEnumVariants(fixture, 'Sample')).toEqual(['First', 'Second']);
});

// A struct variant's own braces are depth, not a terminator, and its fields are
// not variants. Both halves stated: the name survives, the field does not.
test('a struct variant contributes its name and none of its fields', () => {
  const fixture = `
pub enum Sample {
    Plain,
    Held { prefix: String, depth: usize },
    Last,
}
`;
  expect(rustEnumVariants(fixture, 'Sample')).toEqual(['Plain', 'Held', 'Last']);
});

// A trailing comma is optional in Rust, and a reader that needed one would drop
// the last variant — the direction nothing else here would notice.
test('the final variant is read with or without a trailing comma', () => {
  const withComma = 'pub enum Sample {\n    First,\n    Second,\n}\n';
  const without = 'pub enum Sample {\n    First,\n    Second\n}\n';
  expect(rustEnumVariants(withComma, 'Sample')).toEqual(['First', 'Second']);
  expect(rustEnumVariants(without, 'Sample')).toEqual(['First', 'Second']);
});

// The reader must not answer about a neighbour. `SampleTwo` is matched by a
// prefix search for `Sample`, and an answer drawn from it would be a green pin
// against the wrong enum.
test('an enum it cannot find is a throw, not an answer drawn from a neighbour', () => {
  const fixture = 'pub enum SampleTwo {\n    Only,\n}\n';
  expect(() => rustEnumVariants(fixture, 'Sample')).toThrow(/enum Sample not found/);
  expect(rustEnumVariants(fixture, 'SampleTwo')).toEqual(['Only']);
});

// The blind spot it CAN see: a variant-level rename it has no way to express.
// `rename_all` on the enum is the rule this reader already assumes, and must
// not be mistaken for one — both directions, because a guard that fired on
// `rename_all` would make every camelCase enum in the repository unreadable.
test('a variant-level serde rename is refused, and the enum-level rule is not', () => {
  const renamed = `
pub enum Sample {
    First,
    #[serde(rename = "second_thing")]
    Second,
}
`;
  expect(() => rustEnumVariants(renamed, 'Sample')).toThrow(/explicit #\[serde\(rename/);

  const renameAll = '#[serde(rename_all = "camelCase")]\npub enum Sample {\n    FirstThing,\n}\n';
  expect(rustEnumVariants(renameAll, 'Sample')).toEqual(['FirstThing']);
});

// serde's `RenameRule::CamelCase` touches the first character and nothing else.
// The second assertion is the one that matters: a reader that lowercased more
// than the first letter would silently mirror `excludedbyancestor`.
test('camelOf lowercases the first character and leaves the rest as written', () => {
  expect(camelOf('Open')).toBe('open');
  expect(camelOf('ExcludedByAncestor')).toBe('excludedByAncestor');
});
