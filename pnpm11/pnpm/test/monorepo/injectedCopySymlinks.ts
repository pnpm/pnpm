import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages, tempDir } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// An injected copy holds the package's links to its own content the way the
// install writes them, after a script listed in `syncInjectedDepsAfterScripts`
// too (pnpm/tasks#147).

// Creating links needs privileges on Windows.
const testOnPosix = isWindows() ? test.skip : test

// The build replaces a file under a linked directory and adds a new link.
const BUILD = `const fs = require('fs')
fs.rmSync(__dirname + '/files/foo/a.txt')
fs.writeFileSync(__dirname + '/files/foo/a.txt', 'rebuilt')
fs.symlinkSync('files/foo', __dirname + '/new-link')
`

testOnPosix.each([false, true])('a listed script keeps links as the install writes them, deployAllFiles: %s', async (deployAllFiles) => {
  preparePackages([
    {
      name: 'app',
      version: '1.0.0',
      dependencies: { lib: 'workspace:*' },
      dependenciesMeta: { lib: { injected: true } },
    },
    {
      name: 'lib',
      version: '1.0.0',
      scripts: { build: 'node build.cjs' },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['app', 'lib'],
    deployAllFiles,
    dedupeInjectedDeps: false,
    syncInjectedDepsAfterScripts: ['build'],
  })
  fs.writeFileSync('lib/build.cjs', BUILD)
  fs.mkdirSync('lib/files/foo', { recursive: true })
  fs.writeFileSync('lib/files/foo/a.txt', 'a')
  fs.mkdirSync('lib/shared')
  fs.writeFileSync('lib/shared/x.txt', 'x')
  for (const [target, link] of [['files/foo', 'link-to-a-dir'], ['shared', 'a'], ['shared', 'b'], ['.', 'loop']]) {
    fs.symlinkSync(target, path.join('lib', link))
  }
  const workspace = process.cwd()

  await execPnpm(['install'])
  await execPnpm(['--filter=lib', 'run', 'build'])

  const clean = path.join(tempDir(false), 'workspace')
  fs.cpSync(workspace, clean, {
    recursive: true,
    verbatimSymlinks: true,
    filter: (source) => !['node_modules', 'store', 'cache'].includes(path.basename(source)),
  })
  await execPnpm(['--dir', clean, 'install', '--frozen-lockfile'])

  expect(tree(injectedCopy(workspace))).toStrictEqual(tree(injectedCopy(clean)))
})

// The copy of `lib` that `app` resolves in `workspace`.
function injectedCopy (workspace: string): string {
  const copy = fs.realpathSync(path.join(workspace, 'app/node_modules/lib'))
  expect(copy).not.toBe(fs.realpathSync(path.join(workspace, 'lib')))
  return copy
}

// Every entry under `dir` outside `node_modules`, without following links.
function tree (dir: string, prefix = ''): Record<string, string> {
  const entries: Record<string, string> = {}
  for (const entry of fs.readdirSync(path.join(dir, prefix), { withFileTypes: true })) {
    if (entry.name === 'node_modules') continue
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name
    entries[relative] = describe(entry, path.join(dir, relative))
    if (entries[relative] === 'dir') Object.assign(entries, tree(dir, relative))
  }
  return entries
}

function describe (entry: fs.Dirent, absolute: string): string {
  if (entry.isSymbolicLink()) return `link:${fs.readlinkSync(absolute)}`
  if (entry.isDirectory()) return 'dir'
  return `file:${fs.readFileSync(absolute, 'utf8')}`
}
