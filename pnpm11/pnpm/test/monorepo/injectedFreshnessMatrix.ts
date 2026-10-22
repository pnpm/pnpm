import fs from 'node:fs'
import path from 'node:path'
import { isDeepStrictEqual } from 'node:util'

import { describe, expect, test } from '@jest/globals'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// An edit to an injected workspace package, checked across lockfile layouts,
// injection forms, and filtered installs (pnpm/tasks#145). After the edit:
//
// - Agreement: `pnpm install --frozen-lockfile` from a clean tree fails
//   exactly when a non-frozen install of the same selection changes a lockfile.
// - Convergence: after that install, a frozen install from a clean tree passes.
// - Equivalence: the lockfiles the install owns equal the ones a clean install
//   without lockfiles writes. One edited project fails the frozen install of
//   the whole workspace, so only this catches a stale lockfile of another
//   project.

type Injection = 'injectWorkspacePackages' | 'dependenciesMeta' | 'fileSpec'
type Edit = 'none' | 'dependency' | 'peer' | 'optionalPeer' | 'catalogPeer'

interface Layout {
  sharedWorkspaceLockfile: boolean
  injection: Injection
}

const LAYOUTS: Layout[] = [true, false].flatMap((sharedWorkspaceLockfile) =>
  (['injectWorkspacePackages', 'dependenciesMeta', 'fileSpec'] as const).map((injection) => ({ sharedWorkspaceLockfile, injection }))
)
const EDITS: Edit[] = ['none', 'dependency', 'peer', 'optionalPeer', 'catalogPeer']
const FILTERS: string[][] = [[], ['--filter=app']]

// A filtered install leaves a shared lockfile's other importers as they were,
// so a resolution from scratch can judge only the lockfile `--filter=app` owns
// when every project has its own.
const APP_LOCKFILE = 'app/pnpm-lock.yaml'

describe.each(LAYOUTS)('injected package edits: %j', (layout) => {
  test.each(EDITS.flatMap((edit) => FILTERS.map((filter) => [edit, filter] as const)))('%s, filter %j', async (edit, filter) => {
    function cleanCopy (): string {
      const copy = path.join(tempDir(false), 'workspace')
      fs.cpSync('.', copy, {
        recursive: true,
        filter: (source) => !['node_modules', 'store'].includes(path.basename(source)),
      })
      return copy
    }
    async function frozenInstallPasses (): Promise<boolean> {
      try {
        await execPnpm(['--dir', cleanCopy(), 'install', '--frozen-lockfile', ...filter])
        return true
      } catch (err: unknown) {
        expect(String(err)).toMatch(/ERR_PNPM_OUTDATED_LOCKFILE|ERR_PNPM_LOCKFILE_CONFIG_MISMATCH/)
        return false
      }
    }

    prepareWorkspace(layout)
    await execPnpm(['install'])
    applyEdit(layout, edit)

    const frozenPassed = await frozenInstallPasses()
    const before = readLockfiles('.')
    await execPnpm(['install', '--no-frozen-lockfile', ...filter])
    const after = readLockfiles('.')
    expect(isDeepStrictEqual(before, after)).toBe(frozenPassed)

    expect(await frozenInstallPasses()).toBe(true)

    if (filter.length > 0 && layout.sharedWorkspaceLockfile) return
    const scratch = cleanCopy()
    for (const lockfile of Object.keys(readLockfiles(scratch))) {
      fs.rmSync(path.join(scratch, lockfile))
    }
    await execPnpm(['--dir', scratch, 'install', '--no-frozen-lockfile'])
    const expected = readLockfiles(scratch)
    if (filter.length > 0) {
      expect(after[APP_LOCKFILE]).toBe(expected[APP_LOCKFILE])
    } else {
      expect(after).toEqual(expected)
    }
  })
})

// `app` injects `lib`, and `lib` has a dependency and a peer from the catalog
// that `app` provides.
function prepareWorkspace (layout: Layout): void {
  preparePackages([
    {
      name: 'app',
      version: '1.0.0',
      dependencies: {
        lib: layout.injection === 'fileSpec' ? 'file:../lib' : 'workspace:*',
        '@pnpm.e2e/bar': '100.0.0',
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      },
      ...layout.injection === 'dependenciesMeta' ? { dependenciesMeta: { lib: { injected: true } } } : {},
    },
    {
      name: 'lib',
      version: '1.0.0',
      dependencies: { '@pnpm.e2e/foo': '100.0.0' },
      peerDependencies: { '@pnpm.e2e/bar': 'catalog:' },
    },
  ])
  writeWorkspaceYaml(layout, '^100.0.0')
}

function writeWorkspaceYaml (layout: Layout, barCatalogEntry: string): void {
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    sharedWorkspaceLockfile: layout.sharedWorkspaceLockfile,
    injectWorkspacePackages: layout.injection === 'injectWorkspacePackages',
    catalog: { '@pnpm.e2e/bar': barCatalogEntry },
  })
}

function applyEdit (layout: Layout, edit: Edit): void {
  const manifest = JSON.parse(fs.readFileSync('lib/package.json', 'utf8'))
  switch (edit) {
    case 'none':
      return
    case 'dependency':
      manifest.dependencies['@pnpm.e2e/foo'] = '100.1.0'
      break
    case 'peer':
      manifest.peerDependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '^100.0.0'
      break
    case 'optionalPeer':
      manifest.peerDependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '^100.0.0'
      manifest.peerDependenciesMeta = { '@pnpm.e2e/dep-of-pkg-with-1-dep': { optional: true } }
      break
    case 'catalogPeer':
      writeWorkspaceYaml(layout, '100.0.0')
      return
  }
  fs.writeFileSync('lib/package.json', JSON.stringify(manifest, null, 2))
}

// Every wanted lockfile under `dir`, by its path relative to `dir`.
function readLockfiles (dir: string, prefix = ''): Record<string, string> {
  const lockfiles: Record<string, string> = {}
  for (const entry of fs.readdirSync(path.join(dir, prefix), { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name
    if (entry.isDirectory() && entry.name !== 'node_modules' && entry.name !== 'store') {
      Object.assign(lockfiles, readLockfiles(dir, relative))
    } else if (entry.isFile() && entry.name === 'pnpm-lock.yaml') {
      lockfiles[relative] = fs.readFileSync(path.join(dir, relative), 'utf8')
    }
  }
  return lockfiles
}

