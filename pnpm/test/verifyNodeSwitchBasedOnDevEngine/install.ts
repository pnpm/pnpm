import { prepare } from '@pnpm/prepare'
import { execPnpmSync } from '../utils/index.js'

const scenarios = [
  { nodeVersion: '22.20.0', pkg: 'is-positive', pkgVersion: '1.0.0' },
  { nodeVersion: '22.18.0', pkg: 'is-negative', pkgVersion: '1.0.0' },
]

test.each(scenarios)(
  'pnpm respects devEngines on install with Node %s',
  ({ nodeVersion, pkg, pkgVersion }) => {
    const project = prepare({
      name: 'engine-test',
      private: true,
      devEngines: {
        runtime: { name: 'node', version: nodeVersion, onFail: 'download' },
      },
      dependencies: {
        [pkg]: pkgVersion,
      },
    })

    const { stdout, stderr } = execPnpmSync(['install'], { expectSuccess: true })
    const log = stdout.toString() + stderr.toString()

    expect(log).toMatch(new RegExp(`node ${nodeVersion.replace(/\./g, '\\.')}`))

    project.has(pkg)
    const module = project.requireModule(pkg)
    expect(typeof module).toBe('function')
  }
)
