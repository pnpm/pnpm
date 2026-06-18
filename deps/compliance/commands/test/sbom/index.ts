/// <reference path="../../../../../__typings__/index.d.ts" />
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { sbom } from '@pnpm/deps.compliance.commands'
import { install } from '@pnpm/installing.commands'
import { tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

const f = fixtures(import.meta.dirname)

test('pnpm sbom --sbom-format cyclonedx', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.bomFormat).toBe('CycloneDX')
  expect(parsed.specVersion).toBe('1.7')
  expect(parsed.metadata.component.name).toBe('simple-sbom-test')
  expect(parsed.components.length).toBeGreaterThan(0)

  const isPositive = parsed.components.find(
    (c: { name: string }) => c.name === 'is-positive'
  )
  expect(isPositive).toBeDefined()
  expect(isPositive.purl).toBe('pkg:npm/is-positive@3.1.0')
  expect(isPositive.version).toBe('3.1.0')
})

test('pnpm sbom --sbom-format spdx', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'spdx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.spdxVersion).toBe('SPDX-2.3')
  expect(parsed.dataLicense).toBe('CC0-1.0')
  expect(parsed.packages.length).toBeGreaterThanOrEqual(2)

  const isPositive = parsed.packages.find(
    (p: { name: string }) => p.name === 'is-positive'
  )
  expect(isPositive).toBeDefined()
  expect(isPositive.externalRefs[0].referenceLocator).toBe('pkg:npm/is-positive@3.1.0')
})

test('pnpm sbom --lockfile-only', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  // No install — just lockfile
  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.bomFormat).toBe('CycloneDX')
  expect(parsed.components.length).toBeGreaterThan(0)

  const isPositive = parsed.components.find(
    (c: { name: string }) => c.name === 'is-positive'
  )
  expect(isPositive).toBeDefined()
  // In lockfile-only mode, license metadata is absent
  expect(isPositive.licenses).toBeUndefined()
})

test('pnpm sbom missing --sbom-format throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
    })
  ).rejects.toThrow('--sbom-format option is required')
})

test('pnpm sbom invalid --sbom-format throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'invalid',
    })
  ).rejects.toThrow('Invalid SBOM format')
})

test('pnpm sbom with missing lockfile throws', async () => {
  const workspaceDir = tempDir()

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'cyclonedx',
    })
  ).rejects.toThrow('Cannot generate SBOM without a lockfile')
})

test('pnpm sbom --prod excludes devDependencies', async () => {
  const workspaceDir = tempDir()
  f.copy('with-dev-dependency', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    production: true,
    dev: false,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  expect(componentNames).not.toContain('typescript')
})

test('pnpm sbom includes peer dependencies by default', async () => {
  const workspaceDir = tempDir()
  f.copy('with-peer-dependency', workspaceDir)

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  expect(componentNames).toContain('is-odd')
  expect(componentNames).toContain('is-number')
})

test('pnpm sbom --exclude-peers drops peers and their exclusive subtrees', async () => {
  const workspaceDir = tempDir()
  f.copy('with-peer-dependency', workspaceDir)

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
    excludePeers: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  // is-odd is the peer; is-number is only reachable through is-odd
  expect(componentNames).not.toContain('is-odd')
  expect(componentNames).not.toContain('is-number')

  // The dropped peer must not linger in the root's dependency graph either
  const rootRef = parsed.metadata.component['bom-ref']
  const rootDeps = parsed.dependencies.find((d: { ref: string }) => d.ref === rootRef)
  expect(rootDeps.dependsOn).not.toContain('pkg:npm/is-odd@3.0.1')
})

test('pnpm sbom --exclude-peers drops peers declared in workspace sub-packages', async () => {
  const workspaceDir = tempDir()
  f.copy('with-peer-workspace', workspaceDir)

  // No --filter, so no selectedProjectsGraph: every importer is walked. The
  // peer (is-odd) is declared in packages/pkg-a, not the directory pnpm runs in.
  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
    excludePeers: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  expect(componentNames).not.toContain('is-odd')
  expect(componentNames).not.toContain('is-number')
})

test('pnpm sbom --exclude-peers tolerates a malformed importer manifest', async () => {
  const workspaceDir = tempDir()
  f.copy('with-peer-workspace', workspaceDir)
  // Simulate an untrusted/broken importer: a sub-package with unparsable JSON.
  // The peer scan reads every importer manifest, and one bad file must not
  // abort the whole SBOM.
  fs.writeFileSync(path.join(workspaceDir, 'packages/pkg-a/package.json'), '{ not valid json')

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
    excludePeers: true,
  })

  expect(exitCode).toBe(0)
  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
})

test('pnpm sbom --exclude-peers keeps a package that is a peer in one importer and a real dep in another', async () => {
  const workspaceDir = tempDir()
  // pkg-a declares is-odd as a peer (excluded); pkg-b declares it as a real
  // dependency (kept). The importers must be walked independently, or excluding
  // pkg-a's peer would also drop pkg-b's real dependency.
  f.copy('with-peer-and-real-dep', workspaceDir)

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
    excludePeers: true,
  })

  expect(exitCode).toBe(0)
  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  // Present because pkg-b depends on it directly; only pkg-a's peer edge is cut.
  expect(componentNames).toContain('is-odd')
  expect(componentNames).toContain('is-number')
})

test('pnpm sbom --exclude-peers drops peers reached through a workspace link in a filtered run', async () => {
  const workspaceDir = tempDir()
  f.copy('with-peer-workspace-link', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const appADir = path.join(workspaceDir, 'packages/app-a')
  const filteredGraph = Object.fromEntries(
    Object.entries(selectedProjectsGraph).filter(([p]) => p === appADir)
  )

  // Filtered to app-a, which links peer-lib. peer-lib's auto-installed peer
  // (is-odd) is walked through that link, so its peer names must be filtered
  // too — not only the selected app-a's. The graph the peers are read from must
  // therefore cover the whole workspace, not just the selected subset.
  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: appADir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    selectedProjectsGraph: filteredGraph,
    allProjectsGraph,
    excludePeers: true,
  })

  expect(exitCode).toBe(0)
  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive') // app-a's own dependency
  expect(componentNames).toContain('peer-lib') // the linked workspace package
  expect(componentNames).not.toContain('is-odd') // peer-lib's peer
  expect(componentNames).not.toContain('is-number') // only reachable through is-odd
})

test('pnpm sbom marks dev-only components with scope "excluded" (cyclonedx)', async () => {
  const workspaceDir = tempDir()
  f.copy('with-dev-dependency', workspaceDir)

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const typescript = parsed.components.find((c: { name: string }) => c.name === 'typescript')
  expect(typescript.scope).toBe('excluded')

  // Prod components default to "required"; scope is omitted
  const isPositive = parsed.components.find((c: { name: string }) => c.name === 'is-positive')
  expect(isPositive.scope).toBeUndefined()
})

test('pnpm sbom invalid --sbom-type throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'cyclonedx',
      sbomType: 'invalid',
      lockfileOnly: true,
    })
  ).rejects.toThrow('Invalid SBOM type')
})

test('pnpm sbom --sbom-spec-version 1.6', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    sbomSpecVersion: '1.6',
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.specVersion).toBe('1.6')
  expect(parsed.$schema).toBe('http://cyclonedx.org/schema/bom-1.6.schema.json')
})

test('pnpm sbom invalid --sbom-spec-version throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'cyclonedx',
      sbomSpecVersion: '1.4',
      lockfileOnly: true,
    })
  ).rejects.toThrow('Invalid CycloneDX spec version')
})

test('pnpm sbom --sbom-spec-version with spdx format throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'spdx',
      sbomSpecVersion: '1.6',
      lockfileOnly: true,
    })
  ).rejects.toThrow('only supported with --sbom-format cyclonedx')
})

test('pnpm sbom --sbom-type application', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    sbomType: 'application',
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.metadata.component.type).toBe('application')
})

test('pnpm sbom --filter uses workspace manifest for root component', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const appADir = path.join(workspaceDir, 'app-a')
  const filteredGraph = Object.fromEntries(
    Object.entries(selectedProjectsGraph).filter(([p]) =>
      p === appADir
    )
  )

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: appADir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    selectedProjectsGraph: filteredGraph,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.metadata.component.name).toBe('app-a')
  expect(parsed.metadata.component.group).toBe('@test')
  expect(parsed.metadata.component.version).toBe('1.0.0')

  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  expect(componentNames).not.toContain('is-negative')

  // Workspace dep shared-lib and its transitive dep is-odd should be included
  expect(componentNames).toContain('shared-lib')
  expect(componentNames).toContain('is-odd')

  const sharedLib = parsed.components.find(
    (c: { name: string }) => c.name === 'shared-lib'
  )
  expect(sharedLib.version).toBe('0.1.0')
  expect(sharedLib.purl).toBe('pkg:npm/shared-lib@0.1.0')
})

test('pnpm sbom --filter with spdx uses workspace manifest for root component', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const appBDir = path.join(workspaceDir, 'app-b')
  const filteredGraph = Object.fromEntries(
    Object.entries(selectedProjectsGraph).filter(([p]) =>
      p === appBDir
    )
  )

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: appBDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'spdx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    selectedProjectsGraph: filteredGraph,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const rootPkg = parsed.packages.find(
    (p: { name: string }) => p.name === 'app-b'
  )
  expect(rootPkg).toBeDefined()
  expect(rootPkg.versionInfo).toBe('2.0.0')

  const componentNames = parsed.packages.map((p: { name: string }) => p.name)
  expect(componentNames).toContain('is-negative')
  expect(componentNames).not.toContain('is-positive')
})

test('pnpm sbom --prod excludes dev-only workspace dependencies', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom-dev', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const appDir = path.join(workspaceDir, 'app')
  const filteredGraph = Object.fromEntries(
    Object.entries(selectedProjectsGraph).filter(([p]) =>
      p === appDir
    )
  )

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: appDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    selectedProjectsGraph: filteredGraph,
    production: true,
    dev: false,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  // dev-tool is a devDependency workspace dep; excluded with --prod
  expect(componentNames).not.toContain('dev-tool')
  // is-negative is a transitive dep of dev-tool; also excluded
  expect(componentNames).not.toContain('is-negative')
})

test('pnpm sbom --filter includes dev workspace deps without --prod', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom-dev', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const appDir = path.join(workspaceDir, 'app')
  const filteredGraph = Object.fromEntries(
    Object.entries(selectedProjectsGraph).filter(([p]) =>
      p === appDir
    )
  )

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: appDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    selectedProjectsGraph: filteredGraph,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  expect(componentNames).toContain('is-positive')
  expect(componentNames).toContain('dev-tool')
  expect(componentNames).toContain('is-negative')
})

test('pnpm sbom --lockfile-only skips workspace dep resolution', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const appADir = path.join(workspaceDir, 'app-a')
  const filteredGraph = Object.fromEntries(
    Object.entries(selectedProjectsGraph).filter(([p]) =>
      p === appADir
    )
  )

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: appADir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    selectedProjectsGraph: filteredGraph,
    lockfileOnly: true,
  })

  expect(exitCode).toBe(0)

  const parsed = JSON.parse(output)
  expect(parsed.metadata.component.name).toBe('app-a')
  expect(parsed.metadata.component.group).toBe('@test')

  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  // lockfile-only mode: workspace deps are not resolved (no manifest reads)
  expect(componentNames).not.toContain('shared-lib')
  // Transitive deps reachable only through workspace links are not traversed either
  expect(componentNames).not.toContain('is-odd')
  // External deps from the selected importer are still present
  expect(componentNames).toContain('is-positive')
})

test('pnpm sbom --out writes single file', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const outFile = path.join(workspaceDir, 'output', 'sbom.cdx.json')

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    out: outFile,
  })

  expect(exitCode).toBe(0)
  expect(output).toBe(outFile)
  expect(fs.existsSync(outFile)).toBe(true)

  const parsed = JSON.parse(fs.readFileSync(outFile, 'utf8'))
  expect(parsed.bomFormat).toBe('CycloneDX')
  expect(parsed.metadata.component.name).toBe('simple-sbom-test')
})

test('pnpm sbom --out with %s in a single-project repo writes one file', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
  })

  const outFile = path.join(workspaceDir, 'sbom-out', '%s.cdx.json')

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    out: outFile,
  })

  expect(exitCode).toBe(0)
  const expectedFile = path.join(workspaceDir, 'sbom-out', 'simple-sbom-test.cdx.json')
  expect(output).toBe(expectedFile)
  expect(fs.existsSync(expectedFile)).toBe(true)
})

test('pnpm sbom --out with %s writes per-package files', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const outDir = path.join(workspaceDir, 'sbom-out')
  const outPattern = path.join(outDir, '%s.cdx.json')

  const { exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    out: outPattern,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  expect(exitCode).toBe(0)

  const files = fs.readdirSync(outDir).sort()
  expect(files).toContain('test-app-a.cdx.json')
  expect(files).toContain('app-b.cdx.json')
  expect(files).toContain('shared-lib.cdx.json')

  const appA = JSON.parse(fs.readFileSync(path.join(outDir, 'test-app-a.cdx.json'), 'utf8'))
  expect(appA.metadata.component.name).toBe('app-a')
  expect(appA.metadata.component.group).toBe('@test')
  expect(appA.metadata.component.version).toBe('1.0.0')

  const appB = JSON.parse(fs.readFileSync(path.join(outDir, 'app-b.cdx.json'), 'utf8'))
  expect(appB.metadata.component.name).toBe('app-b')
})

test('pnpm sbom --split outputs NDJSON to stdout', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const { output, exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    split: true,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  expect(exitCode).toBe(0)

  const lines = output.trim().split('\n')
  expect(lines).toHaveLength(4)

  const names = lines.map((line) => {
    const component = JSON.parse(line).metadata.component
    return component.group ? `${component.group}/${component.name}` : component.name
  }).sort()
  expect(names).toContain('@test/app-a')
  expect(names).toContain('app-b')
  expect(names).toContain('shared-lib')

  for (const line of lines) {
    expect(line).toBe(line.trim())
    expect(line.startsWith('{')).toBe(true)
    JSON.parse(line)
  }
})

test('pnpm sbom --out with %s and %v uses name and version in filename', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom', workspaceDir)

  const { allProjects, allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  const storeDir = path.join(workspaceDir, 'store')
  await install.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    storeDir,
    allProjects,
    allProjectsGraph,
    selectedProjectsGraph,
  })

  const outDir = path.join(workspaceDir, 'sbom-out')
  const outPattern = path.join(outDir, '%s-%v.cdx.json')

  const appADir = path.join(workspaceDir, 'app-a')
  const filteredGraph = Object.fromEntries(
    Object.entries(selectedProjectsGraph).filter(([p]) =>
      p === appADir
    )
  )

  const { exitCode } = await sbom.handler({
    ...DEFAULT_OPTS,
    dir: workspaceDir,
    lockfileDir: workspaceDir,
    pnpmHomeDir: '',
    sbomFormat: 'cyclonedx',
    storeDir: path.resolve(storeDir, STORE_VERSION),
    out: outPattern,
    selectedProjectsGraph: filteredGraph,
  })

  expect(exitCode).toBe(0)

  const files = fs.readdirSync(outDir)
  expect(files).toContain('test-app-a-1.0.0.cdx.json')
})

test('pnpm sbom --split without workspace throws', async () => {
  const workspaceDir = tempDir()
  f.copy('simple-sbom', workspaceDir)

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'cyclonedx',
      split: true,
      lockfileOnly: true,
    })
  ).rejects.toThrow('requires a workspace')
})

test('pnpm sbom --split --out without %s throws', async () => {
  const workspaceDir = tempDir()
  f.copy('workspace-sbom', workspaceDir)

  const { allProjectsGraph, selectedProjectsGraph } =
    await filterProjectsBySelectorObjectsFromDir(workspaceDir, [])

  await expect(
    sbom.handler({
      ...DEFAULT_OPTS,
      dir: workspaceDir,
      lockfileDir: workspaceDir,
      pnpmHomeDir: '',
      sbomFormat: 'cyclonedx',
      split: true,
      out: 'sbom.cdx.json',
      allProjectsGraph,
      selectedProjectsGraph,
      lockfileOnly: true,
    })
  ).rejects.toThrow('must contain %s')
})
