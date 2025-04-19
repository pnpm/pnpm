import { PnpmError } from '@pnpm/error'

export function jsrToNpmPackageName (jsrPkgName: string): string {
  if (!jsrPkgName.startsWith('@')) {
    throw new PnpmError('MISSING_JSR_PACKAGE_SCOPE', 'Package names from JSR must have a scope')
  }
  const { scope, name } = scopeAndName(jsrPkgName)
  return `@jsr/${scope.substring(1)}__${name}`
}

function scopeAndName (fullNameWithoutLeadingAt: string): { scope: string, name: string } {
  const sepIndex = fullNameWithoutLeadingAt.indexOf('/')
  if (sepIndex === -1) {
    throw new PnpmError('INVALID_JSR_PACKAGE_NAME', `The package name '@${fullNameWithoutLeadingAt}' is invalid`)
  }

  const scope = fullNameWithoutLeadingAt.substring(0, sepIndex)
  const name = fullNameWithoutLeadingAt.substring(sepIndex + '/'.length)
  return { scope, name }
}
