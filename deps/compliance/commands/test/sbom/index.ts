/// <reference path="../../../../../__typings__/index.d.ts" />
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

  const componentNames = parsed.components.map((c: { name: string }) => c.name)
  // lockfile-only mode: workspace deps are not resolved (no manifest reads)
  expect(componentNames).not.toContain('shared-lib')
  // External deps from the selected importer are still present
  expect(componentNames).toContain('is-positive')
})
