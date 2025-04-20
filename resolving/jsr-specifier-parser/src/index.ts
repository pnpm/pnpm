import { PnpmError } from '@pnpm/error'

export interface JsrSpec {
  jsrPkgName: string
  npmPkgName: string
  // A versionSelector may be a semver range (e.g. ^1.0.0), exact version (e.g. 2.3.4), or a dist-tag (e.g. "latest").
  versionSelector?: string
}

export function parseJsrSpecifier (rawSpecifier: string, alias?: string): JsrSpec | null {
  if (!rawSpecifier.startsWith('jsr:')) return null

  rawSpecifier = rawSpecifier.substring('jsr:'.length)

  // syntax: jsr:@<scope>/<name>[@<version_selector>]
  if (rawSpecifier.startsWith('@')) {
    const index = rawSpecifier.lastIndexOf('@')

    // syntax: jsr:@<scope>/<name>
    if (index === 0) {
      return {
        jsrPkgName: rawSpecifier,
        npmPkgName: jsrToNpmPackageName(rawSpecifier),
      }
    }

    // syntax: jsr:@<scope>/<name>@<version_selector>
    const jsrPkgName = rawSpecifier.substring(0, index)
    return {
      jsrPkgName,
      npmPkgName: jsrToNpmPackageName(jsrPkgName),
      versionSelector: rawSpecifier.substring(index + '@'.length),
    }
  }

  // syntax: jsr:<name>@<version_selector> (invalid)
  if (rawSpecifier.includes('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have a scope')
  }

  if (!alias) {
    throw new PnpmError('INVALID_JSR_SPECIFIER', `JSR specifier '${rawSpecifier}' is missing a package name`)
  }

  // syntax: jsr:<spec>
  return {
    versionSelector: rawSpecifier,
    jsrPkgName: alias,
    npmPkgName: jsrToNpmPackageName(alias),
  }
}

function jsrToNpmPackageName (jsrPkgName: string): string {
  if (!jsrPkgName.startsWith('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have a scope')
  }
  const sepIndex = jsrPkgName.indexOf('/')
  if (sepIndex === -1) {
    throw new PnpmError('INVALID_JSR_PACKAGE_NAME', `The package name '${jsrPkgName}' is invalid`)
  }
  const scope = jsrPkgName.substring(0, sepIndex)
  const name = jsrPkgName.substring(sepIndex + '/'.length)
  return `@jsr/${scope.substring(1)}__${name}`
}
