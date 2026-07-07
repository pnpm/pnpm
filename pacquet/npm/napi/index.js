'use strict'

// Loader for the pacquet-napi native addon.
//
// Resolution order:
// 1. PNPM_NAPI_BINARY env var — explicit path to a .node file (local dev,
//    custom builds).
// 2. A platform package (`@pnpm/napi.<platform>`) installed as an
//    optionalDependency (release-time; injected by the package generator like
//    the `@pnpm/exe.*` packages).
// 3. A locally built artifact next to this file or under the Cargo target dir
//    (development checkouts).

const path = require('node:path')
const fs = require('node:fs')

// `'glibc' | 'musl' | null` — `null` when the host isn't Linux or the libc
// can't be probed (`process.report` may be unavailable/disabled). glibc builds
// set `glibcVersionRuntime`; musl leaves it unset.
function detectLinuxLibc() {
  if (process.platform !== 'linux') return null
  try {
    return process.report?.getReport()?.header?.glibcVersionRuntime ? 'glibc' : 'musl'
  } catch {
    return null
  }
}

// Ordered platform triples to try. On Linux both libc variants are attempted
// (ordered by detection) so a musl host whose libc can't be probed still
// resolves the `-musl` addon instead of failing on the glibc one; elsewhere
// there is a single triple.
function platformTriples() {
  const { platform, arch } = process
  if (platform === 'linux') {
    const order = detectLinuxLibc() === 'musl' ? ['-musl', ''] : ['', '-musl']
    return order.map((suffix) => `linux-${arch}${suffix}`)
  }
  return [`${platform}-${arch}`]
}

function tryLoad(candidate, loadErrors) {
  if (!candidate) return null
  try {
    if (candidate.endsWith('.node') && !fs.existsSync(candidate)) return null
    return require(candidate)
  } catch (err) {
    if (isMissingCandidate(err, candidate)) {
      loadErrors.push(err)
      return null
    }
    throw err
  }
}

function isMissingCandidate(err, candidate) {
  return (
    err &&
    err.code === 'MODULE_NOT_FOUND' &&
    typeof err.message === 'string' &&
    err.message.includes(`'${candidate}'`)
  )
}

function loadFailure(triple, loadErrors) {
  const error = new Error(
    `Failed to load the pnpm Rust engine for ${triple}. ` +
      'Install the matching @pnpm/napi platform package, or point ' +
      'PNPM_NAPI_BINARY at a locally built .node file.'
  )
  if (loadErrors.length > 0) {
    error.cause = loadErrors[0]
  }
  return error
}

function loadBinding() {
  const triples = platformTriples()
  const loadErrors = []
  // Only ever hand a `.node` addon to require(). Without this, a non-.node
  // PNPM_NAPI_BINARY value would be require()'d as an arbitrary JS module.
  const explicit = process.env.PNPM_NAPI_BINARY
  if (explicit && !explicit.endsWith('.node')) {
    throw new Error(`PNPM_NAPI_BINARY must point to a .node addon file, got: ${explicit}`)
  }
  const candidates = [
    explicit,
    ...triples.flatMap((triple) => [
      `@pnpm/napi.${triple}`,
      path.join(__dirname, `pnpm-napi.${triple}.node`),
    ]),
    path.join(__dirname, 'pnpm-napi.node'),
  ]
  let binding = null
  for (const candidate of candidates) {
    binding = tryLoad(candidate, loadErrors)
    if (binding) break
  }
  if (!binding) {
    throw loadFailure(triples[0], loadErrors)
  }
  return binding
}

// The Rust side encodes structured error fields (code/hint) as a JSON envelope
// in the thrown error's message, prefixed with PNPM_ERR_JSON:. Lift them back
// onto the Error object so consumers keep reading `err.code` / `err.hint`.
const ENVELOPE_PREFIX = 'PNPM_ERR_JSON:'
function decorateError(err) {
  if (err && typeof err.message === 'string' && err.message.startsWith(ENVELOPE_PREFIX)) {
    try {
      const parsed = JSON.parse(err.message.slice(ENVELOPE_PREFIX.length))
      if (parsed.code != null) err.code = parsed.code
      if (parsed.hint != null) err.hint = parsed.hint
      if (typeof parsed.message === 'string') err.message = parsed.message
    } catch {
      // Leave the error untouched if the envelope doesn't parse.
    }
  }
  return err
}

// Wrap every exported function so both sync throws and rejected promises get
// the envelope lifted. Non-function exports (if any) pass through unchanged.
function wrapExports(binding) {
  const wrapped = {}
  for (const key of Object.keys(binding)) {
    const value = binding[key]
    if (typeof value !== 'function') {
      wrapped[key] = value
      continue
    }
    wrapped[key] = function (...args) {
      try {
        const result = value.apply(this, args)
        if (result && typeof result.then === 'function') {
          return result.then(
            (resolved) => resolved,
            (err) => {
              throw decorateError(err)
            }
          )
        }
        return result
      } catch (err) {
        throw decorateError(err)
      }
    }
  }
  return wrapped
}

module.exports = wrapExports(loadBinding())
