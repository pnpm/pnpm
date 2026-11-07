import { redactAndSanitize } from '@pnpm/error'

/**
 * Whether the authority of `url` carries a `user:pass@` prefix. The authority
 * ends at the first `/`, `?`, or `#`, so a later `@` in the path is not one.
 *
 * Both the full form and the scheme-less `//host/` form count. The latter is
 * the shape `.npmrc` scopes settings with, so it is the one a user is most
 * likely to reach for here.
 */
export function registryUrlHasUserinfo (url: string): boolean {
  return userinfoEnd(url) !== undefined
}

/**
 * The offset just past the `user:pass@` of `url`, or `undefined` when its
 * authority carries none. Splitting it out keeps the detection and the
 * redaction below agreeing on what the authority is.
 */
function userinfoEnd (url: string): number | undefined {
  const authorityStart = authorityStartOf(url)
  if (authorityStart === undefined) return undefined
  const authority = url.slice(authorityStart)
  const authorityEnd = authority.search(/[/?#]/)
  const at = (authorityEnd === -1 ? authority : authority.slice(0, authorityEnd)).lastIndexOf('@')
  return at === -1 ? undefined : authorityStart + at + 1
}

/**
 * Where the authority of `url` begins, or `undefined` if it has none.
 *
 * The scheme is anchored at the start rather than found by searching for the
 * first `://`: a `://` inside the path (`//host/a://b`) would otherwise be
 * taken for the separator, and the real authority — credentials and all —
 * would go unexamined.
 */
function authorityStartOf (url: string): number | undefined {
  const schemeEnd = url.indexOf('://')
  if (schemeEnd !== -1 && SCHEME.test(url.slice(0, schemeEnd))) return schemeEnd + '://'.length
  if (url.startsWith('//')) return '//'.length
  return undefined
}

const SCHEME = /^[a-z][a-z0-9+.-]*$/i

/**
 * `url` with any `user:pass@` removed, safe to put in a message.
 *
 * {@link redactAndSanitize} only recognizes an authority after a `://`, and
 * deliberately so: it runs over arbitrary prose, where a bare `//` is more
 * often a comment or a path than a URL. Here the string is known to be a
 * registry URL or a URL-scoped `.npmrc` key, so the scheme-less `//host/`
 * form can be handled too.
 */
export function redactRegistryUrl (url: string): string {
  const authorityStart = authorityStartOf(url)
  const end = userinfoEnd(url)
  if (authorityStart === undefined || end === undefined) return redactAndSanitize(url)
  return redactAndSanitize(`${url.slice(0, authorityStart)}${url.slice(end)}`)
}
