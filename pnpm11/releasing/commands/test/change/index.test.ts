import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, it } from '@jest/globals'

import { change, lane, version } from '../../src/index.js'

interface FixturePkg {
  name: string
  version: string
  dependencies?: Record<string, string>
}

describe('change command and intent-consuming version -r', () => {
  let tempDir: string

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-change-test-'))
    fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  })

  afterEach(() => {
    fs.rmSync(tempDir, { recursive: true, force: true })
  })

  function addPkg (pkg: FixturePkg): { rootDir: string, manifest: FixturePkg } {
    const rootDir = path.join(tempDir, 'packages', pkg.name)
    fs.mkdirSync(rootDir, { recursive: true })
    fs.writeFileSync(path.join(rootDir, 'package.json'), JSON.stringify(pkg, null, 2))
    return { rootDir, manifest: pkg }
  }

  function baseOpts (projects: Array<{ rootDir: string, manifest: FixturePkg }>): object {
    return {
      dir: tempDir,
      workspaceDir: tempDir,
      allProjects: projects,
      gitChecks: false,
      recursive: true,
    }
  }

  it('exports the change command', () => {
    expect(change.commandNames).toEqual(['change'])
    expect(change.help()).toContain('change intent')
  })

  it('records an intent non-interactively and shows it in status', async () => {
    const lib = addPkg({ name: 'lib', version: '1.0.0' })
    const cli = addPkg({ name: 'cli', version: '2.0.0', dependencies: { lib: 'workspace:*' } })
    const opts = baseOpts([lib, cli])

    const output = await change.handler({ ...opts, bump: 'minor', summary: 'Added a feature.' } as any, ['lib']) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(output).toMatch(/Recorded change intent \.changeset\/.+\.md/)

    const status = await change.handler(opts as any, ['status']) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(status).toContain('lib: 1.0.0 → 1.1.0')
    expect(status).toContain('cli: 2.0.0 → 2.0.1')
  })

  it('rejects an unknown package name', async () => {
    const lib = addPkg({ name: 'lib', version: '1.0.0' })
    await expect(
      change.handler({ ...baseOpts([lib]), bump: 'patch', summary: 'x' } as any, ['ghost']) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_VERSIONING_UNKNOWN_PACKAGE' })
  })

  it('bare version -r applies the release plan and cleans up the intent', async () => {
    const lib = addPkg({ name: 'lib', version: '1.0.0' })
    const cli = addPkg({ name: 'cli', version: '2.0.0', dependencies: { lib: 'workspace:*' } })
    const opts = baseOpts([lib, cli])

    await change.handler({ ...opts, bump: 'major', summary: 'Breaking change.' } as any, ['lib']) // eslint-disable-line @typescript-eslint/no-explicit-any

    const dryRun = await version.handler({ ...opts, dryRun: true } as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(dryRun).toContain('lib: 1.0.0 → 2.0.0')

    const output = await version.handler(opts as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(output).toContain('lib: 1.0.0 → 2.0.0')
    expect(output).toContain('cli: 2.0.0 → 2.0.1')

    expect(JSON.parse(fs.readFileSync(path.join(lib.rootDir, 'package.json'), 'utf8')).version).toBe('2.0.0')
    const changelog = fs.readFileSync(path.join(lib.rootDir, 'CHANGELOG.md'), 'utf8')
    expect(changelog).toContain('- Breaking change.')
    const remaining = fs.readdirSync(path.join(tempDir, '.changeset')).filter((fileName) => fileName.endsWith('.md'))
    expect(remaining).toHaveLength(0)
    expect(fs.readFileSync(path.join(tempDir, '.changeset', 'ledger.yaml'), 'utf8')).toContain('lib@2.0.0')
  })

  it('bare version -r without pending intents reports nothing to do', async () => {
    const lib = addPkg({ name: 'lib', version: '1.0.0' })
    const output = await version.handler(baseOpts([lib]) as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(output).toContain('No pending changes')
  })

  it('lane assignments update versioning.lanes in pnpm-workspace.yaml', async () => {
    const cli = addPkg({ name: 'cli', version: '2.0.0' })
    const opts = {
      ...baseOpts([cli]),
      filter: ['cli'],
      selectedProjectsGraph: {
        [cli.rootDir]: { dependencies: [], package: { rootDir: cli.rootDir, manifest: cli.manifest } },
      },
    }

    await lane.handler(opts as any, ['alpha']) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(fs.readFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'utf8')).toContain('cli: alpha')

    const status = await lane.handler({ ...opts, versioning: { lanes: { cli: 'alpha' } } } as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(status).toContain('alpha:')
    expect(status).toContain('    cli')

    await lane.handler({ ...opts, versioning: { lanes: { cli: 'alpha' } } } as any, ['main']) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(fs.readFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'utf8')).not.toContain('alpha')
  })

  it('a none-only intent is consumed by a version -r run with nothing to release', async () => {
    const lib = addPkg({ name: 'lib', version: '1.0.0' })
    const opts = baseOpts([lib])
    await change.handler({ ...opts, bump: 'none', summary: 'refactor, no release needed' } as any, ['lib']) // eslint-disable-line @typescript-eslint/no-explicit-any
    const output = await version.handler(opts as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(output).toContain('No pending changes')
    const remaining = fs.readdirSync(path.join(tempDir, '.changeset')).filter((fileName) => fileName.endsWith('.md'))
    expect(remaining).toHaveLength(0)
  })

  it('rejects differently-cased spellings of the reserved main lane', async () => {
    const cli = addPkg({ name: 'cli', version: '2.0.0' })
    const opts = {
      ...baseOpts([cli]),
      filter: ['cli'],
      selectedProjectsGraph: {
        [cli.rootDir]: { dependencies: [], package: { rootDir: cli.rootDir, manifest: cli.manifest } },
      },
    }
    await expect(
      lane.handler(opts as any, ['MAIN']) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_VERSIONING_INVALID_LANE_NAME' })
  })

  it('lane assignment requires a filter', async () => {
    const cli = addPkg({ name: 'cli', version: '2.0.0' })
    await expect(
      lane.handler(baseOpts([cli]) as any, ['alpha']) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toMatchObject({ code: 'ERR_PNPM_VERSIONING_LANE_FILTER_REQUIRED' })
  })

  it('bare lane reports when everything is on the main lane', async () => {
    const cli = addPkg({ name: 'cli', version: '2.0.0' })
    const output = await lane.handler(baseOpts([cli]) as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(output).toBe('All packages are on the main lane.')
  })
})
