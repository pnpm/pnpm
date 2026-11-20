/**
 * Compares two strings by code unit, so that the sort order is the same on
 * every machine. `String.prototype.localeCompare` must not be used for
 * anything that ends up in a lockfile or any other file we compare across
 * machines, as its result depends on the current locale.
 */
export function lexCompare (left: string, right: string): number {
  return left > right ? 1 : left < right ? -1 : 0
}
