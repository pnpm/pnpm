import { PnpmError } from '@pnpm/error'
import { jsrToNpmPackageName } from './string'

export interface JsrSpec {
  jsrPkgName?: string
  npmPkgName?: string
  pref?: string
}

export function parseJsrSpecifier (rawSpecifier: string): { jsrPkgName?: string, npmPkgName?: string, pref?: string } | null {
  if (!rawSpecifier.startsWith('jsr:')) return null

  rawSpecifier = rawSpecifier.substring('jsr:'.length)

  // syntax: jsr:@<scope>/<name>[@<spec>]
  if (rawSpecifier.startsWith('@')) {
    const index = rawSpecifier.lastIndexOf('@')

    // syntax: jsr:@<scope>/<name>
    if (index === 0) {
      return {
        jsrPkgName: rawSpecifier,
        npmPkgName: jsrToNpmPackageName(rawSpecifier),
      }
    }

    // syntax: jsr:@<scope>/<name>@<spec>
    const jsrPkgName = rawSpecifier.substring(0, index)
    return {
      jsrPkgName,
      npmPkgName: jsrToNpmPackageName(jsrPkgName),
      pref: rawSpecifier.substring(index + '@'.length),
    }
  }

  // syntax: jsr:<name>@<spec> (invalid)
  if (rawSpecifier.includes('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have a scope')
  }

  // syntax: jsr:<spec>
  return { pref: rawSpecifier }
}
