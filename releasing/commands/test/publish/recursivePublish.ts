import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { streamParser } from '@pnpm/logger'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { publish } from '@pnpm/releasing.commands'
import type { ProjectManifest } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import crossSpawn from 'cross-spawn'
import { safeExeca as execa } from 'execa'
import { loadJsonFileSync } from 'load-json-file'

import { checkPkgExists, DEFAULT_OPTS } from './utils/index.js'

const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    creds: {
      basicAuth: REGISTRY_MOCK_CREDENTIALS,
    },
  },
}

test('recursive publish', async () => {
  // This suffix is added to the package name to avoid issue if Jest reruns the test
  const SUFFIX = Date.now()

  const pkg1 = {
    name: `@pnpmtest/test-recursive-publish-project-1-${SUFFIX}`,
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  const pkg2 = {
    name: `@pnpmtest/test-recursive-publish-project-2-${SUFFIX}`,
    version: '1.0.0',

    dependencies: {
      'is-negative': '1.0.0',
    },
  }
  const projects = preparePackages([
    pkg1,
    pkg2,
    // This will not be published because is-positive@1.0.0 is in the registry
    {
      name: 'is-positive',
      version: '1.0.0',

      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // This will not be published because it is a private package
    {
      name: 'i-am-private',
      version: '1.0.0',

      private: true,
      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // Package with no name is skipped
    {
      location: 'no-name',
      package: {
        scripts: {
          prepublishOnly: 'exit 1',
        },
      },
    },
  ])

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    dryRun: true,
    recursive: true,
  }, [])

  {
    const { status } = crossSpawn.sync('pnpm', ['view', pkg1.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(status).toBe(1)
  }
  {
    const { status } = crossSpawn.sync('pnpm', ['view', pkg2.name, 'versions', '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`, '--json'])
    expect(status).toBe(1)
  }

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    recursive: true,
  }, [])

  await checkPkgExists(pkg1.name, pkg1.version)
  await checkPkgExists(pkg2.name, pkg2.version)

  projects[pkg1.name].writePackageJson({ ...pkg1, version: '2.0.0' })

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    recursive: true,
    tag: 'next',
  }, [])

  {
    const { stdout } = await execa('pnpm', ['dist-tag', 'ls', pkg1.name, '--registry', `http://localhost:${REGISTRY_MOCK_PORT}`])
    expect(stdout?.toString()).toContain('next: 2.0.0')
  }
})

test('print info when no packages are published', async () => {
  preparePackages([
    // This will not be published because is-positive@1.0.0 is in the registry
    {
      name: 'is-positive',
      version: '1.0.0',

      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // This will not be published because it is a private package
    {
      name: 'i-am-private',
      version: '1.0.0',

      private: true,
      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // Package with no name is skipped
    {
      location: 'no-name',
      package: {
        scripts: {
          prepublishOnly: 'exit 1',
        },
      },
    },
  ])

  const reporter = jest.fn()
  streamParser.on('data', reporter)

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    dryRun: true,
    recursive: true,
  }, [])

  streamParser.removeListener('data', reporter)
  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    level: 'info',
    message: 'There are no new packages that should be published',
    name: 'pnpm',
    prefix: process.cwd(),
  }))
})

test('packages are released even if their current version is published, when force=true', async () => {
  preparePackages([
    // This version is already in the registry
    {
      name: 'is-positive',
      version: '3.1.0',

      scripts: {
        prepublishOnly: 'npm version major',
      },
    },
  ])

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    force: true,
    dir: process.cwd(),
    dryRun: true,
    recursive: true,
  }, [])

  const manifest = loadJsonFileSync<ProjectManifest>('is-positive/package.json')
  expect(manifest.version).toBe('4.0.0')
})

test('recursive publish writes publish summary', async () => {
  preparePackages([
    {
      name: '@pnpmtest/test-recursive-publish-project-3',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: '@pnpmtest/test-recursive-publish-project-4',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    // This will not be published because is-positive@1.0.0 is in the registry
    {
      name: 'is-positive',
      version: '1.0.0',

      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // This will not be published because it is a private package
    {
      name: 'i-am-private',
      version: '1.0.0',

      private: true,
      scripts: {
        prepublishOnly: 'exit 1',
      },
    },
    // Package with no name is skipped
    {
      location: 'no-name',
      package: {
        scripts: {
          prepublishOnly: 'exit 1',
        },
      },
    },
  ])

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    recursive: true,
    reportSummary: true,
  }, [])

  {
    const publishSummary = loadJsonFileSync<{ publishedPackages: Array<{ name: string, version: string }> }>('pnpm-publish-summary.json')
    // Each entry is now a full PublishSummary; assert by name/version pair to avoid coupling to
    // hash/size values that vary per pnpm version.
    const namesAndVersions = publishSummary.publishedPackages
      .map(({ name, version }) => ({ name, version }))
      .sort((a, b) => a.name.localeCompare(b.name))
    expect(namesAndVersions).toEqual([
      { name: '@pnpmtest/test-recursive-publish-project-3', version: '1.0.0' },
      { name: '@pnpmtest/test-recursive-publish-project-4', version: '1.0.0' },
    ])
    fs.unlinkSync('pnpm-publish-summary.json')
  }

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    recursive: true,
    reportSummary: true,
  }, [])

  {
    const publishSummary = loadJsonFileSync('pnpm-publish-summary.json')
    expect(publishSummary).toStrictEqual({
      publishedPackages: [],
    })
  }
})

test('recursive publish summary uses manifest from publishConfig.directory', async () => {
  const pkgName = `@pnpmtest/test-recursive-publish-config-directory-${Date.now()}`
  const packages = preparePackages([
    {
      name: pkgName,
      version: '1.0.0',

      publishConfig: {
        directory: 'dist',
      },
    },
  ])
  const selectedProjects = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  const project = packages[pkgName]
  fs.mkdirSync(path.join(project.dir(), 'dist'))
  fs.writeFileSync(path.join(project.dir(), 'dist/package.json'), JSON.stringify({
    name: pkgName,
    version: '2.0.0',
  }))

  await publish.handler({
    ...DEFAULT_OPTS,
    ...selectedProjects,
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    dryRun: true,
    force: true,
    recursive: true,
    reportSummary: true,
  }, [])

  const summary = loadJsonFileSync<{ publishedPackages: Array<{ name: string, version: string }> }>('pnpm-publish-summary.json')
  expect(summary.publishedPackages).toHaveLength(1)
  expect(summary.publishedPackages[0]).toMatchObject({
    name: pkgName,
    version: '2.0.0',
  })
})

test('errors on fake registry', async () => {
  preparePackages([
    {
      name: '@pnpmtest/test-recursive-publish-project-5',
      version: '1.0.0',
    },
    {
      name: '@pnpmtest/test-recursive-publish-project-6',
      version: '1.0.0',
    },
  ])

  const fakeRegistry = 'https://__fake_npm_registry__.com'

  const promise = publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: {},
    registries: {
      ...DEFAULT_OPTS.registries,
      default: fakeRegistry,
    },
    dir: process.cwd(),
    recursive: true,
    force: true,
  }, [])

  // NOTE: normally this should be a PnpmError, but we'd like to keep the code
  //       simple so we just let the internal functions throw error for now.
  await expect(promise).rejects.toHaveProperty(['code'], 'ENOTFOUND')
  await expect(promise).rejects.toHaveProperty(['hostname'], '__fake_npm_registry__.com')
})

test('recursive publish --json: writes per-package summary array to stdout', async () => {
  const SUFFIX = Date.now()
  const pkg1 = { name: `@pnpmtest/test-publish-r-json-1-${SUFFIX}`, version: '1.0.0' }
  const pkg2 = { name: `@pnpmtest/test-publish-r-json-2-${SUFFIX}`, version: '1.0.0' }
  preparePackages([pkg1, pkg2])

  const result = await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    json: true,
    recursive: true,
  }, [])

  expect(result?.output).toBeDefined()
  const summaries = JSON.parse(result!.output!) as Array<Record<string, unknown>>
  expect(Array.isArray(summaries)).toBe(true)
  expect(summaries).toHaveLength(2)
  expect(summaries.map((s) => s.name).sort()).toEqual([pkg1.name, pkg2.name].sort())
  for (const s of summaries) {
    expect(s).toMatchObject({ version: '1.0.0', bundled: [] })
    expect(s.id).toBe(`${s.name as string}@${s.version as string}`)
    expect(s.size).toEqual(expect.any(Number))
    expect(s.shasum).toMatch(/^[0-9a-f]{40}$/)
    expect(s.integrity).toMatch(/^sha512-/)
  }
})

test('recursive publish --report-summary: file entries use per-package summary shape', async () => {
  const SUFFIX = Date.now()
  const pkg = { name: `@pnpmtest/test-publish-r-report-summary-${SUFFIX}`, version: '1.0.0' }
  preparePackages([pkg])

  await publish.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
    recursive: true,
    reportSummary: true,
  }, [])

  const summary = loadJsonFileSync<{ publishedPackages: Array<Record<string, unknown>> }>('pnpm-publish-summary.json')
  expect(summary.publishedPackages).toHaveLength(1)
  const entry = summary.publishedPackages[0]
  expect(entry).toMatchObject({
    id: `${pkg.name}@1.0.0`,
    name: pkg.name,
    version: '1.0.0',
    bundled: [],
  })
  expect(entry.shasum).toMatch(/^[0-9a-f]{40}$/)
  expect(entry.integrity).toMatch(/^sha512-/)
  expect(entry.size).toEqual(expect.any(Number))
  expect(entry.unpackedSize).toEqual(expect.any(Number))
  fs.unlinkSync('pnpm-publish-summary.json')
})
