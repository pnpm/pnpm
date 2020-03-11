import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { ProjectManifest } from '@pnpm/types'
import isSubdir = require('is-subdir')
import path = require('path')
import exists = require('path-exists')

const repoRoot = path.join(__dirname, '../../..')

; (async () => {
  const pkgs = await findWorkspacePackages(repoRoot, { engineStrict: false })
  const pkgsDir = path.join(repoRoot, 'packages')
  for (const { dir, manifest, writeProjectManifest } of pkgs) {
    if (!isSubdir(pkgsDir, dir)) continue
    await writeProjectManifest(await updateManifest(dir, manifest))
  }
})()

let registryMockPort = 7769

async function updateManifest (dir: string, manifest: ProjectManifest) {
  const relative = path.relative(repoRoot, dir)
  let scripts: Record<string, string>
  switch (manifest.name) {
    case '@pnpm/lockfile-types':
      scripts = {}
      break
    case '@pnpm/headless':
    case '@pnpm/outdated':
    case '@pnpm/plugin-commands-import':
    case '@pnpm/plugin-commands-installation':
    case '@pnpm/plugin-commands-listing':
    case '@pnpm/plugin-commands-outdated':
    case '@pnpm/plugin-commands-publishing':
    case '@pnpm/plugin-commands-rebuild':
    case '@pnpm/plugin-commands-script-runners':
    case '@pnpm/plugin-commands-store':
    case 'pnpm':
    case 'supi':
      // supi tests currently works only with port 4873 due to the usage of
      // the next package: pkg-with-tarball-dep-from-registry
      const port = manifest.name === 'supi' ? 4873 : ++registryMockPort
      scripts = {
        ...manifest.scripts,
        'registry-mock': 'registry-mock',
        'test:tap': `cd ../.. && c8 --reporter lcov --reports-dir ${path.join(relative, 'coverage')} ts-node ${path.join(relative, 'test')} --type-check`,

        'test:e2e': 'registry-mock prepare && run-p -r registry-mock test:tap',
      }
      if (manifest.name === 'pnpm') {
        scripts.test = 'pnpm run _test'
        scripts._test = `pnpm run tsc && cross-env PNPM_REGISTRY_MOCK_PORT=${port} pnpm run test:e2e`
      } else {
        scripts.test = 'pnpm run tsc -- --sourceMap && pnpm run _test'
        scripts._test = `cross-env PNPM_REGISTRY_MOCK_PORT=${port} pnpm run test:e2e`
      }
      break
    default:
      if (await exists(path.join(dir, 'test'))) {
        scripts = {
          ...manifest.scripts,
          _test: `cd ../.. && c8 --reporter lcov --reports-dir ${path.join(relative, 'coverage')} ts-node ${path.join(relative, 'test')} --type-check`,
          test: 'pnpm run tsc -- --sourceMap && pnpm run _test',
        }
      } else {
        scripts = {
          ...manifest.scripts,
          test: 'pnpm run tsc -- --sourceMap',
        }
      }
      break
  }
  return {
    ...manifest,
    scripts,
  }
}
