import { writeFileSync } from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { type MutatedProject, mutateModules, mutateModulesInSingleProject, type ProjectOptions } from '@pnpm/installing.deps-installer'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

test('workspace packages should maintain link: protocol after single-project pnpm rm with injectWorkspacePackages', async () => {
  const projectAManifest: { name: string, version: string, dependencies: Record<string, string> } = {
    name: 'a',
    version: '1.0.0',
    dependencies: {
      'b': 'workspace:*',
      'is-positive': '1.0.0',
    },
  }
  const projectBManifest = {
    name: 'b',
    version: '1.0.0',
  }

  preparePackages([
    {
      location: 'a',
      package: projectAManifest,
    },
    {
      location: 'b',
      package: projectBManifest,
    },
  ])

  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: projectAManifest,
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: projectBManifest,
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ]

  // Initial full install with all projects
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects,
    injectWorkspacePackages: true,
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  expect(lockfile.importers.a.dependencies!.b.version).toBe('link:../b')

  // Remove a dependency using mutateModulesInSingleProject.
  // This is the code path used by `pnpm rm` when run from within a single
  // workspace package directory. It passes allProjects with only the single
  // project, so ctx.projects won't contain the other workspace packages.
  // The workspacePackages map must still include all workspace packages
  // for resolution to work.
  delete projectAManifest.dependencies['is-positive']
  const workspacePackages = new Map([
    ['a', new Map([
      ['1.0.0', {
        rootDir: path.resolve('a') as ProjectRootDir,
        manifest: projectAManifest,
      }],
    ])],
    ['b', new Map([
      ['1.0.0', {
        rootDir: path.resolve('b') as ProjectRootDir,
        manifest: projectBManifest,
      }],
    ])],
  ])
  await mutateModulesInSingleProject(
    {
      binsDir: path.resolve('a', 'node_modules', '.bin'),
      dependencyNames: ['is-positive'],
      manifest: projectAManifest,
      mutation: 'uninstallSome',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    testDefaults({
      workspacePackages,
      injectWorkspacePackages: true,
    })
  )

  const lockfileAfterRm = rootModules.readLockfile()

  // Without the fix, workspace dep 'b' would switch from link: to file: protocol
  // because dedupeInjectedDeps couldn't identify 'b' as a workspace package
  // when only package 'a' was in the projects list.
  expect(lockfileAfterRm.importers.a.dependencies!.b.version).toBe('link:../b')
})

test('workspace packages with their own dependencies should maintain link: protocol after single-project pnpm rm with injectWorkspacePackages', async () => {
  const projectAManifest: { name: string, version: string, dependencies: Record<string, string> } = {
    name: 'a',
    version: '1.0.0',
    dependencies: {
      'b': 'workspace:*',
      'is-positive': '1.0.0',
    },
  }
  const projectBManifest = {
    name: 'b',
    version: '1.0.0',
    dependencies: {
      'is-negative': '1.0.0',
    },
  }

  preparePackages([
    {
      location: 'a',
      package: projectAManifest,
    },
    {
      location: 'b',
      package: projectBManifest,
    },
  ])

  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: projectAManifest,
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: projectBManifest,
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ]

  // Initial full install with all projects
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects,
    injectWorkspacePackages: true,
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  expect(lockfile.importers.a.dependencies!.b.version).toBe('link:../b')

  // Same single-project rm path as the test above, but `b` has its own dependency
  // (`is-negative`). The injected file: dep then has children, which hits a separate
  // branch in dedupeInjectedDeps.
  delete projectAManifest.dependencies['is-positive']
  const workspacePackages = new Map([
    ['a', new Map([
      ['1.0.0', {
        rootDir: path.resolve('a') as ProjectRootDir,
        manifest: projectAManifest,
      }],
    ])],
    ['b', new Map([
      ['1.0.0', {
        rootDir: path.resolve('b') as ProjectRootDir,
        manifest: projectBManifest,
      }],
    ])],
  ])
  await mutateModulesInSingleProject(
    {
      binsDir: path.resolve('a', 'node_modules', '.bin'),
      dependencyNames: ['is-positive'],
      manifest: projectAManifest,
      mutation: 'uninstallSome',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    testDefaults({
      workspacePackages,
      injectWorkspacePackages: true,
    })
  )

  const lockfileAfterRm = rootModules.readLockfile()

  // Without the fix, dedupeInjectedDeps would skip dedupe when the injected dep had
  // children and the target workspace project wasn't in the current resolution, so
  // workspace dep 'b' would switch from link: to file:.
  expect(lockfileAfterRm.importers.a.dependencies!.b.version).toBe('link:../b')
})

test('peer-resolved workspace packages should keep their file: protocol after single-project pnpm rm with injectWorkspacePackages', async () => {
  const projectAManifest: { name: string, version: string, dependencies: Record<string, string> } = {
    name: 'a',
    version: '1.0.0',
    dependencies: {
      'b': 'workspace:*',
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }
  const projectBManifest: { name: string, version: string, dependencies: Record<string, string>, peerDependencies: Record<string, string> } = {
    name: 'b',
    version: '1.0.0',
    dependencies: {},
    peerDependencies: {
      'is-positive': '>=1.0.0',
    },
  }

  preparePackages([
    {
      location: 'a',
      package: projectAManifest,
    },
    {
      location: 'b',
      package: projectBManifest,
    },
  ])

  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: projectAManifest,
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: projectBManifest,
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ]

  // Initial full install with all projects
  await mutateModules([
    {
      mutation: 'install',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('b') as ProjectRootDir,
    },
  ], testDefaults({
    allProjects,
    injectWorkspacePackages: true,
    autoInstallPeers: false,
  }))

  const rootModules = assertProject(process.cwd())
  const lockfile = rootModules.readLockfile()
  // With a peer dep on `b`, the injected resolution depends on `a`'s peer context, so the
  // entry stays in file: form rather than collapsing to link:../b.
  const initialVersion = lockfile.importers.a.dependencies!.b.version
  expect(initialVersion).not.toBe('link:../b')
  expect(initialVersion.startsWith('file:')).toBe(true)

  // Single-project rm of an unrelated dep should preserve the peer-resolved file: form.
  delete projectAManifest.dependencies['is-negative']
  const workspacePackages = new Map([
    ['a', new Map([
      ['1.0.0', {
        rootDir: path.resolve('a') as ProjectRootDir,
        manifest: projectAManifest,
      }],
    ])],
    ['b', new Map([
      ['1.0.0', {
        rootDir: path.resolve('b') as ProjectRootDir,
        manifest: projectBManifest,
      }],
    ])],
  ])
  await mutateModulesInSingleProject(
    {
      binsDir: path.resolve('a', 'node_modules', '.bin'),
      dependencyNames: ['is-negative'],
      manifest: projectAManifest,
      mutation: 'uninstallSome',
      rootDir: path.resolve('a') as ProjectRootDir,
    },
    testDefaults({
      workspacePackages,
      injectWorkspacePackages: true,
      autoInstallPeers: false,
    })
  )

  const lockfileAfterRm = rootModules.readLockfile()

  // The fast-path must skip dedupe for peer-suffixed depPaths. Without the peer-suffix
  // check, dedupeInjectedDeps would collapse the peer-resolved file: entry to link:../b
  // and lose the importer's peer context.
  expect(lockfileAfterRm.importers.a.dependencies!.b.version).toBe(initialVersion)
})

// Regression test for https://github.com/pnpm/pnpm/issues/10433: updating one
// dependency must not rewrite an untouched workspace dependency's `link:` entry
// to a peer-suffixed `file:`.
//
// `d` peer-depends on `a` and `b`; `b` has an optional `winston` peer. On the
// initial install `consumer` -> `d` dedupes to `link:../d`. Updating the
// unrelated `winston` re-resolves the untouched injected workspace deps on a
// path dedupeInjectedDeps doesn't reach and, without the fix, flips
// `consumer` -> `d` to `file:d(a@file:a)(b@file:b(a@file:a)(winston@3.19.0))`.
//
// The update is driven exactly like `pnpm update winston@3.19.0 --recursive`
// lowers it (see installing/commands/src/recursive.ts): the importer whose
// manifest has `winston` gets `installSome` with the selector, every other
// importer gets `installSome` with no selectors and `update: true`.
test('injected workspace dependency keeps link: when an unrelated dependency is updated', async () => {
  const pkgA = { name: 'a', version: '1.0.0' }
  const pkgB = {
    name: 'b',
    version: '1.0.0',
    dependencies: { bluebird: '^3.7.2', 'c': 'workspace:^' },
    peerDependencies: { winston: '^3' },
    peerDependenciesMeta: { winston: { optional: true } },
  }
  const pkgC = {
    name: 'c',
    version: '1.0.0',
    peerDependencies: { 'a': '>=1.0.0' },
    dependencies: { debug: '^4.4.3' },
  }
  const pkgD = {
    name: 'd',
    version: '1.0.0',
    peerDependencies: { 'a': '*', 'b': '^1.0.0' },
    dependencies: { debug: '^4.4.3' },
  }
  const consumerManifest = {
    name: 'consumer',
    version: '1.0.0',
    dependencies: { 'd': 'workspace:*', 'a': 'workspace:*' },
  }
  const rootManifest = {
    name: 'root',
    version: '0.0.0',
    devDependencies: { winston: '3.17.0' },
  }

  preparePackages([
    { location: 'a', package: pkgA },
    { location: 'b', package: pkgB },
    { location: 'c', package: pkgC },
    { location: 'd', package: pkgD },
    { location: 'consumer', package: consumerManifest },
  ])
  // The workspace root carries the dependency that will be updated.
  writeFileSync('package.json', `${JSON.stringify(rootManifest, null, 2)}\n`)

  const allProjects: ProjectOptions[] = [
    { buildIndex: 0, manifest: rootManifest, rootDir: path.resolve('.') as ProjectRootDir },
    { buildIndex: 0, manifest: pkgA, rootDir: path.resolve('a') as ProjectRootDir },
    { buildIndex: 0, manifest: pkgB, rootDir: path.resolve('b') as ProjectRootDir },
    { buildIndex: 0, manifest: pkgC, rootDir: path.resolve('c') as ProjectRootDir },
    { buildIndex: 0, manifest: pkgD, rootDir: path.resolve('d') as ProjectRootDir },
    { buildIndex: 0, manifest: consumerManifest, rootDir: path.resolve('consumer') as ProjectRootDir },
  ]

  const sharedOpts = {
    allProjects,
    injectWorkspacePackages: true,
    linkWorkspacePackages: 'deep' as const,
    preferWorkspacePackages: true,
    dedupeInjectedDeps: true,
    dedupePeerDependents: true,
  }

  // Initial install: `consumer` -> `d` dedupes to link:../d.
  await mutateModules(
    allProjects.map(({ rootDir }) => ({ mutation: 'install' as const, rootDir })),
    testDefaults(sharedOpts)
  )

  const rootModules = assertProject(process.cwd())
  expect(rootModules.readLockfile().importers.consumer.dependencies!.d.version).toBe('link:../d')

  // `pnpm update winston@3.19.0 --recursive`, as lowered by
  // installing/commands/src/recursive.ts.
  const updateMutations: MutatedProject[] = allProjects.map(({ rootDir }) =>
    rootDir === path.resolve('.')
      ? {
        allowNew: false,
        dependencySelectors: ['winston@3.19.0'],
        mutation: 'installSome' as const,
        rootDir,
        update: true,
        updatePackageManifest: true,
      } as MutatedProject
      : {
        allowNew: false,
        dependencySelectors: [],
        mutation: 'installSome' as const,
        rootDir,
        update: true,
        updatePackageManifest: true,
      } as MutatedProject
  )
  await mutateModules(updateMutations, testDefaults({
    ...sharedOpts,
    depth: Infinity,
  }))

  const lockfileAfterUpdate = rootModules.readLockfile()
  // The update itself must still happen.
  expect(lockfileAfterUpdate.importers['.'].devDependencies!.winston.version).toBe('3.19.0')
  // The untouched workspace dependency must keep its link:.
  expect(lockfileAfterUpdate.importers.consumer.dependencies!.d.version).toBe('link:../d')
})

// Same protection on the plain `pnpm install` path (pnpm/pnpm#10433), for the
// genuine peer-context divergence that dedupeInjectedDeps rightly refuses to
// collapse. `q` peer-depends `@pnpm.e2e/foo`; `n` depends on `q` but does not
// pin `foo` itself, so `q`'s peer is provided by an ancestor. Initially only
// `foo@100.0.0` exists (via the root), so `q`'s peer resolves to it in both
// `n`'s own context and `n`'s injected-under-consumer context — the occurrences
// match and `consumer` -> `n` dedupes to `link:`.
//
// Then `foo@100.1.0` is added to the consumer and a plain install is run. Now
// the injected `q` under the consumer resolves its `foo` peer to the consumer's
// `100.1.0`, while `n`'s own context still resolves it to the root's `100.0.0`
// — a real divergence, so dedupeInjectedDeps correctly keeps the injected copy
// `file:`. The `consumer` -> `n` importer entry itself was not targeted by the
// change and must be preserved as `link:` rather than rewritten to a
// peer-suffixed `file:`.
//
// This is exactly the case the criterion depends on: a plain install marks
// every consumer manifest dependency (including `n`) with `updateSpec: true`,
// which must NOT count as targeting `n`.
test('injected workspace dependency keeps link: on a plain install when a consumer change makes an ancestor-provided peer diverge', async () => {
  const pkgQ = {
    name: 'q',
    version: '1.0.0',
    peerDependencies: { '@pnpm.e2e/foo': '*' },
  }
  const pkgN = {
    name: 'n',
    version: '1.0.0',
    dependencies: { 'q': 'workspace:^' },
  }
  const consumerManifest: { name: string, version: string, dependencies: Record<string, string> } = {
    name: 'consumer',
    version: '1.0.0',
    dependencies: { 'n': 'workspace:*' },
  }
  const rootManifest = {
    name: 'root',
    version: '0.0.0',
    dependencies: { '@pnpm.e2e/foo': '100.0.0' },
  }

  preparePackages([
    { location: 'q', package: pkgQ },
    { location: 'n', package: pkgN },
    { location: 'consumer', package: consumerManifest },
  ])
  writeFileSync('package.json', `${JSON.stringify(rootManifest, null, 2)}\n`)

  const allProjects: ProjectOptions[] = [
    { buildIndex: 0, manifest: rootManifest, rootDir: path.resolve('.') as ProjectRootDir },
    { buildIndex: 0, manifest: pkgQ, rootDir: path.resolve('q') as ProjectRootDir },
    { buildIndex: 0, manifest: pkgN, rootDir: path.resolve('n') as ProjectRootDir },
    { buildIndex: 0, manifest: consumerManifest, rootDir: path.resolve('consumer') as ProjectRootDir },
  ]

  const sharedOpts = {
    allProjects,
    injectWorkspacePackages: true,
    linkWorkspacePackages: 'deep' as const,
    preferWorkspacePackages: true,
    dedupeInjectedDeps: true,
    dedupePeerDependents: true,
  }
  const installMutations = allProjects.map(({ rootDir }) => ({ mutation: 'install' as const, rootDir }))

  // Initial install: peers match at 100.0.0, so consumer -> n dedupes to link:.
  await mutateModules(installMutations, testDefaults(sharedOpts))

  const rootModules = assertProject(process.cwd())
  expect(rootModules.readLockfile().importers.consumer.dependencies!.n.version).toBe('link:../n')

  // Add a competing foo@100.1.0 to the consumer and run a plain install. n
  // itself is untouched.
  consumerManifest.dependencies['@pnpm.e2e/foo'] = '100.1.0'
  const allProjectsAfter = allProjects.map((p) =>
    p.rootDir === path.resolve('consumer') ? { ...p, manifest: consumerManifest } : p
  )
  await mutateModules(installMutations, testDefaults({ ...sharedOpts, allProjects: allProjectsAfter }))

  const lockfileAfterInstall = rootModules.readLockfile()
  // The consumer change itself must be reflected.
  expect(lockfileAfterInstall.importers.consumer.dependencies!['@pnpm.e2e/foo'].version).toBe('100.1.0')
  // The peer context genuinely diverged: both foo versions resolved — the
  // consumer's direct 100.1.0 (what q's peer resolves to in n's injected
  // context) and the root-provided 100.0.0 (what q's peer resolves to in n's
  // own context). Without both present there would be no divergence for
  // dedupeInjectedDeps to keep as file:, and the link: assertion below would
  // pass vacuously.
  const fooVersions = Object.keys(lockfileAfterInstall.packages ?? {})
    .filter((key) => key.startsWith('@pnpm.e2e/foo@'))
    .sort()
  expect(fooVersions).toStrictEqual(['@pnpm.e2e/foo@100.0.0', '@pnpm.e2e/foo@100.1.0'])
  // The untouched workspace dependency must keep its link:.
  expect(lockfileAfterInstall.importers.consumer.dependencies!.n.version).toBe('link:../n')
})
