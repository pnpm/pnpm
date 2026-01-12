export interface UnwrappedPackageInfo {
  readonly pkgName: string
  readonly bareSpecifier: string
}

/**
 * When declaring dependencies in a package.json file, it's possible to specify
 * an "alias".
 *
 * An example of this would be:
 *
 * @example
 * ```json
 * {
 *   "dependencies": {
 *     "my-alias": "npm:is-positive@^1.0.0"
 *   }
 * }
 * ```
 *
 * This function normalizes out the alias and returns the real package name
 * (i.e. "is-positive" for this example) if "npm:" is used.
 */
export function unwrapPackageName (alias: string, originalBareSpecifier: string): UnwrappedPackageInfo {
  // If the specifier doesn't start with "npm:", there's no more work to do. The
  // "alias" argument is just the real package's name.
  if (!originalBareSpecifier.startsWith('npm:')) {
    return { pkgName: alias, bareSpecifier: originalBareSpecifier }
  }

  const npmAliasSpecifierValue = originalBareSpecifier.slice(4)

  const index = npmAliasSpecifierValue.lastIndexOf('@')

  // If the "@" character isn't found or is the first character, then
  // this alias is just a package name without a spec. Examples:
  // "npm:is-positive", "npm:@pnpm/lockfile.fs"
  //
  // In this case, the bare specifier will be "*".
  if (index === -1 || index === 0) {
    return { pkgName: npmAliasSpecifierValue, bareSpecifier: '*' }
  }

  const bareSpecifier = npmAliasSpecifierValue.slice(index + 1)
  const pkgName = npmAliasSpecifierValue.substring(0, index)

  return { pkgName, bareSpecifier }
}
