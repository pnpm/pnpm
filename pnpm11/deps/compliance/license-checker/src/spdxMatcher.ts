import spdxParse from 'spdx-expression-parse'
import satisfies from 'spdx-satisfies'

export interface LicenseMatchResult {
  allowed: boolean
  reason: 'explicitly-allowed' | 'explicitly-disallowed' | 'not-in-allowed-list' | 'unknown-license' | 'allowed-by-default'
}

export interface MatchPolicyOptions {
  allowed?: Set<string>
  disallowed?: Set<string>
  mode: 'strict' | 'loose'
}

type SpdxNode = spdxParse.SpdxLicense | spdxParse.SpdxConjunction

export function matchLicenseAgainstPolicy (
  license: string,
  opts: MatchPolicyOptions
): LicenseMatchResult {
  const hasAllowedList = !!(opts.allowed && opts.allowed.size > 0)

  if (!license || license === 'Unknown') {
    return unknown(opts, hasAllowedList)
  }

  let tree: SpdxNode
  try {
    tree = spdxParse(license)
  } catch {
    // Not a valid SPDX expression. Do NOT fall back to literal matching — that
    // was case-sensitive and let mis-cased ids bypass the policy. Treat the
    // whole string as a single opaque id and evaluate it directly.
    return evaluateOpaqueId(license, opts, hasAllowedList)
  }

  // Disallow first: a package violates the disallow policy only if it cannot be
  // used WITHOUT a disallowed license (see isBlockedByDisallowed). An OR escape
  // branch counts only when it is itself known-acceptable.
  if (opts.disallowed && opts.disallowed.size > 0 && isBlockedByDisallowed(tree, opts)) {
    return { allowed: false, reason: 'explicitly-disallowed' }
  }

  if (hasAllowedList) {
    if (satisfies(license, [...opts.allowed!])) {
      return { allowed: true, reason: 'explicitly-allowed' }
    }
    if (opts.mode === 'strict') {
      return { allowed: false, reason: 'not-in-allowed-list' }
    }
    // loose + not in allowed list ⇒ warning-level (allowed-by-default so
    // checkLicenses downgrades it to a warning)
    return { allowed: true, reason: 'not-in-allowed-list' }
  }

  return { allowed: true, reason: 'allowed-by-default' }
}

export function extractLicenseIds (license: string): string[] {
  try {
    return collectIds(spdxParse(license))
  } catch {
    return []
  }
}

// -- helpers --

function unknown (opts: MatchPolicyOptions, hasAllowedList: boolean): LicenseMatchResult {
  return opts.mode === 'strict' && hasAllowedList
    ? { allowed: false, reason: 'unknown-license' }
    : { allowed: true, reason: 'allowed-by-default' }
}

// A single non-SPDX token: match case-insensitively against the policy sets.
function evaluateOpaqueId (id: string, opts: MatchPolicyOptions, hasAllowedList: boolean): LicenseMatchResult {
  if (opts.disallowed && setHasCaseInsensitive(opts.disallowed, id)) {
    return { allowed: false, reason: 'explicitly-disallowed' }
  }
  if (hasAllowedList) {
    if (setHasCaseInsensitive(opts.allowed!, id)) {
      return { allowed: true, reason: 'explicitly-allowed' }
    }
    return opts.mode === 'strict'
      ? { allowed: false, reason: 'not-in-allowed-list' }
      : { allowed: true, reason: 'not-in-allowed-list' }
  }
  return unknown(opts, hasAllowedList)
}

// A package is blocked by the disallow policy iff EVERY way to satisfy its
// license expression requires a disallowed license. For an OR, the package is
// blocked only if BOTH branches are blocked; an escape branch is valid only if
// it is known-acceptable (satisfies the allowed list, or — with no allowed list
// — is a recognised leaf that is not itself disallowed). This prevents an
// opaque LicenseRef from laundering a disallowed license.
function isBlockedByDisallowed (node: SpdxNode, opts: MatchPolicyOptions): boolean {
  if ('conjunction' in node) {
    return node.conjunction === 'or'
      ? isBlockedByDisallowed(node.left, opts) && isBlockedByDisallowed(node.right, opts)
      : isBlockedByDisallowed(node.left, opts) || isBlockedByDisallowed(node.right, opts)
  }
  return leafIsBlocked(node, opts)
}

function leafIsBlocked (node: spdxParse.SpdxLicense, opts: MatchPolicyOptions): boolean {
  const candidates = leafCandidates(node)
  if (candidates.some((c) => setHasCaseInsensitive(opts.disallowed!, c))) {
    return true
  }
  // Not itself disallowed. As an OR escape it is only "safe" when known-
  // acceptable: in the allowed list, or (no allowed list) a recognised id.
  const hasAllowedList = !!(opts.allowed && opts.allowed.size > 0)
  if (hasAllowedList) {
    return !candidates.some((c) => setHasCaseInsensitive(opts.allowed!, c))
  }
  // No allowed list: a recognised SPDX id is a valid escape; an opaque
  // LicenseRef (license starts with "LicenseRef-") is NOT.
  return node.license.startsWith('LicenseRef-')
}

function leafCandidates (node: spdxParse.SpdxLicense): string[] {
  const candidates: string[] = []
  if (node.exception) candidates.push(`${node.license} WITH ${node.exception}`)
  if (node.plus) candidates.push(`${node.license}+`)
  candidates.push(node.license)
  return candidates
}

function setHasCaseInsensitive (set: Set<string>, value: string): boolean {
  const lower = value.toLowerCase()
  for (const entry of set) {
    if (entry.toLowerCase() === lower) return true
  }
  return false
}

function collectIds (node: SpdxNode): string[] {
  if ('conjunction' in node) {
    return [...collectIds(node.left), ...collectIds(node.right)]
  }
  return [node.license]
}
