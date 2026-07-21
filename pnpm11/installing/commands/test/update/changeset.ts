import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { loadJsonFileSync } from 'load-json-file'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { DEFAULT_OPTS } from '../utils/index.js'

const originalModule = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => {
  return {
    ...originalModule,
    globalInfo: jest.fn(),
    globalWarn: jest.fn(),
  }
})

const { globalInfo, globalWarn } = await import('@pnpm/logger')
const { install, update } = await import('@pnpm/installing.commands')
const { captureUpdateChangesetContext, generateUpdateChangeset } = await import('../../src/update/generateUpdateChangeset.js')

beforeEach(() => {
  jest.mocked(globalInfo).mockClear()
  jest.mocked(globalWarn).mockClear()
})

function writeChangesetConfig (config: { ignore?: string[] } = {}): void {
  fs.mkdirSync('.changeset', { recursive: true })
  fs.writeFileSync(path.join('.changeset', 'config.json'), JSON.stringify(config))
}

function readGeneratedChangesets (): string[] {
  if (!fs.existsSync('.changeset')) return []
  return fs.readdirSync('.changeset').filter((fileName) => fileName.startsWith('pnpm-update-') && fileName.endsWith('.md'))
}

test('update --changeset generates a changeset for packages whose production dependencies changed', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': '^100.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      devDependencies: {
        '@pnpm.e2e/bar': '^100.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      private: true,

      dependencies: {
        '@pnpm.e2e/foo': '^100.0.0',
      },
    },
    {
      name: 'ignored-project',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': '^100.0.0',
      },
    },
  ])
  writeChangesetConfig({ ignore: ['ignored-*'] })

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    changeset: true,
    dir: process.cwd(),
    latest: true,
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  // The update did change project-2's manifest, but only its devDependencies.
  expect(loadJsonFileSync('project-2/package.json')).toHaveProperty(['devDependencies', '@pnpm.e2e/bar'], '^100.1.0')

  const changesetFiles = readGeneratedChangesets()
  expect(changesetFiles).toHaveLength(1)
  expect(fs.readFileSync(path.join('.changeset', changesetFiles[0]), 'utf8')).toBe(`---
"project-1": patch
---

Update dependencies.
`)
  expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('Generated a changeset'))
})

test('update --changeset generates a major changeset when peer dependencies change', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  preparePackages([
    {
      name: 'project',
      version: '1.0.0',
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
      peerDependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**'],
    catalog: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })
  writeChangesetConfig()
  const catalogs = { default: { '@pnpm.e2e/foo': '^100.0.0' } }
  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    catalogs,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    catalogs,
    changeset: true,
    dir: process.cwd(),
    latest: true,
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(readYamlFileSync('pnpm-workspace.yaml')).toHaveProperty(['catalog', '@pnpm.e2e/foo'], '^100.1.0')
  const changesetFiles = readGeneratedChangesets()
  expect(changesetFiles).toHaveLength(1)
  expect(fs.readFileSync(path.join('.changeset', changesetFiles[0]), 'utf8')).toBe(`---
"project": major
---

Update dependencies.
`)
})

test('generated changesets escape package names in frontmatter', async () => {
  prepare({
    name: 'project',
    version: '1.0.0',
    dependencies: {
      foo: '1.0.0',
    },
  })
  writeChangesetConfig()
  const ctx = await captureUpdateChangesetContext({ dir: process.cwd() })
  const packageName = 'project"\\\n"injected'
  fs.writeFileSync('package.json', JSON.stringify({
    name: packageName,
    version: '1.0.0',
    dependencies: {
      foo: '2.0.0',
    },
  }))

  await generateUpdateChangeset(ctx)

  const changesetFiles = readGeneratedChangesets()
  expect(changesetFiles).toHaveLength(1)
  expect(fs.readFileSync(path.join('.changeset', changesetFiles[0]), 'utf8')).toBe(`---
${JSON.stringify(packageName)}: patch
---

Update dependencies.
`)
})

test('update --changeset generates no changeset when only devDependencies changed', async () => {
  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      devDependencies: {
        '@pnpm.e2e/bar': '^100.0.0',
      },
    },
  ])
  writeChangesetConfig()

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    changeset: true,
    dir: process.cwd(),
    latest: true,
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(loadJsonFileSync('project-1/package.json')).toHaveProperty(['devDependencies', '@pnpm.e2e/bar'], '^100.1.0')
  expect(readGeneratedChangesets()).toHaveLength(0)
  expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('No changeset was generated'))
})

test('update --changeset generates a changeset for every consumer of an updated catalog entry, even outside the selection', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**'],
    catalog: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })
  writeChangesetConfig()
  const catalogs = { default: { '@pnpm.e2e/foo': '^100.0.0' } }

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    catalogs,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  const filtered = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [{ namePattern: 'project-1' }])
  await update.handler({
    ...DEFAULT_OPTS,
    allProjects: filtered.allProjects,
    catalogs,
    changeset: true,
    dir: process.cwd(),
    latest: true,
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph: filtered.selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(readYamlFileSync('pnpm-workspace.yaml')).toHaveProperty(['catalog', '@pnpm.e2e/foo'], '^100.1.0')
  // The manifests of the catalog consumers are unchanged. Only the catalog
  // diff reveals that their production dependencies changed.
  expect(loadJsonFileSync('project-2/package.json')).toHaveProperty(['dependencies', '@pnpm.e2e/foo'], 'catalog:')

  const changesetFiles = readGeneratedChangesets()
  expect(changesetFiles).toHaveLength(1)
  expect(fs.readFileSync(path.join('.changeset', changesetFiles[0]), 'utf8')).toBe(`---
"project-1": patch
"project-2": patch
---

Update dependencies.
`)
})

test('update --changeset generates no changeset when a catalog entry re-resolves in the lockfile without a spec change', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**'],
    catalog: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })
  writeChangesetConfig()
  const catalogs = { default: { '@pnpm.e2e/foo': '^100.0.0' } }

  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    catalogs,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })
  expect(readYamlFileSync('pnpm-lock.yaml')).toHaveProperty(['catalogs', 'default', '@pnpm.e2e/foo', 'version'], '100.0.0')

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    allProjects,
    catalogs,
    changeset: true,
    dir: process.cwd(),
    lockfileDir: process.cwd(),
    recursive: true,
    save: false,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  // The resolution moved within the unchanged ^100.0.0 spec.
  expect(readYamlFileSync('pnpm-lock.yaml')).toHaveProperty(['catalogs', 'default', '@pnpm.e2e/foo', 'version'], '100.1.0')
  expect(readYamlFileSync('pnpm-workspace.yaml')).toHaveProperty(['catalog', '@pnpm.e2e/foo'], '^100.0.0')
  expect(readGeneratedChangesets()).toHaveLength(0)
  expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('No changeset was generated'))
})

test('update --changeset warns and skips generation when .changeset/config.json is missing', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    changeset: true,
    dir: process.cwd(),
    latest: true,
  })

  expect(loadJsonFileSync('package.json')).toHaveProperty(['dependencies', '@pnpm.e2e/foo'], '^100.1.0')
  expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining(path.join('.changeset', 'config.json')))
  expect(fs.existsSync('.changeset')).toBe(false)
})

test('updateConfig.changeset enables changeset generation by default', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })
  writeChangesetConfig()

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    updateConfig: { changeset: true },
  })

  expect(readGeneratedChangesets()).toHaveLength(1)
})

test('--no-changeset overrides updateConfig.changeset', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })
  writeChangesetConfig()

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    changeset: false,
    dir: process.cwd(),
    latest: true,
    updateConfig: { changeset: true },
  })

  expect(readGeneratedChangesets()).toHaveLength(0)
})

test('update --changeset reports malformed changeset config with a stable error', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  fs.mkdirSync('.changeset')
  fs.writeFileSync(path.join('.changeset', 'config.json'), '{')

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await expect(update.handler({
    ...DEFAULT_OPTS,
    changeset: true,
    dir: process.cwd(),
    latest: true,
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_CHANGESET_CONFIG',
    message: expect.stringContaining(path.join('.changeset', 'config.json')),
  })
})

test('update --changeset refuses a symlinked changeset directory', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const outsideChangesetDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-update-changeset-'))
  fs.writeFileSync(path.join(outsideChangesetDir, 'config.json'), '{}')
  fs.symlinkSync(outsideChangesetDir, '.changeset', process.platform === 'win32' ? 'junction' : 'dir')

  try {
    await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

    await expect(update.handler({
      ...DEFAULT_OPTS,
      changeset: true,
      dir: process.cwd(),
      latest: true,
    })).rejects.toMatchObject({ code: 'ERR_PNPM_UNSAFE_CHANGESET_DIR' })
    expect(fs.readdirSync(outsideChangesetDir)).toEqual(['config.json'])
  } finally {
    fs.rmSync(outsideChangesetDir, { force: true, recursive: true })
  }
})

test('update --changeset generates a changeset for a single package outside a workspace', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  prepare({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })
  writeChangesetConfig()

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await update.handler({
    ...DEFAULT_OPTS,
    changeset: true,
    dir: process.cwd(),
    latest: true,
  })

  const changesetFiles = readGeneratedChangesets()
  expect(changesetFiles).toHaveLength(1)
  expect(fs.readFileSync(path.join('.changeset', changesetFiles[0]), 'utf8')).toBe(`---
"project": patch
---

Update dependencies.
`)
})
