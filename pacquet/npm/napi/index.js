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

function platformTriple() {
  const { platform, arch } = process
  if (platform === 'linux') {
    const isMusl = (() => {
      try {
        const report = process.report?.getReport()
        return !report?.header?.glibcVersionRuntime
      } catch {
        return false
      }
    })()
    return `linux-${arch}${isMusl ? '-musl' : ''}`
  }
  return `${platform}-${arch}`
}

function tryLoad(candidate) {
  if (!candidate) return null
  try {
    if (candidate.endsWith('.node') && !fs.existsSync(candidate)) return null
    return require(candidate)
  } catch {
    return null
  }
}

function loadBinding() {
  const triple = platformTriple()
  const binding =
    tryLoad(process.env.PNPM_NAPI_BINARY) ??
    tryLoad(`@pnpm/napi.${triple}`) ??
    tryLoad(path.join(__dirname, `pnpm-napi.${triple}.node`)) ??
    tryLoad(path.join(__dirname, 'pnpm-napi.node'))
  if (!binding) {
    throw new Error(
      `Failed to load the pnpm Rust engine for ${triple}. ` +
        'Install the matching @pnpm/napi platform package, or point ' +
        'PNPM_NAPI_BINARY at a locally built .node file.'
    )
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
