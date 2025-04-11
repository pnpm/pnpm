import { PnpmError } from '@pnpm/error'
import { type JsrSpec, type JsrSpecWithAlias, type ParsedJsrPackageName } from './types'

export function parseJsrPref (pref: string): JsrSpec | null {
  if (!pref.startsWith('jsr:')) return null

  pref = pref.slice('jsr:'.length)

  // syntax: jsr:@<scope>/<name>[@<spec>]
  if (pref.startsWith('@')) {
    pref = pref.slice('@'.length)

    const index = pref.lastIndexOf('@')

    // syntax: jsr:@<scope>/<name>
    if (index === -1) {
      return scopeAndName(pref)
    }

    // syntax: jsr:@<scope>/<name>@<spec>
    const result: JsrSpecWithAlias = scopeAndName(pref.slice(0, index))
    result.pref = pref.slice(index + '@'.length)
    return result
  }

  // syntax: jsr:<name>@<spec> (invalid)
  if (pref.includes('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have a scope')
  }

  // syntax: jsr:<spec>
  return { pref: pref }
}

export function parseJsrPackageName (fullName: string): ParsedJsrPackageName {
  if (!fullName.startsWith('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have a scope')
  }

  fullName = fullName.slice('@'.length)
  return scopeAndName(fullName)
}

function scopeAndName (fullNameWithoutLeadingAt: string): ParsedJsrPackageName {
  const sepIndex = fullNameWithoutLeadingAt.indexOf('/')
  if (sepIndex === -1) {
    throw new PnpmError('INVALID_JSR_PACKAGE_NAME', `The package name '@${fullNameWithoutLeadingAt}' is invalid`)
  }

  const scope = fullNameWithoutLeadingAt.slice(0, sepIndex)
  const name = fullNameWithoutLeadingAt.slice(sepIndex + '/'.length)
  return { scope, name }
}
