import { expect, test } from 'vitest';
import {
  camelOf, camelOfSnake, rustEnumVariants, rustStructFields,
} from './rust-enum';

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

// Task 11a. `rustStructFields`'s own version of the same case
// `rustEnumVariants` is pinned against above: a doc comment carrying a lone
// `}` must not truncate the field list, and a doc comment carrying NEITHER
// brace (the ordinary case, and the one every real struct in this codebase
// takes) must not confuse it either.
test('the field reader sees a field hidden behind a doc comment carrying a lone brace', () => {
  const fixture = `
/// A doc comment with a lone } in it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub first: String,
    /// A brace } here truncated the body before comments were stripped first.
    pub second: u64,
}
`;
  expect(rustStructFields(fixture, 'Sample')).toEqual(['first', 'second']);
});

// A field typed with a generic carries its own `<>`, and neither its own
// comma (a two-parameter generic) nor its own bracket may be mistaken for the
// struct's own — both directions, since either mistake drops or splits a
// field wrongly.
test('a generic field type does not confuse the struct\'s own closing brace or its own field separators', () => {
  const fixture = `
pub struct Sample {
    pub plain: String,
    pub nested: Option<Vec<crate::job::Frozen>>,
    pub last: bool,
}
`;
  expect(rustStructFields(fixture, 'Sample')).toEqual(['plain', 'nested', 'last']);
});

// The final field is read with or without a trailing comma, the same pair
// `rustEnumVariants` is pinned against for a variant.
test('the final field is read with or without a trailing comma', () => {
  const withComma = 'pub struct Sample {\n    pub first: u64,\n    pub second: bool,\n}\n';
  const without = 'pub struct Sample {\n    pub first: u64,\n    pub second: bool\n}\n';
  expect(rustStructFields(withComma, 'Sample')).toEqual(['first', 'second']);
  expect(rustStructFields(without, 'Sample')).toEqual(['first', 'second']);
});

// The reader must not answer about a neighbour, the same guarantee
// `rustEnumVariants` gives for an enum it cannot find.
test('a struct it cannot find is a throw, not an answer drawn from a neighbour', () => {
  const fixture = 'pub struct SampleTwo {\n    pub only: bool,\n}\n';
  expect(() => rustStructFields(fixture, 'Sample')).toThrow(/struct Sample not found/);
  expect(rustStructFields(fixture, 'SampleTwo')).toEqual(['only']);
});

// serde's `RenameRule::CamelCase` applied to a snake_case struct field: every
// `_x` becomes `X`, the underscore dropped, and a field with no underscore at
// all is left exactly as written.
test('camelOfSnake turns snake_case into camelCase and leaves a single word alone', () => {
  expect(camelOfSnake('root_path')).toBe('rootPath');
  expect(camelOfSnake('roots_read')).toBe('rootsRead');
  expect(camelOfSnake('reason')).toBe('reason');
});

// Task 11a fix round 1 (Minor 1). The field-level twin of the enum reader's
// own refusal: without this, a `#[serde(rename = "…")]` on a field reaches
// `camelOfSnake` unnoticed, and the pin compares two lists that both look
// complete while quietly holding the wrong name for one of them — a false
// GREEN this throw turns into a loud failure instead.
test('a field-level serde rename is refused, the way a variant-level one is', () => {
  const renamed = `
pub struct Sample {
    pub first: String,
    #[serde(rename = "second_thing")]
    pub second: bool,
}
`;
  expect(() => rustStructFields(renamed, 'Sample')).toThrow(/explicit #\[serde\(rename/);
});
