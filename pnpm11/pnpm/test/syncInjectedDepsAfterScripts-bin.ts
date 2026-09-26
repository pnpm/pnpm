import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test('sync bin links after build script', async () => {
  preparePackages([
    {
      name: 'cli-tool',
      version: '1.0.0',
      bin: {
        'cli-tool': 'bin/cli.js',
      },
      scripts: {
        build: 'node -e "const fs = require(\'fs\'); fs.mkdirSync(\'bin\', { recursive: true }); fs.writeFileSync(\'bin/cli.js\', \'#!/usr/bin/env node\\nconsole.log(\\\'CLI tool works!\\\')\\n\', \'utf-8\')"',
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
      scripts: {
        test: 'cli-tool',
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['*'],
    reporter: 'append-only',
    injectWorkspacePackages: true,
    dedupeInjectedDeps: false,
    syncInjectedDepsAfterScripts: ['build'],
  })

  // Install - bin won't be created because bin/cli.js doesn't exist yet
  await execPnpm(['install'])

  // Verify injection happened
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('cli-tool@file+cli-tool')

  // Build cli-tool
  await execPnpm(['--filter=cli-tool', 'run', 'build'])

  // Verify bin/cli.js was created
  expect(fs.existsSync('cli-tool/bin/cli.js')).toBe(true)

  // Verify bin was synced to the injected location
  const injectedBinPath = 'node_modules/.pnpm/cli-tool@file+cli-tool/node_modules/cli-tool/bin/cli.js'
  expect(fs.existsSync(injectedBinPath)).toBe(true)

  // Verify bin link was created
  const binPath = 'node_modules/.pnpm/cli-tool@file+cli-tool/node_modules/.bin/cli-tool'
  expect(fs.existsSync(binPath) || fs.existsSync(`${binPath}.CMD`) || fs.existsSync(`${binPath}.ps1`)).toBe(true)

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
  // directories — beside the copy, inside it, and the hoisted one — so
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
