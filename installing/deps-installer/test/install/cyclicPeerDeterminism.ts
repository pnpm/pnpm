import { afterEach, expect, test } from '@jest/globals'
import { type MutatedProject, mutateModules, type MutateModulesOptions, type ProjectOptions } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

afterEach(async () => {
  await teardownMockAgent()
})

// Regression test for https://github.com/pnpm/pnpm/issues/8155.
//
// Reproduces the @aws-sdk/client-sts ↔ @aws-sdk/client-sso-oidc situation: a
// "wrapper" package pulls in two transitive deps that declare each other as
// peer dependencies, and `auto-install-peers` hoists those two peers up to the
// workspace root. Before the fix, `resolveDependencies` pushed onto its
// pkgAddresses / postponedResolutionsQueue arrays from inside
// `Promise.all`-spawned callbacks, so completion order leaked into the array
// order and the cyclic-peer suffix flipped between two equally valid forms
// across consecutive installs.
test('cyclic transitive peer dependencies resolve deterministically across installs', async () => {
  const rootProject = prepareEmpty()
  const lockfileDir = rootProject.dir()

  const wrapperName = '@pnpm.e2e/cyclic-wrapper'
  const aName = '@pnpm.e2e/cyclic-a'
  const bName = '@pnpm.e2e/cyclic-b'

  const manifest: ProjectManifest = {
    name: 'root',
    dependencies: {
      [wrapperName]: '1.0.0',
    },
  }
  const allProjects: ProjectOptions[] = [{
    buildIndex: 0,
    manifest,
    rootDir: lockfileDir as ProjectRootDir,
  }]
  const options = {
    ...testDefaults(
      { allProjects, autoInstallPeers: true, forceFullResolution: true },
      { retry: { retries: 0 } }
    ),
    lockfileDir,
    lockfileOnly: true,
  } satisfies MutateModulesOptions

  const installProjects: MutatedProject[] = [{
    mutation: 'install',
    rootDir: lockfileDir as ProjectRootDir,
  }]

  const registryUrl = options.registries.default.replace(/\/$/, '')

  function makeMeta (name: string, deps: Record<string, string>, peerDeps: Record<string, string>): PackageMeta {
    return {
      name,
      versions: {
        '1.0.0': {
          name,
          version: '1.0.0',
          dependencies: deps,
          peerDependencies: peerDeps,
          dist: {
            // Resolver only reads metadata when lockfileOnly is true, so the
            // shasum value is never checked against a tarball.
            shasum: '0000000000000000000000000000000000000000',
            tarball: `${options.registries.default}/${encodeURIComponent(name)}-1.0.0.tgz`,
          },
        },
      },
      'dist-tags': { latest: '1.0.0' },
    }
  }

  const metaByName = {
    [wrapperName]: makeMeta(wrapperName, { [aName]: '1.0.0', [bName]: '1.0.0' }, {}),
    [aName]: makeMeta(aName, {}, { [bName]: '1.0.0' }),
    [bName]: makeMeta(bName, {}, { [aName]: '1.0.0' }),
  }

  function metadataPath (name: string): string {
    return `/${name.replaceAll('/', '%2F')}`
  }

  function arm (): void {
    const agent = getMockAgent().get(registryUrl)
    for (const [name, meta] of Object.entries(metaByName)) {
      agent.intercept({ path: metadataPath(name), method: 'GET' }).reply(200, meta).persist()
    }
  }

  // ~30 iterations gives a ≥ (1 − 2⁻²⁹) chance of catching a 50/50 flip if
  // the bug returns. The fix makes the walk order canonical so a single run
  // would suffice, but iterating cheaply hedges against scheduling drift.
  const iterations = 30

  async function runOnce (): Promise<string[]> {
    await setupMockAgent()
    arm()
    options.storeController.clearResolutionCache()
    await mutateModules(installProjects, options)
    const lockfile = rootProject.readLockfile()
    const snapshotKeys = Object.keys(lockfile.snapshots ?? {})
    await teardownMockAgent()
    return snapshotKeys
  }

  const first = JSON.stringify(await runOnce())
  for (let i = 1; i < iterations; i++) {
    const subsequent = JSON.stringify(await runOnce()) // eslint-disable-line no-await-in-loop
    expect(subsequent).toEqual(first)
  }
})

// Regression test for https://github.com/pnpm/pnpm/issues/11999.
//
// An aliased install (`a@npm:a-real`) pulls in two siblings `b` and `c` that
// each bring a transitive package — `x` under `b`, `y` under `c` — declaring
// each other as peer dependencies. Both `x` and `y` are auto-installed at the
// importer root via the missing-peer hoist. The aliased install also yields a
// transitive peer back to `a`. The deep instances of `x` and `y` populate the
// peersCache during `a`'s subtree walk, so the auto-installed top-level
// instances hit findHit instead of running their own calculateDepPath. The
// cycle between `x` and `y` is detected at the project root level but does
// not include the root alias `a`, so awaiting it must not deadlock.
test('aliased install with a transitive mutual-peer cycle should not hang', async () => {
  const rootProject = prepareEmpty()
  const lockfileDir = rootProject.dir()

  const aRealName = '@pnpm.e2e/aliased-cycle-a-real'
  const bRealName = '@pnpm.e2e/aliased-cycle-b-real'
  const cRealName = '@pnpm.e2e/aliased-cycle-c-real'
  const xName = '@pnpm.e2e/aliased-cycle-x'
  const yName = '@pnpm.e2e/aliased-cycle-y'

  const manifest: ProjectManifest = {
    name: 'root',
    dependencies: {
      a: `npm:${aRealName}@1.0.0`,
    },
  }
  const allProjects: ProjectOptions[] = [{
    buildIndex: 0,
    manifest,
    rootDir: lockfileDir as ProjectRootDir,
  }]
  const options = {
    ...testDefaults(
      { allProjects, autoInstallPeers: true, forceFullResolution: true },
      { retry: { retries: 0 } }
    ),
    lockfileDir,
    lockfileOnly: true,
  } satisfies MutateModulesOptions

  const installProjects: MutatedProject[] = [{
    mutation: 'install',
    rootDir: lockfileDir as ProjectRootDir,
  }]

  const registryUrl = options.registries.default.replace(/\/$/, '')

  function makeMeta (name: string, deps: Record<string, string>, peerDeps: Record<string, string>): PackageMeta {
    return {
      name,
      versions: {
        '1.0.0': {
          name,
          version: '1.0.0',
          dependencies: deps,
          peerDependencies: peerDeps,
          dist: {
            shasum: '0000000000000000000000000000000000000000',
            tarball: `${options.registries.default}/${encodeURIComponent(name)}-1.0.0.tgz`,
          },
        },
      },
      'dist-tags': { latest: '1.0.0' },
    }
  }

  const metaByName = {
    [aRealName]: makeMeta(aRealName, {
      b: `npm:${bRealName}@1.0.0`,
      c: `npm:${cRealName}@1.0.0`,
    }, {}),
    [bRealName]: makeMeta(bRealName, {
      [xName]: '1.0.0',
    }, {
      a: `npm:${aRealName}@1.0.0`,
    }),
    [cRealName]: makeMeta(cRealName, {
      [yName]: '1.0.0',
    }, {
      a: `npm:${aRealName}@1.0.0`,
    }),
    [xName]: makeMeta(xName, {}, {
      [yName]: '1.0.0',
    }),
    [yName]: makeMeta(yName, {}, {
      [xName]: '1.0.0',
    }),
  }

  await setupMockAgent()
  const agent = getMockAgent().get(registryUrl)
  for (const [name, meta] of Object.entries(metaByName)) {
    agent.intercept({ path: `/${name.replaceAll('/', '%2F')}`, method: 'GET' }).reply(200, meta).persist()
  }

  options.storeController.clearResolutionCache()
  await mutateModules(installProjects, options)

  const lockfile = rootProject.readLockfile()
  expect(lockfile.importers?.['.']?.dependencies?.a?.version).toContain(`${aRealName}@1.0.0`)
})

test('transitivePeerDependencies propagate through regular dep cycles', async () => {
  const rootProject = prepareEmpty()
  const lockfileDir = rootProject.dir()

  const parentName = '@pnpm.e2e/tpd-cycle-parent'
  const aName = '@pnpm.e2e/tpd-cycle-a'
  const bName = '@pnpm.e2e/tpd-cycle-b'
  const cName = '@pnpm.e2e/tpd-cycle-c'
  const dName = '@pnpm.e2e/tpd-cycle-d'
  const hName = '@pnpm.e2e/tpd-cycle-h'

  const manifest: ProjectManifest = {
    name: 'root',
    dependencies: {
      [parentName]: '1.0.0',
    },
  }
  const allProjects: ProjectOptions[] = [{
    buildIndex: 0,
    manifest,
    rootDir: lockfileDir as ProjectRootDir,
  }]
  const options = {
    ...testDefaults(
      { allProjects, forceFullResolution: true },
      { retry: { retries: 0 } }
    ),
    lockfileDir,
    lockfileOnly: true,
  } satisfies MutateModulesOptions

  const installProjects: MutatedProject[] = [{
    mutation: 'install',
    rootDir: lockfileDir as ProjectRootDir,
  }]

  const registryUrl = options.registries.default.replace(/\/$/, '')

  function makeMeta (name: string, deps: Record<string, string>, peerDeps?: Record<string, string>, peerMeta?: Record<string, { optional?: boolean }>): PackageMeta {
    return {
      name,
      versions: {
        '1.0.0': {
          name,
          version: '1.0.0',
          dependencies: deps,
          ...(peerDeps ? { peerDependencies: peerDeps } : {}),
          ...(peerMeta ? { peerDependenciesMeta: peerMeta } : {}),
          dist: {
            shasum: '0000000000000000000000000000000000000000',
            tarball: `${options.registries.default}/${encodeURIComponent(name)}-1.0.0.tgz`,
          },
        },
      },
      'dist-tags': { latest: '1.0.0' },
    }
  }

  const metaByName = {
    [parentName]: makeMeta(parentName, { [aName]: '1.0.0', [hName]: '1.0.0' }),
    [aName]: makeMeta(aName, { [bName]: '1.0.0', [cName]: '1.0.0' }),
    [bName]: makeMeta(bName, { [aName]: '1.0.0' }),
    [cName]: makeMeta(cName, { [dName]: '1.0.0' }),
    [dName]: makeMeta(dName, {}, { e: '1.0.0' }, { e: { optional: true } }),
    [hName]: makeMeta(hName, { [aName]: '1.0.0' }),
  }

  await setupMockAgent()
  const agent = getMockAgent().get(registryUrl)
  for (const [name, meta] of Object.entries(metaByName)) {
    agent.intercept({ path: `/${name.replaceAll('/', '%2F')}`, method: 'GET' }).reply(200, meta).persist()
  }

  options.storeController.clearResolutionCache()
  await mutateModules(installProjects, options)

  const lockfile = rootProject.readLockfile()

  const snapshotsWithTpd = Object.entries(lockfile.snapshots ?? {})
    .filter(([, snapshot]) => snapshot.transitivePeerDependencies?.includes('e'))
    .map(([key]) => key)

  expect(snapshotsWithTpd).toContain(`${aName}@1.0.0`)
  expect(snapshotsWithTpd).toContain(`${hName}@1.0.0`)
  expect(snapshotsWithTpd).toContain(`${parentName}@1.0.0`)

  await teardownMockAgent()

  await setupMockAgent()
  const agent2 = getMockAgent().get(registryUrl)
  for (const [name, meta] of Object.entries(metaByName)) {
    agent2.intercept({ path: `/${name.replaceAll('/', '%2F')}`, method: 'GET' }).reply(200, meta).persist()
  }
  options.storeController.clearResolutionCache()
  await mutateModules(installProjects, options)

  const lockfile2 = rootProject.readLockfile()
  expect(Object.keys(lockfile2.snapshots ?? {})).toStrictEqual(Object.keys(lockfile.snapshots ?? {}))
})
