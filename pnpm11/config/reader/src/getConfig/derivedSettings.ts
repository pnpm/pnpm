import { createMatcher } from '@pnpm/config.matcher'
import { PnpmError } from '@pnpm/error'
import { getCurrentBranch } from '@pnpm/network.git-utils'

import { extractAndRemoveDependencyBuildOptions, hasDependencyBuildOptions } from '../dependencyBuildOptions.js'
import { npmDefaults } from '../npmDefaults.js'
import { overrideSupportedArchitecturesWithCLI } from '../overrideSupportedArchitecturesWithCLI.js'
import type { CliOptions, ConfigBuildState, PnpmConfigInProgress } from './configBuildState.js'
import type { MaxSocketsFromEnv } from './envVars.js'
import { warnAboutUnmatchedRegistryOptions } from './registries.js'

export type DependencyBuildOptions = ReturnType<typeof extractAndRemoveDependencyBuildOptions>

/** Settings derived once every source (files, environment, CLI) was applied. */
export async function applyInstallSettingsFromAllSources (
  state: ConfigBuildState,
  { maxSocketsFromEnv, globalDepsBuildConfig }: { maxSocketsFromEnv: MaxSocketsFromEnv, globalDepsBuildConfig: DependencyBuildOptions }
): Promise<void> {
  const { cliOptions, pnpmConfig } = state
  // After the env loop: PNPM_CONFIG_REGISTRY can still change
  // `registries.default` above, and an entry matching it must not be reported
  // as unused.
  warnAboutUnmatchedRegistryOptions(pnpmConfig, state.warnings)

  foldMaxSockets(state, maxSocketsFromEnv)

  // When the user explicitly sets `minimumReleaseAge`, treat it as strict by
  // default. Without this, a user-set value would silently fall back to
  // installing an immature version when no mature version satisfies the
  // requested range — making the setting look like it had no effect.
  // The built-in default for `minimumReleaseAge` is intentionally non-strict
  // for backward compatibility. This must run after env var parsing so
  // pnpm_config_minimum_release_age also enables strict mode.
  if (
    pnpmConfig.explicitlySetKeys.has('minimumReleaseAge') &&
    pnpmConfig.minimumReleaseAgeStrict == null
  ) {
    pnpmConfig.minimumReleaseAgeStrict = true
  }

  overrideSupportedArchitecturesWithCLI(pnpmConfig, cliOptions)

  await resolveLockfileSettings(pnpmConfig)
  applyBuildPolicyDefaults(pnpmConfig, globalDepsBuildConfig)
  assertSavePeerIsExclusive(cliOptions)
  splitSpaceSeparatedFilters(pnpmConfig)

  if (pnpmConfig.sharedWorkspaceLockfile && !pnpmConfig.lockfileDir && pnpmConfig.workspaceDir) {
    pnpmConfig.lockfileDir = pnpmConfig.workspaceDir
  }
}

/**
 * Also after the env loop, and after the config files were applied: npm
 * spells the setting `maxsockets`, so every source may carry either
 * spelling and both have to be folded into the one field the rest of
 * pnpm reads. The layers keep their usual rank — command line over
 * environment over config files — and within each layer the canonical
 * spelling wins. Ranking the command line here rather than leaving it to
 * the loop's CLI guard is what keeps a `--maxsockets` above a
 * `PNPM_CONFIG_MAX_SOCKETS`, and above a `maxSockets` in the YAML.
 * npm's own default stands in when no layer set either.
 */
function foldMaxSockets ({ configFromCliOpts, pnpmConfig }: ConfigBuildState, { maxSocketsFromEnv, maxsocketsFromEnv }: MaxSocketsFromEnv): void {
  const maxSocketsFromCli = (configFromCliOpts.maxSockets ?? configFromCliOpts.maxsockets) as number | undefined
  // @ts-expect-error - maxsockets (lowercase) comes from npmConfigTypes, maxSockets (camelCase) is the Config field
  const maxSocketsFromFiles: number | undefined = pnpmConfig.maxSockets ?? pnpmConfig['maxsockets']
  pnpmConfig.maxSockets = maxSocketsFromCli ?? maxSocketsFromEnv ?? maxsocketsFromEnv ?? maxSocketsFromFiles ?? npmDefaults.maxsockets
  // @ts-expect-error -- maxsockets (lowercase) comes from npmConfigTypes and is not a Config field
  delete pnpmConfig['maxsockets']
}

async function resolveLockfileSettings (pnpmConfig: PnpmConfigInProgress): Promise<void> {
  pnpmConfig.useLockfile = (() => {
    if (typeof pnpmConfig.lockfile === 'boolean') return pnpmConfig.lockfile
    if (typeof pnpmConfig.packageLock === 'boolean') return pnpmConfig.packageLock
    return false
  })()

  pnpmConfig.useGitBranchLockfile = (() => {
    if (typeof pnpmConfig.gitBranchLockfile === 'boolean') return pnpmConfig.gitBranchLockfile
    return false
  })()
  pnpmConfig.mergeGitBranchLockfiles = await shouldMergeGitBranchLockfiles(pnpmConfig)
}

async function shouldMergeGitBranchLockfiles (pnpmConfig: PnpmConfigInProgress): Promise<boolean | undefined> {
  if (typeof pnpmConfig.mergeGitBranchLockfiles === 'boolean') return pnpmConfig.mergeGitBranchLockfiles
  if (pnpmConfig.mergeGitBranchLockfilesBranchPattern == null || pnpmConfig.mergeGitBranchLockfilesBranchPattern.length === 0) {
    return undefined
  }
  const branch = await getCurrentBranch()
  if (!branch) return undefined
  const branchMatcher = createMatcher(pnpmConfig.mergeGitBranchLockfilesBranchPattern)
  return branchMatcher(branch)
}

function applyBuildPolicyDefaults (pnpmConfig: PnpmConfigInProgress, globalDepsBuildConfig: DependencyBuildOptions): void {
  if (!hasDependencyBuildOptions(pnpmConfig)) {
    Object.assign(pnpmConfig, globalDepsBuildConfig)
  }
  // Default allowBuilds to {} when GVS is enabled and no build policy is
  // configured. This makes GVS hashes engine-agnostic for pure-JS packages.
  // When a build policy (dangerouslyAllowAllBuilds from global config.yaml,
  // or allowBuilds from the workspace manifest) exists, GVS hashes must
  // include ENGINE_NAME so that built packages and their dependents are
  // correctly invalidated across Node upgrades and architecture changes.
  if (
    pnpmConfig.enableGlobalVirtualStore &&
    pnpmConfig.allowBuilds == null &&
    pnpmConfig.dangerouslyAllowAllBuilds !== true
  ) {
    pnpmConfig.allowBuilds = {}
  }
}

function assertSavePeerIsExclusive (cliOptions: CliOptions): void {
  if (!cliOptions['save-peer']) return
  if (cliOptions['save-prod']) {
    throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_PROD_DEP', 'A package cannot be a peer dependency and a prod dependency at the same time')
  }
  if (cliOptions['save-optional']) {
    throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP',
      'A package cannot be a peer dependency and an optional dependency at the same time')
  }
}

function splitSpaceSeparatedFilters (pnpmConfig: PnpmConfigInProgress): void {
  if (typeof pnpmConfig.filter === 'string') {
    pnpmConfig.filter = (pnpmConfig.filter as string).split(' ')
  }

  if (typeof pnpmConfig.filterProd === 'string') {
    pnpmConfig.filterProd = (pnpmConfig.filterProd as string).split(' ')
  }
}
