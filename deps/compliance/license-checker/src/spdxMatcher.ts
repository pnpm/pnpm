import spdxParse from 'spdx-expression-parse'

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
  if (!license || license === 'Unknown') {
    return opts.mode === 'strict'
      ? { allowed: false, reason: 'unknown-license' }
      : { allowed: true, reason: 'allowed-by-default' }
  }

  let tree: SpdxNode
  try {
    tree = spdxParse(license)
  } catch {
    // Non-SPDX license string — check raw string against policy before giving up
    const rawResult = evaluateId(license, opts)
    if (rawResult.reason !== 'allowed-by-default') {
      return rawResult
    }
    return opts.mode === 'strict'
      ? { allowed: false, reason: 'unknown-license' }
      : { allowed: true, reason: 'allowed-by-default' }
  }

  return evaluateNode(tree, opts)
}

export function extractLicenseIds (license: string): string[] {
  try {
    const tree = spdxParse(license)
    return collectIds(tree)
  } catch {
    return []
  }
}

// -- Policy evaluation --

function evaluateNode (node: SpdxNode, opts: MatchPolicyOptions): LicenseMatchResult {
  if ('conjunction' in node) {
    return node.conjunction === 'or'
      ? evaluateOr(node.left, node.right, opts)
      : evaluateAnd(node.left, node.right, opts)
  }
  return evaluateLicenseNode(node, opts)
}

function evaluateLicenseNode (node: spdxParse.SpdxLicense, opts: MatchPolicyOptions): LicenseMatchResult {
  // Build the full literal form for WITH/plus variants so policy entries
  // written as exact strings (e.g. "Apache-2.0 WITH LLVM-exception" or
  // "GPL-2.0+") can match.
  const candidates: string[] = []
  if (node.exception) {
    candidates.push(`${node.license} WITH ${node.exception}`)
  }
  if (node.plus) {
    candidates.push(`${node.license}+`)
  }
  candidates.push(node.license)

  // Check disallowed — any candidate match is a disallow
  if (opts.disallowed && opts.disallowed.size > 0) {
    for (const id of candidates) {
      if (opts.disallowed.has(id)) {
        return { allowed: false, reason: 'explicitly-disallowed' }
      }
    }
  }

  // Check allowed — any candidate match is an allow.
  // When no allowed list is configured, the check is skipped entirely
  // (even in strict mode). This lets users use strict mode with only a
  // disallowed list to block specific licenses without enumerating every
  // permitted license.
  if (opts.allowed && opts.allowed.size > 0) {
    for (const id of candidates) {
      if (opts.allowed.has(id)) {
        return { allowed: true, reason: 'explicitly-allowed' }
      }
    }
    if (opts.mode === 'strict') {
      return { allowed: false, reason: 'not-in-allowed-list' }
    }
  }

  return { allowed: true, reason: 'allowed-by-default' }
}

function evaluateId (id: string, opts: MatchPolicyOptions): LicenseMatchResult {
  if (opts.disallowed && opts.disallowed.has(id)) {
    return { allowed: false, reason: 'explicitly-disallowed' }
  }
  if (opts.allowed && opts.allowed.size > 0) {
    if (opts.allowed.has(id)) {
      return { allowed: true, reason: 'explicitly-allowed' }
    }
    if (opts.mode === 'strict') {
      return { allowed: false, reason: 'not-in-allowed-list' }
    }
  }
  return { allowed: true, reason: 'allowed-by-default' }
}

function evaluateOr (left: SpdxNode, right: SpdxNode, opts: MatchPolicyOptions): LicenseMatchResult {
  const leftResult = evaluateNode(left, opts)
  const rightResult = evaluateNode(right, opts)

  // If either side is explicitly allowed, the OR passes
  if (leftResult.allowed && leftResult.reason === 'explicitly-allowed') return leftResult
  if (rightResult.allowed && rightResult.reason === 'explicitly-allowed') return rightResult

  // If either side passes (even by default), the OR passes
  if (leftResult.allowed) return leftResult
  if (rightResult.allowed) return rightResult

  // Both sides failed — prefer the more specific reason
  if (leftResult.reason === 'explicitly-disallowed' && rightResult.reason === 'explicitly-disallowed') {
    return { allowed: false, reason: 'explicitly-disallowed' }
  }
  return leftResult
}

function evaluateAnd (left: SpdxNode, right: SpdxNode, opts: MatchPolicyOptions): LicenseMatchResult {
  const leftResult = evaluateNode(left, opts)
  const rightResult = evaluateNode(right, opts)

  // Both sides must pass for AND
  if (!leftResult.allowed) return leftResult
  if (!rightResult.allowed) return rightResult

  // Both passed — prefer the more specific reason
  if (leftResult.reason === 'explicitly-allowed' || rightResult.reason === 'explicitly-allowed') {
    return { allowed: true, reason: 'explicitly-allowed' }
  }
  return leftResult
}

function collectIds (node: SpdxNode): string[] {
  if ('conjunction' in node) {
    return [...collectIds(node.left), ...collectIds(node.right)]
  }
  return [node.license]
}
