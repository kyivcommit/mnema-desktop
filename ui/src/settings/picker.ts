/** Every whitespace-separated word of `query` must be a substring of `haystack`, ignoring case. */
export function matches(query: string, haystack: string): boolean {
  const h = haystack.toLowerCase();
  return query.toLowerCase().split(/\s+/).filter(Boolean).every((w) => h.includes(w));
}
