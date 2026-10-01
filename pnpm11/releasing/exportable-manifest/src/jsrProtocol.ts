import { parseJsrSpecifier } from '@pnpm/resolving.jsr-specifier-parser'

export async function replaceJsrProtocol (depName: string, depSpec: string): Promise<string> {
  const spec = parseJsrSpecifier(depSpec, depName)
  if (spec == null) {
    return depSpec
  }
  return createNpmAliasedSpecifier(spec.npmPkgName, spec.versionSelector)
}

function createNpmAliasedSpecifier (npmPkgName: string, versionSelector?: string): string {
  const npmPkgSpecifier = `npm:${npmPkgName}`
  if (!versionSelector) {
    return npmPkgSpecifier
  }
  return `${npmPkgSpecifier}@${versionSelector}`
}
