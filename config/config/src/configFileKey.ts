import npmTypes from '@pnpm/npm-conf/lib/types.js'
import { type pnpmTypes } from './types.js'

type NpmKey = keyof typeof npmTypes.types
type PnpmKey = keyof typeof pnpmTypes

/**
 * Keys from {@link pnpmTypes} that are valid fields in a global config file.
 */
export const pnpmConfigFileKeys = [
  'bail',
  'ci',
  'color',
  'cache-dir',
  'child-concurrency',
  'dangerously-allow-all-builds',
  'enable-modules-dir',
  'enable-global-virtual-store',
  'exclude-links-from-lockfile',
  'extend-node-path',
  'fetch-timeout',
  'fetch-warn-timeout-ms',
  'fetch-min-speed-ki-bps',
  'fetching-concurrency',
  'git-checks',
  'git-shallow-hosts',
  'global-bin-dir',
  'global-dir',
  'global-path',
  'global-pnpmfile',
  'optimistic-repeat-install',
  'loglevel',
  'maxsockets',
  'modules-cache-max-age',
  'dlx-cache-max-age',
  'minimum-release-age',
  'minimum-release-age-exclude',
  'network-concurrency',
  'noproxy',
  'npm-path',
  'package-import-method',
  'prefer-frozen-lockfile',
  'prefer-offline',
  'prefer-symlinked-executables',
  'block-exotic-subdeps',
  'reporter',
  'resolution-mode',
  'store-dir',
  'use-beta-cli',
] as const satisfies readonly PnpmKey[]
export type PnpmConfigFileKey = typeof pnpmConfigFileKeys[number]

/**
 * Keys that present in {@link pnpmTypes} but are excluded from {@link ConfigFileKey}.
 * They are usually CLI flags or workspace-only settings.
 */
export const excludedPnpmKeys = [
  'auto-install-peers',
  'catalog-mode',
  'config-dir',
  'merge-git-branch-lockfiles',
  'merge-git-branch-lockfiles-branch-pattern',
  'deploy-all-files',
  'dedupe-peer-dependents',
  'dedupe-direct-deps',
  'dedupe-injected-deps',
  'dev',
  'dir',
  'disallow-workspace-cycles',
  'enable-pre-post-scripts',
  'filter',
  'filter-prod',
  'force-legacy-deploy',
  'frozen-lockfile',
  'git-branch-lockfile',
  'hoist',
  'hoist-pattern',
  'hoist-workspace-packages',
  'ignore-compatibility-db',
  'ignore-dep-scripts',
  'ignore-pnpmfile',
  'ignore-workspace',
  'ignore-workspace-cycles',
  'ignore-workspace-root-check',
  'include-workspace-root',
  'init-package-manager',
  'init-type',
  'inject-workspace-packages',
  'legacy-dir-filtering',
  'link-workspace-packages',
  'lockfile',
  'lockfile-dir',
  'lockfile-directory',
  'lockfile-include-tarball-url',
  'lockfile-only',
  'manage-package-manager-versions',
  'modules-dir',
  'node-linker',
  'offline',
  'pack-destination',
  'pack-gzip-level',
  'patches-dir',
  'pnpmfile',
  'package-manager-strict',
  'package-manager-strict-version',
  'prefer-workspace-packages',
  'preserve-absolute-paths',
  'production',
  'public-hoist-pattern',
  'publish-branch',
  'recursive-install',
  'resolve-peers-from-workspace-root',
  'aggregate-output',
  'reporter-hide-prefix',
  'save-catalog-name',
  'save-peer',
  'save-workspace-protocol',
  'script-shell',
  'shamefully-flatten',
  'shamefully-hoist',
  'shared-workspace-lockfile',
  'shell-emulator',
  'side-effects-cache',
  'side-effects-cache-readonly',
  'symlink',
  'sort',
  'state-dir',
  'stream',
  'strict-dep-builds',
  'strict-store-pkg-content-check',
  'strict-peer-dependencies',
  'trust-policy',
  'trust-policy-exclude',
  'trust-policy-ignore-after',
  'use-stderr',
  'verify-deps-before-run',
  'verify-store-integrity',
  'global-virtual-store-dir',
  'virtual-store-dir',
  'virtual-store-dir-max-length',
  'peers-suffix-max-length',
  'workspace-concurrency',
  'workspace-packages',
  'workspace-root',
  'test-pattern',
  'changed-files-ignore-pattern',
  'embed-readme',
  'update-notifier',
  'registry-supports-time-field',
  'fail-if-no-match',
  'sync-injected-deps-after-scripts',
  'cpu',
  'libc',
  'os',
  'audit-level',
  'yes',
] as const satisfies ReadonlyArray<Exclude<PnpmKey, PnpmConfigFileKey>>
export type ExcludedPnpmKey = typeof excludedPnpmKeys[number]

/**
 * Proof that {@link excludedPnpmKeys} is complete and exhaustive, i.e. All keys that appear in {@link pnpmTypes} but not in
 * {@link pnpmConfigFileKeys} should be included in {@link excludedPnpmKeys}.
 */
export const _proofExcludedPnpmKeysIsExhaustive = (carrier: Exclude<PnpmKey, PnpmConfigFileKey>): ExcludedPnpmKey => carrier

/**
 * Proof that there are no keys that are both included and excluded, i.e. {@link pnpmConfigFileKeys} and {@link excludedPnpmKeys}
 * have no overlap.
 */
export const _proofNoContradiction = (carrier: PnpmConfigFileKey & ExcludedPnpmKey): never => carrier

// even npmTypes still have keys that don't make sense in global config, but the list is quite long, let's do it another day.
// TODO: compile a list of npm keys that are valid or invalid in a global config file.
export type NpmConfigFileKey = Exclude<NpmKey, ExcludedPnpmKey>

/** Key that is valid in a global config file. */
export type ConfigFileKey = NpmConfigFileKey | PnpmConfigFileKey

const setOfPnpmConfigFilesKeys: ReadonlySet<string> = new Set(pnpmConfigFileKeys)
const setOfExcludedPnpmKeys: ReadonlySet<string> = new Set(excludedPnpmKeys)

/** Whether the key (in kebab-case) is a valid key in a global config file. */
export const isConfigFileKey = (kebabKey: string): kebabKey is ConfigFileKey =>
  setOfPnpmConfigFilesKeys.has(kebabKey) || (kebabKey in npmTypes.types && !setOfExcludedPnpmKeys.has(kebabKey))
