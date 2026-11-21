import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test.each([
  { bin: { 'cli-tool': 'bin/cli-tool' } },
  { directories: { bin: 'bin' } },
])('sync bin links after build script with %j', async binFields => {
  preparePackages([
    {
      name: '@example/cli-tool',
      version: '1.0.0',
      ...binFields,
      scripts: {
        build: 'node -e "const fs = require(\'fs\'); fs.mkdirSync(\'bin\', { recursive: true }); fs.writeFileSync(\'bin/cli-tool\', \'#!/usr/bin/env node\\nconsole.log(\\\'CLI tool works!\\\')\\n\', \'utf-8\')"',
      },
    },
    {
      name: 'consumer',
      version: '1.0.0',
      dependencies: {
        '@example/cli-tool': 'workspace:*',
      },
      dependenciesMeta: {
        '@example/cli-tool': {
          injected: true,
        },
      },
      scripts: {
        test: 'cli-tool',
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['consumer', '@example/*'],
    reporter: 'append-only',
    injectWorkspacePackages: true,
    dedupeInjectedDeps: false,
    syncInjectedDepsAfterScripts: ['build'],
  })

  // Install - bin won't be created because bin/cli-tool doesn't exist yet
  await execPnpm(['install'])

  // Build cli-tool
  await execPnpm(['--filter=@example/cli-tool', 'run', 'build'])

  // Verify bin/cli-tool was created
  expect(fs.existsSync('@example/cli-tool/bin/cli-tool')).toBe(true)

  // Verify bin was synced to the injected location
  const copy = fs.realpathSync('consumer/node_modules/@example/cli-tool')
  expect(fs.existsSync(path.join(copy, 'bin/cli-tool'))).toBe(true)
  expect(binExists(path.resolve(copy, '../../.bin/cli-tool'))).toBe(true)
  expect(fs.existsSync(path.resolve(copy, '../.bin'))).toBe(false)

  // Run the consumer's test script which uses the bin
  await execPnpm(['--filter=consumer', 'test'])
})

test('removes the bin link of a bin the build script dropped', async () => {
  preparePackages([
    {
      name: 'cli-tool',
      version: '1.0.0',
      bin: {
        'kept-cli': 'bin/kept.js',
        'dropped-cli': 'bin/dropped.js',
      },
      scripts: {
        // Rewrite the manifest without `dropped-cli`, the way a build step
        // that regenerates package.json would.
        build: 'node ./drop-bin.cjs',
      },
    },
    {
      name: 'consumer',
      version: '1.0.0',
      dependencies: {
        'cli-tool': 'workspace:*',
      },
      dependenciesMeta: {
        'cli-tool': {
          injected: true,
        },
      },
    },
  ])

  fs.mkdirSync('cli-tool/bin', { recursive: true })
  fs.writeFileSync('cli-tool/bin/kept.js', '#!/usr/bin/env node\nconsole.log("kept")\n')
  fs.writeFileSync('cli-tool/bin/dropped.js', '#!/usr/bin/env node\nconsole.log("dropped")\n')
  fs.writeFileSync(
    'cli-tool/drop-bin.cjs',
    [
      "const fs = require('fs')",
      "const manifestPath = __dirname + '/package.json'",
      "const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'))",
      "delete manifest.bin['dropped-cli']",
      'fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))',
      "fs.rmSync(__dirname + '/bin/dropped.js')",
      '',
    ].join('\n')
  )

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['*'],
    reporter: 'append-only',
    injectWorkspacePackages: true,
    dedupeInjectedDeps: false,
    syncInjectedDepsAfterScripts: ['build'],
  })

  await execPnpm(['install'])

  // The install spreads an injected package's bins over several `.bin`
  // directories (beside the copy, inside it, and the hoisted one), so
  // assert over every directory rather than the ones we happen to expect.
  expect(binDirsHolding('.', 'dropped-cli')).not.toStrictEqual([])
  expect(binDirsHolding('.', 'kept-cli')).not.toStrictEqual([])

  await execPnpm(['--filter=cli-tool', 'run', 'build'])

  expect(binDirsHolding('.', 'dropped-cli')).toStrictEqual([])
  expect(binDirsHolding('.', 'kept-cli')).not.toStrictEqual([])
})

/** Every `.bin` directory under `dir` holding a shim named `binName`. */
function binDirsHolding (dir: string, binName: string): string[] {
  const found: string[] = []
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue
    const child = path.join(dir, entry.name)
    if (entry.name === '.bin' && binExists(path.join(child, binName))) {
      found.push(child)
    }
    found.push(...binDirsHolding(child, binName))
  }
  return found.sort()
}

/** A bin is a symlink on POSIX and a set of shims on Windows. */
function binExists (binPath: string): boolean {
  return fs.existsSync(binPath) ||
    fs.existsSync(`${binPath}.CMD`) ||
    fs.existsSync(`${binPath}.ps1`)
}
