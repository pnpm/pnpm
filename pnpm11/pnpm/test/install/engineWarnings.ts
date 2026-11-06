import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpmSync } from '../utils/index.js'

test.each([
  { logLevel: 'info', optional: false },
  { logLevel: 'warn', optional: false },
  { logLevel: 'info', optional: true },
  { logLevel: 'warn', optional: true },
])('engine mismatch with optional=$optional at loglevel $logLevel on fresh and frozen installs', ({ logLevel, optional }) => {
  const project = prepare({
    [optional ? 'optionalDependencies' : 'dependencies']: { 'incompatible-engine': 'file:./incompatible-engine' },
  })
  fs.mkdirSync('incompatible-engine')
  fs.writeFileSync('incompatible-engine/package.json', JSON.stringify({
    name: 'incompatible-engine',
    version: '1.0.0',
    engines: { node: '>=999.0.0' },
  }))
  fs.writeFileSync('pnpm-workspace.yaml', 'nodeVersion: 20.0.0\nengineStrict: false\n')

  for (const args of [[], ['--frozen-lockfile']]) {
    const result = execPnpmSync(['install', '--reporter=append-only', `--loglevel=${logLevel}`, ...args])
    expect(result.status).toBe(0)
    const output = `${result.stdout}${result.stderr}`
    if (optional) {
      expect(output).not.toContain('Unsupported engine for')
    } else {
      expect(output).toContain('WARN')
      expect(output).toContain('Unsupported engine for')
      expect(output).toContain('incompatible-engine')
      expect(output).toContain('{"node":"20.0.0"}')
      project.has('incompatible-engine/package.json')
    }
  }
})
