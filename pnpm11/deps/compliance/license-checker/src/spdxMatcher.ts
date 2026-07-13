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
  // branch counts only when it is itself trustworthy. Disallow is independent
  // of the allowed list.
  if (opts.disallowed && opts.disallowed.size > 0 && isBlockedByDisallowed(tree, opts, false)) {
    return { allowed: false, reason: 'explicitly-disallowed' }
  }

  if (hasAllowedList) {
    const allowedList = [...opts.allowed!]
    // `satisfies` throws on any approved entry that is not valid SPDX (e.g.
    // "Apache 2.0" or a lowercased id — states `pnpm licenses allow` can
    // create and documents). Split the list: run the SPDX-parseable entries
    // through `satisfies`, and match the rest literally (case-insensitively)
    // against the license's own leaf candidates.
    const spdxAllowed = allowedList.filter(isParseableSpdx)
    const literalAllowed = allowedList.filter((a) => !isParseableSpdx(a))
    let allowedMatch = false
    // `satisfies` throws if ANY element of the array it is given is a
    // compound (AND/OR) expression, so a single bad entry (e.g. a
    // hand-edited "MIT AND Apache-2.0" in pnpm-workspace.yaml — `pnpm
    // licenses allow/disallow` reject these at input, but hand edits bypass
    // that) must not poison every other entry in the list. Drop compound
    // entries rather than evaluating one-by-one: `satisfies` treats the
    // array itself as an OR, and evaluating an AND-license expression (e.g.
    // "MIT AND BSD-3-Clause") needs to see multiple simple entries together
    // in one call to be satisfied — splitting the call per entry would break
    // that.
    const batchAllowed = spdxAllowed.filter((a) => !isCompoundLicenseExpression(a))
    if (batchAllowed.length > 0) {
      try {
        allowedMatch = satisfies(license, batchAllowed)
      } catch {
        allowedMatch = false
      }
    }
    if (!allowedMatch && literalAllowed.length > 0) {
      const candidates = collectLeafCandidates(tree)
      allowedMatch = literalAllowed.some((lit) => candidates.some((c) => c.toLowerCase() === lit.toLowerCase()))
    }
    if (allowedMatch) {
      return { allowed: true, reason: 'explicitly-allowed' }
    }
    if (opts.mode === 'strict') {
      return { allowed: false, reason: 'not-in-allowed-list' }
    }
    // loose + not in allowed list ⇒ non-blocking (allowed:true); no warning is
    // emitted.
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

// True iff `s` parses as a valid SPDX expression AND that expression is a
// compound (AND/OR) of two or more license terms. Simple ids ("MIT"), `+`
// ranges ("GPL-2.0+"), `WITH` exceptions ("Apache-2.0 WITH LLVM-exception"),
// and unparseable/literal strings all return false — those are the forms
// `spdx-satisfies` accepts as approved-license array entries. Compound
// expressions are rejected because flattening them (e.g. storing "MIT AND
// Apache-2.0" and matching either half individually) silently changes their
// meaning, and passing one through unflattened to `spdx-satisfies` throws.
export function isCompoundLicenseExpression (s: string): boolean {
  let tree: SpdxNode
  try {
    tree = spdxParse(s)
  } catch {
    return false
  }
  return containsConjunction(tree)
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
// license expression requires a disallowed license. `asEscape` tracks whether
// this node is being evaluated as one branch of an OR that has a disallowed
// sibling — an OR branch can only rescue the package if it is a trustworthy
// escape (see leafIsBlocked). Disallow is independent of the allowed list.
function isBlockedByDisallowed (node: SpdxNode, opts: MatchPolicyOptions, asEscape: boolean): boolean {
  if ('conjunction' in node) {
    return node.conjunction === 'or'
      // Either branch can be chosen: blocked only if BOTH branches are blocked
      // as escapes (a branch that isn't a trustworthy escape can't rescue an OR).
      ? isBlockedByDisallowed(node.left, opts, true) && isBlockedByDisallowed(node.right, opts, true)
      // Both branches must be accepted: blocked if EITHER is blocked.
      : isBlockedByDisallowed(node.left, opts, asEscape) || isBlockedByDisallowed(node.right, opts, asEscape)
  }
  return leafIsBlocked(node, opts, asEscape)
}

function leafIsBlocked (node: spdxParse.SpdxLicense, opts: MatchPolicyOptions, asEscape: boolean): boolean {
  const candidates = leafCandidates(node)
  if (candidates.some((c) => setHasCaseInsensitive(opts.disallowed!, c))) {
    return true
  }
  // Not disallowed. A standalone (non-escape) leaf never forces a disallowed
  // license. As an OR escape branch it is a valid escape UNLESS it is an opaque
  // LicenseRef whose contents we cannot verify (that would launder a disallowed
  // sibling). Note: this is independent of the allowed list — allow is a
  // separate check.
  return asEscape && node.license.startsWith('LicenseRef-')
}

function leafCandidates (node: spdxParse.SpdxLicense): string[] {
  const candidates: string[] = []
  if (node.exception) candidates.push(`${node.license} WITH ${node.exception}`)
  if (node.plus) candidates.push(`${node.license}+`)
  candidates.push(node.license)
  return candidates
}

// Walks the tree and returns every leaf's candidate forms (WITH/plus/base
// variants), used to match a license against non-SPDX (literal) allowed entries.
function collectLeafCandidates (node: SpdxNode): string[] {
  if ('conjunction' in node) {
    return [...collectLeafCandidates(node.left), ...collectLeafCandidates(node.right)]
  }
  return leafCandidates(node)
}

function isParseableSpdx (s: string): boolean {
  try {
    spdxParse(s)
    return true
  } catch {
    return false
  }
}

// Checks for a `conjunction` (AND/OR) node anywhere in the tree. A leaf
// (SpdxLicense) node has no children, so if the root itself is not a
// conjunction, none of its descendants can be one either — the root check
// alone covers "at any level".
function containsConjunction (node: SpdxNode): boolean {
  return 'conjunction' in node
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
