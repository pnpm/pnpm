import {
  scanGlobalPackages,
  getGlobalPackageDetails,
} from '@pnpm/global.packages'
import { createMatcher } from '@pnpm/matcher'
import { lexCompare } from '@pnpm/util.lex-comparator'

export async function listGlobalPackages (globalPkgDir: string, params: string[]): Promise<string> {
  const packages = scanGlobalPackages(globalPkgDir)
  const allDetails = await Promise.all(packages.map((pkg) => getGlobalPackageDetails(pkg)))
  const matches = params.length > 0 ? createMatcher(params) : () => true
  const lines: string[] = []
  for (const installed of allDetails.flat()) {
    if (!matches(installed.alias)) continue
    lines.push(`${installed.alias}@${installed.version}`)
  }
  if (lines.length === 0) {
    return params.length > 0
      ? 'No matching global packages found'
      : 'No global packages found'
  }
  lines.sort(lexCompare)
  return lines.join('\n')
}
