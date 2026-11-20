import { checkbox } from '@inquirer/prompts'
import { interactivePromptPageSize, readProjectManifestOnly } from '@pnpm/cli.utils'
import type { OutdatedPackage } from '@pnpm/deps.inspection.outdated'
import { hasPnpmCliDependency } from '@pnpm/global.commands'
import { type GlobalPackageInfo, scanGlobalPackages } from '@pnpm/global.packages'
import { sanitizeInline } from '@pnpm/text.sanitize'
import type { ProjectRootDir } from '@pnpm/types'

import { findOutdatedDependencies } from './findOutdatedDependencies.js'
import type { UpdateCommandOptions } from './update.js'
import { describeUpToDate, runUpdatePrompt } from './updatePrompt.js'

interface GlobalGroupChoice {
  name: string
  value: string
}

export async function selectGlobalPackageGroups (
  input: string[],
  opts: UpdateCommandOptions
): Promise<Set<string> | string> {
  const scannedPackages = scanGlobalPackages(opts.globalPkgDir!)
  if (scannedPackages.length === 0) return 'No global packages found'
  // The pnpm CLI's own global install belongs to `pnpm self-update`, so it is
  // never offered as a choice. See `hasPnpmCliDependency`.
  const globalPackages = scannedPackages.filter((pkg) => !hasPnpmCliDependency(pkg))
  if (globalPackages.length === 0) {
    return 'No global packages to update. Run "pnpm self-update" to update pnpm itself.'
  }
  // A global group is always updated as a whole, so the params select groups
  // rather than dependencies, the same way `handleGlobalUpdate()` reads them.
  const matchedPackages = input.length === 0
    ? globalPackages
    : globalPackages.filter((pkg) => input.some((param) => Object.hasOwn(pkg.dependencies, param)))
  if (matchedPackages.length === 0) return 'No matching global packages found'
  const choices = await createOutdatedGroupChoices(matchedPackages, opts)
  if (choices.length === 0) {
    return describeUpToDate(opts.latest)
  }
  return new Set(await runUpdatePrompt(() => checkbox({
    choices,
    message: 'Choose which global package groups to update (space to select, enter to confirm)',
    pageSize: Math.min(choices.length, interactivePromptPageSize()),
  })))
}

async function createOutdatedGroupChoices (
  globalPackages: GlobalPackageInfo[],
  opts: UpdateCommandOptions
): Promise<GlobalGroupChoice[]> {
  const outdatedPerGroup = await Promise.all(globalPackages.map(async (pkg) => ({
    pkg,
    outdated: await findOutdatedDependenciesOfGroup(pkg, opts),
  })))
  return outdatedPerGroup
    .filter(({ outdated }) => outdated.length > 0)
    .map(({ pkg, outdated }) => ({
      name: outdated
        .map((outdatedPkg) => describeOutdatedDependency(outdatedPkg, opts.latest))
        .join(', '),
      value: pkg.hash,
    }))
}

async function findOutdatedDependenciesOfGroup (
  pkg: GlobalPackageInfo,
  opts: UpdateCommandOptions
): Promise<OutdatedPackage[]> {
  const project = {
    rootDir: pkg.installDir as ProjectRootDir,
    manifest: await readProjectManifestOnly(pkg.installDir, opts),
  }
  const [outdated] = await findOutdatedDependencies([project], [], {
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
    opts,
  })
  return outdated
}

function describeOutdatedDependency (
  { alias, current, wanted, latestManifest }: OutdatedPackage,
  latest: boolean | undefined
): string {
  return [alias, current ?? 'missing', '→', latest ? latestManifest?.version ?? wanted : wanted]
    .map(sanitizeInline)
    .join(' ')
}
