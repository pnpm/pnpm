import { searchForPackages, flattenSearchedPackages } from '@pnpm/list'
import { parseWantedDependency, ParseWantedDependencyResult } from '@pnpm/parse-wanted-dependency'
import { prompt } from 'enquirer'
import { logger } from '@pnpm/logger'
import type { Config } from '@pnpm/config'

export async function getPatchedDependency ({
  pkg,
  selectedProjectsGraph,
  lockfileDir,
}: {
  pkg: string
  lockfileDir: string
} & Pick<Config, 'selectedProjectsGraph'>): Promise<ParseWantedDependencyResult> {
  const dep = parseWantedDependency(pkg)
  let prefixes = [lockfileDir]
  if (selectedProjectsGraph) {
    prefixes = Object.values(selectedProjectsGraph).map(wsPkg => wsPkg.package.dir)
  }
  const pkgs = await searchForPackages([pkg], prefixes, {
    depth: Infinity,
    lockfileDir,
  })

  const versions = Array.from(new Set(flattenSearchedPackages(pkgs, { lockfileDir })
    .map(({ version }) => version)))
    .filter(Boolean) as string[]

  if (dep.alias && dep.pref) {
    if (!versions.length) {
      logger.warn({
        message: `Can not find ${dep.alias}@${dep.pref} in project ${lockfileDir}${versions.length ? `, you can specify currently installed version: ${versions.join(', ')} ` : ''}`,
        prefix: lockfileDir,
      })
    }
  }

  if (versions.length) {
    dep.alias = dep.alias ?? pkg
    if (versions.length > 1) {
      const { version } = await prompt<{
        version: string
      }>({
        type: 'select',
        name: 'version',
        message: 'Choose which version to patch',
        choices: versions,
      })
      dep.pref = version
    } else {
      dep.pref = versions[0]
    }
  }

  return dep
}
