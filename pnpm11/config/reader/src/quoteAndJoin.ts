/** Renders values for a message that lists the settings it objected to. */
export function quoteAndJoin (values: string[]): string {
  return values.map((value) => `"${value}"`).join(', ')
}
