import fs from 'fs'
import { preparePackages } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'
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

  writeYamlFile('pnpm-workspace.yaml', {
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
