import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { ProjectManifest } from '@pnpm/types'
import isSubdir = require('is-subdir')
import loadJsonFile = require('load-json-file')
import path = require('path')
import exists = require('path-exists')
import writeJsonFile = require('write-json-file')

const repoRoot = path.join(__dirname, '../../..')

; (async () => {
  const pkgs = await findWorkspacePackages(repoRoot, { engineStrict: false })
  const pkgsDir = path.join(repoRoot, 'packages')
  const lockfile = await readWantedLockfile(repoRoot, { ignoreIncompatible: false })
  for (const { dir, manifest, writeProjectManifest } of pkgs) {
    if (isSubdir(pkgsDir, dir)) {
      await writeProjectManifest(await updateManifest(dir, manifest))
    }
    if (manifest.name === '@pnpm/fetch' || manifest.name === '@pnpm/tsconfig') continue
    const relative = path.relative(repoRoot, dir)
    const importer = lockfile.importers[relative]
    if (!importer) continue
    const tsconfigLoc = path.join(dir, 'tsconfig.json')
    if (!await exists(tsconfigLoc)) continue
    const deps = {
      ...importer.dependencies,
      ...importer.devDependencies,
    }
    const references = [] as Array<{ path: string }>
    for (const spec of Object.values(deps)) {
      if (!spec.startsWith('link:') || spec.length === 5 || spec === 'link:../fetch') continue
      references.push({ path: spec.substr(5) })
    }
    const tsConfig = await loadJsonFile<Object>(tsconfigLoc)
    await writeJsonFile(tsconfigLoc, {
      ...tsConfig,
      compilerOptions: {
        ...tsConfig['compilerOptions'],
        rootDir: 'src',
      },
      references,
    }, { indent: 2 })
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
        scripts.test = 'pnpm run tsc && pnpm run _test'
        scripts._test = `cross-env PNPM_REGISTRY_MOCK_PORT=${port} pnpm run test:e2e`
      }
      break
    default:
      if (await exists(path.join(dir, 'test'))) {
        scripts = {
          ...manifest.scripts,
          _test: `cd ../.. && c8 --reporter lcov --reports-dir ${path.join(relative, 'coverage')} ts-node ${path.join(relative, 'test')} --type-check`,
          test: 'pnpm run tsc && pnpm run _test',
        }
      } else {
        scripts = {
          ...manifest.scripts,
          test: 'pnpm run tsc',
        }
      }
      break
  }
  if (manifest.name === '@pnpm/fetch') {
    scripts.tsc = 'rimraf lib && tsc && cpy src/**/*.d.ts lib'
  } else {
    scripts.tsc = 'rimraf lib tsconfig.tsbuildinfo && tsc --build'
  }
  let homepage: string
  let repository: string | { type: 'git', url: string }
  if (manifest.name === 'pnpm') {
    homepage = 'https://pnpm.js.org'
    repository = {
      type: 'git',
      url: 'git+https://github.com/pnpm/pnpm.git',
    }
  } else {
    homepage = `https://github.com/pnpm/pnpm/blob/master/${relative}#readme`
    repository = `https://github.com/pnpm/pnpm/blob/master/${relative}`
  }
  const files = ['lib', '!*.map']
  if (manifest.bin) {
    files.push('bin')
  }
  files.sort()
  return {
    ...manifest,
    author: 'Zoltan Kochan <z@kochan.io> (https://www.kochan.io/)',
    bugs: {
      url: 'https://github.com/pnpm/pnpm/issues',
    },
    engines: {
      node: '>=10',
    },
    files,
    homepage,
    license: 'MIT',
    repository,
    scripts,
  }
}
