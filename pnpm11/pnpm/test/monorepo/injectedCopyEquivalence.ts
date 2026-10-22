import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

// Every writer of an injected copy leaves the files the install selects: the
// package's packlist, or every file under `deployAllFiles` (pnpm/tasks#145).
// The writers, in the order a case runs them:
//
// 1. the install, which also refreshes the copy after the package's
//    `prepare` script;
// 2. the sync after a script listed in `syncInjectedDepsAfterScripts`;
// 3. the same sync after a source edit;
// 4. a frozen install into a clean tree.

interface Layout {
  sharedWorkspaceLockfile: boolean
  nodeLinker: 'isolated' | 'hoisted'
  deployAllFiles: boolean
}

const LAYOUTS: Layout[] = [true, false].flatMap((sharedWorkspaceLockfile) =>
  (['isolated', 'hoisted'] as const).flatMap((nodeLinker) =>
    [false, true].map((deployAllFiles) => ({ sharedWorkspaceLockfile, nodeLinker, deployAllFiles }))
  )
)

// `writer.cjs` writes `dist/<name>.js`, which `files` lists, and
// `<name>.log`, which it leaves out.
const WRITER = `const fs = require('fs')
const name = process.argv[2]
fs.mkdirSync(__dirname + '/dist', { recursive: true })
fs.writeFileSync(__dirname + '/dist/' + name + '.js', fs.readFileSync(__dirname + '/index.js'))
fs.writeFileSync(__dirname + '/' + name + '.log', name)
`

describe.each(LAYOUTS)('injected copy equivalence: %j', (layout) => {
  test('every writer leaves the files the install selects', async () => {
    prepareWorkspace(layout)
    const workspace = process.cwd()

    await execPnpm(['install'])
    expect(fs.existsSync('lib/dist/prepared.js')).toBe(true)
    expectCopy(layout, workspace, 'the install')

    await execPnpm(['--filter=lib', 'run', 'build'])
    expectCopy(layout, workspace, 'a listed script')

    fs.writeFileSync('lib/index.js', 'module.exports = 2\n')
    await execPnpm(['--filter=lib', 'run', 'build'])
    expectCopy(layout, workspace, 'a listed script after an edit')

    const clean = path.join(tempDir(false), 'workspace')
    fs.cpSync(workspace, clean, { recursive: true, filter: (source) => !['node_modules', 'store', 'cache'].includes(path.basename(source)) })
    await execPnpm(['--dir', clean, 'install', '--frozen-lockfile'])
    expectCopy(layout, clean, 'a frozen install into a clean tree')
  })
})

// `app` injects `lib`. `lib` lists `dist` and `index.js` in `files`, and also
// holds a README that every packlist includes, a source file and a secret
// that `files` leaves out.
function prepareWorkspace (layout: Layout): void {
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
      files: ['dist', 'index.js'],
      scripts: {
        prepare: 'node writer.cjs prepared',
        build: 'node writer.cjs built',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    sharedWorkspaceLockfile: layout.sharedWorkspaceLockfile,
    nodeLinker: layout.nodeLinker,
    deployAllFiles: layout.deployAllFiles,
    dedupeInjectedDeps: false,
    syncInjectedDepsAfterScripts: ['build'],
  })
  for (const [file, contents] of Object.entries({
    'writer.cjs': WRITER,
    'index.js': 'module.exports = 1\n',
    'README.md': '# lib\n',
    'secret.txt': 'secret\n',
    'src/source.ts': 'export default 1\n',
  })) {
    fs.mkdirSync(path.dirname(path.join('lib', file)), { recursive: true })
    fs.writeFileSync(path.join('lib', file), contents)
  }
}

function expectCopy (layout: Layout, workspace: string, writer: string): void {
  const copy = injectedCopy(workspace)
  const expected = expectedFiles(layout, path.join(workspace, 'lib'))
  expect({ writer, files: filesUnder(copy) }).toEqual({ writer, files: expected })
}

// The injected copy that `app` in `workspace` resolves `lib` to.
function injectedCopy (workspace: string): string {
  let dir = path.join(workspace, 'app')
  for (;;) {
    const candidate = path.join(dir, 'node_modules', 'lib')
    if (fs.existsSync(path.join(candidate, 'package.json'))) {
      const copy = fs.realpathSync(candidate)
      expect(copy).not.toBe(fs.realpathSync(path.join(workspace, 'lib')))
      return copy
    }
    const parent = path.dirname(dir)
    if (parent === dir) throw new Error(`app in ${workspace} does not resolve lib`)
    dir = parent
  }
}

// The files the install selects from `lib`, by path, with their contents.
function expectedFiles (layout: Layout, lib: string): Record<string, string> {
  const paths = layout.deployAllFiles ? Object.keys(filesUnder(lib)) : packlist(lib)
  return Object.fromEntries(paths.sort().map((file) => [file, fs.readFileSync(path.join(lib, file), 'utf8')]))
}

// `pnpm pack --dry-run` of a copy of `lib`, so no script touches it.
function packlist (lib: string): string[] {
  const copy = path.join(tempDir(false), 'lib')
  fs.cpSync(lib, copy, { recursive: true, filter: (source) => path.basename(source) !== 'node_modules' })
  const { stdout } = execPnpmSync(['--dir', copy, 'pack', '--dry-run', '--json'], { expectSuccess: true })
  return (JSON.parse(stdout.toString()) as { files: Array<{ path: string }> }).files.map((file) => file.path)
}

// Every file under `dir` outside `node_modules`, by its `/`-separated path.
function filesUnder (dir: string, prefix = ''): Record<string, string> {
  const files: Record<string, string> = {}
  for (const entry of fs.readdirSync(path.join(dir, prefix), { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name
    if (entry.isDirectory() && entry.name !== 'node_modules') {
      Object.assign(files, filesUnder(dir, relative))
    } else if (entry.isFile()) {
      files[relative] = fs.readFileSync(path.join(dir, relative), 'utf8')
    }
  }
  return Object.fromEntries(Object.entries(files).sort(([left], [right]) => left.localeCompare(right)))
}
