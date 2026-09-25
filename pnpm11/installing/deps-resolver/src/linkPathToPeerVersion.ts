// Converts a link: path into a stable, filename-safe token used as the
// peer's "version" inside peer-suffix hashes. The output must stay stable
// across pnpm versions so that lockfiles don't churn; it replicates what
// filenamify v4 produced for these paths in pnpm <= 10.
//
// Note: this encoding is lossy and can collide. Any leading run of `.`
// characters is dropped, and `/`, `\`, and literal `+` all collapse into
// a single `+`. For example, `packages/b`, `./packages/b`, and
// `../packages/b` all produce `packages+b`, and `.hidden/pkg` produces
// `hidden+pkg`. The only way to make this collision-free is to hash the
// normalized link target (or switch to a lossless escape encoding),
// either of which would change every link-path peer suffix in existing
// lockfiles. We accept the (extremely rare in practice) collision for
// lockfile stability; see https://github.com/pnpm/pnpm/issues/11272.
export function linkPathToPeerVersion (relPath: string): string {
  // Drop leading dots: v4 replaced `^\.+` with '+' and then stripOuter removed it.
  let i = 0
  while (i < relPath.length && relPath[i] === '.') i++

  let out = ''
  let lastWasPlus = true // pretend we just emitted '+' so leading '+' chars are suppressed
  for (; i < relPath.length; i++) {
    const c = relPath.charCodeAt(i)
    // Reserved filename chars, C0 controls, and literal '+' all collapse into a single '+'.
    const replace = c < 32 ||
      c === 34 /* " */ || c === 42 /* * */ || c === 43 /* + */ ||
      c === 47 /* / */ || c === 58 /* : */ || c === 60 /* < */ ||
      c === 62 /* > */ || c === 63 /* ? */ || c === 92 /* \ */ ||
      c === 124 /* | */
    if (replace) {
      if (!lastWasPlus) {
        out += '+'
        lastWasPlus = true
      }
    } else {
      out += relPath[i]
      lastWasPlus = false
    }
  }

  // Trim trailing '+' and '.' (v4 stripped trailing periods and outer replacement).
  let end = out.length
  while (end > 0) {
    const ch = out.charCodeAt(end - 1)
    if (ch !== 43 /* + */ && ch !== 46 /* . */) break
    end--
  }
  if (end > 0) return out.slice(0, end)
  // Empty result with something consumed collapses to a single '+'.
  return relPath.length === 0 ? '' : '+'
}
