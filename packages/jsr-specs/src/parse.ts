import { PnpmError } from '@pnpm/error'
import { type JsrSpec, type JsrSpecWithAlias, type ParsedJsrPackageName } from './types'

export function parseJsrSpecifier (rawSpecifier: string): JsrSpec | null {
  if (!rawSpecifier.startsWith('jsr:')) return null

  rawSpecifier = rawSpecifier.slice('jsr:'.length)

  // syntax: jsr:@<scope>/<name>[@<spec>]
  if (rawSpecifier.startsWith('@')) {
    rawSpecifier = rawSpecifier.slice('@'.length)

    const index = rawSpecifier.lastIndexOf('@')

    // syntax: jsr:@<scope>/<name>
    if (index === -1) {
      return scopeAndName(rawSpecifier)
    }

    // syntax: jsr:@<scope>/<name>@<spec>
    const result: JsrSpecWithAlias = scopeAndName(rawSpecifier.slice(0, index))
    result.pref = rawSpecifier.slice(index + '@'.length)
    return result
  }

  // syntax: jsr:<name>@<spec> (invalid)
  if (rawSpecifier.includes('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have a scope')
  }

  // syntax: jsr:<spec>
  return { pref: rawSpecifier }
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
