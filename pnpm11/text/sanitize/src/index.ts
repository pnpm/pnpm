/**
 * The control and formatting characters {@link sanitizeInline} strips: C0/C1
 * controls, soft hyphens, bidi overrides and isolates, zero-width joiners,
 * and the other invisible formatting characters that can make a rendered
 * line read as something else.
 *
 * Variation selectors are deliberately absent: they change how the character
 * before them is drawn rather than where the rest of the line goes, and they
 * are part of the emoji a package description may legitimately carry.
 */
// eslint-disable-next-line no-control-regex
const CONTROL_AND_FORMAT_CHARACTERS = /[\u0000-\u001F\u007F-\u009F\u00AD\u0600-\u0605\u061C\u06DD\u070F\u0890\u0891\u08E2\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u206F\uFEFF\uFFF9-\uFFFB\u{110BD}\u{110CD}\u{13430}-\u{1343F}\u{1BCA0}-\u{1BCA3}\u{1D173}-\u{1D17A}\u{E0001}\u{E0020}-\u{E007F}]/gu

/**
 * Strips control and formatting characters from text embedded in a
 * single-line field that reaches the terminal.
 *
 * Registry- and store-derived strings (package names, versions, tags,
 * usernames) can carry escape sequences, bidi overrides, or zero-width
 * joiners that make rendered output misrepresent what is installed or
 * published, so every such string is filtered before it is printed.
 *
 * The TypeScript counterpart of the Rust `pnpm-text-sanitize` crate's
 * `sanitize_inline`; the two strip the same characters.
 */
export function sanitizeInline (text: string): string {
  return text.replace(CONTROL_AND_FORMAT_CHARACTERS, '')
}
