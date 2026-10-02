import { isCI } from 'ci-info'
import isWindows from 'is-windows'

import { getDefaultWorkspaceConcurrency } from '../concurrency.js'
import type { ConfigWithDeprecatedSettings } from '../Config.js'
import { npmDefaults } from '../npmDefaults.js'

type CamelToKebabCase<Name extends string> = Name extends `${infer Head}${infer Rest}`
  ? `${Head extends Lowercase<Head> ? '' : '-'}${Lowercase<Head>}${CamelToKebabCase<Rest>}`
  : Name

export type KebabCaseConfig = {
  [K in keyof ConfigWithDeprecatedSettings as CamelToKebabCase<K>]: ConfigWithDeprecatedSettings[K];
}

export function createDefaultOptions (workspaceDir: string | undefined): Partial<KebabCaseConfig> {
  return {
    ...defaultOptions,
    'workspace-concurrency': getDefaultWorkspaceConcurrency(),
    'workspace-prefix': workspaceDir,
    'virtual-store-dir-max-length': isWindows() ? 60 : 120,
  }
}

const defaultOptions: Partial<KebabCaseConfig> = {
  'auto-install-peers': true,
  bail: true,
  'catalog-mode': 'manual',
  ci: isCI,
  color: 'auto',
  'dangerously-allow-all-builds': false,
  'deploy-all-files': false,
  'dedupe-peer-dependents': true,
  'dedupe-peers': false,
  'dedupe-direct-deps': false,
  'dedupe-injected-deps': true,
  'disallow-workspace-cycles': false,
  'enable-modules-dir': true,
  'node-experimental-package-map': false,
  'node-package-map-type': 'standard',
  'enable-pre-post-scripts': true,
  'exclude-links-from-lockfile': false,
  'extend-node-path': true,
  'fail-if-no-match': false,
  'fetch-retries': 2,
  'fetch-retry-factor': 10,
  'fetch-retry-maxtimeout': 60000,
  'fetch-retry-mintimeout': 10000,
  'fetch-timeout': 60000,
  'fetch-warn-timeout-ms': 10_000, // 10 sec
  'fetch-min-speed-ki-bps': 50, // 50 KiB/s
  'force-ignores-platform': true,
  'force-legacy-deploy': false,
  'git-shallow-hosts': [
    // Follow https://github.com/npm/git/blob/1e1dbd26bd5b87ca055defecc3679777cb480e2a/lib/clone.js#L13-L19
    'github.com',
    'gist.github.com',
    'gitlab.com',
    'bitbucket.com',
    'bitbucket.org',
  ],
  'git-branch-lockfile': false,
  hoist: true,
  'hoist-pattern': ['*'],
  'hoist-workspace-packages': true,
  'ignore-workspace-cycles': false,
  'ignore-workspace-root-check': false,
  'optimistic-repeat-install': true,
  optional: true,
  'init-package-manager': true,
  'init-type': 'module',
  'inject-workspace-packages': false,
  'link-workspace-packages': false,
  'lockfile-include-tarball-url': false,
  'minimum-release-age': 24 * 60, // 1 day
  'minimum-release-age-ignore-missing-time': true,
  'modules-cache-max-age': 7 * 24 * 60, // 7 days
  'dlx-cache-max-age': 24 * 60, // 1 day
  'node-linker': 'isolated',
  'package-lock': npmDefaults['package-lock'],
  pending: false,
  'prefer-workspace-packages': false,
  'public-hoist-pattern': [],
  'recursive-install': true,
  registry: npmDefaults.registry,
  'block-exotic-subdeps': true,
  'resolution-mode': 'highest',
  'resolve-peers-from-workspace-root': true,
  'save-peer': false,
  'save-catalog-name': undefined,
  'save-workspace-protocol': 'rolling',
  'scripts-prepend-node-path': false,
  'strict-dep-builds': true,
  'side-effects-cache': true,
  symlink: true,
  'shared-workspace-lockfile': true,
  'shell-emulator': false,
  'strict-store-pkg-content-check': true,
  reverse: false,
  sort: true,
  'strict-peer-dependencies': false,
  'unsafe-perm': npmDefaults['unsafe-perm'],
  'use-beta-cli': false,
  userconfig: npmDefaults.userconfig,
  'verify-deps-before-run': 'install',
  'verify-store-integrity': true,
  'frozen-store': false,
  'workspace-concurrency': 4,
  'workspace-prefix': undefined,
  'embed-readme': false,
  'skip-manifest-obfuscation': false,
  'registry-supports-time-field': false,
  'virtual-store-dir-max-length': 120,
  'virtual-store-only': false,
  'peers-suffix-max-length': 1000,
}

