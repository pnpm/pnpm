import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { isCamelCase } from '@pnpm/text.naming-cases'
import type { ProjectManifest, RemoteSideEffectsCacheSettings } from '@pnpm/types'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

import type { Config, ConfigContext, VerifyDepsBeforeRun } from '../Config.js'
import { getOptionsFromPnpmSettings } from '../getOptionsFromRootManifest.js'
import { CONFIG_CONTEXT_KEY_SET } from './configContext.js'
import { applySideEffectsCacheDeclaration, withCanonicalOrg } from './sideEffectsCache.js'
import type { SkippableKey } from './skippedKeys.js'

export interface WorkspaceManifestSettingsOptions {
  configFromCliOpts: Record<string, unknown>
  expandRequestDestinationEnv?: boolean
  projectManifest: ProjectManifest | undefined
  /** Settings this manifest may not contribute, chosen by the caller. */
  skipSettings?: ReadonlySet<SkippableKey>
  /** See {@link getOptionsFromPnpmSettings}. Only the global config yaml is trusted. */
  trustedSource?: boolean
  workspaceDir: string | undefined
  workspaceManifest: WorkspaceManifest
}

export function addSettingsFromWorkspaceManifestToConfig (pnpmConfig: Config & ConfigContext, {
  trustedSource,
  configFromCliOpts,
  expandRequestDestinationEnv,
  projectManifest,
  skipSettings,
  workspaceManifest,
  workspaceDir,
}: WorkspaceManifestSettingsOptions): void {
  const skipped: ReadonlySet<string> | undefined = skipSettings
  const settingsFromManifest = getOptionsFromPnpmSettings(workspaceDir, workspaceManifest, { manifest: projectManifest, expandRequestDestinationEnv, trustedSource })
  const sideEffectsCacheFromManifest = settingsFromManifest.sideEffectsCache
  const newSettings = Object.assign(settingsFromManifest, configFromCliOpts)
  for (const [key, value] of Object.entries(newSettings)) {
    if (!isCamelCase(key)) continue
    if (CONFIG_CONTEXT_KEY_SET.has(key)) continue
    if (skipped?.has(key)) continue
    applyWorkspaceSetting(pnpmConfig, { key, value, sideEffectsCacheFromManifest })
    pnpmConfig.explicitlySetKeys.add(key)
  }
  // All the pnpm_config_ env variables should override the settings from pnpm-workspace.yaml,
  // as it happens with .npmrc.
  // Until that is fixed, we should at the very least keep the right priority for verifyDepsBeforeRun,
  // or else, we'll get infinite recursion.
  // Related issue: https://github.com/pnpm/pnpm/issues/10060
  if (process.env.pnpm_config_verify_deps_before_run != null) {
    pnpmConfig.verifyDepsBeforeRun = process.env.pnpm_config_verify_deps_before_run as VerifyDepsBeforeRun
  }
  pnpmConfig.catalogs = getCatalogsFromWorkspaceManifest(workspaceManifest)
}

interface WorkspaceSetting {
  key: string
  value: unknown
  sideEffectsCacheFromManifest: unknown
}

function applyWorkspaceSetting (pnpmConfig: Config & ConfigContext, { key, value, sideEffectsCacheFromManifest }: WorkspaceSetting): void {
  // A workspace declares eligibility while the machine holds the signing
  // trust root, so the two sources contribute different fields of one object
  // and the later one must not drop what the earlier one set.
  //
  // The two spellings accumulate separately and are combined once, in
  // `resolveSideEffectsCache`. Merging them here would make precedence a
  // function of the order the keys happen to appear in, so a file listing
  // the deprecated spelling second would have it win.
  if (key === 'remoteSideEffectsCache') {
    pnpmConfig.remoteSideEffectsCache = {
      ...pnpmConfig.remoteSideEffectsCache,
      ...withCanonicalOrg(value as RemoteSideEffectsCacheSettings),
    }
    return
  }
  if (key === 'sideEffectsCache') {
    // The command line is a layer on top of the manifest, not a substitute
    // for it: applying only the value that won the merge above would drop a
    // remote tier the manifest declared, which the boolean says nothing
    // about.
    if (sideEffectsCacheFromManifest != null && sideEffectsCacheFromManifest !== value) {
      applySideEffectsCacheDeclaration(pnpmConfig, sideEffectsCacheFromManifest)
    }
    applySideEffectsCacheDeclaration(pnpmConfig, value)
    return
  }
  // @ts-expect-error -- the value's type depends on which key this is
  pnpmConfig[key] = value
}
