import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { ProjectManifest } from '@pnpm/types'
import fs = require('fs')
import isSubdir = require('is-subdir')
import loadJsonFile = require('load-json-file')
import normalizePath = require('normalize-path')
import path = require('path')
import exists = require('path-exists')
import writeJsonFile = require('write-json-file')

const repoRoot = path.join(__dirname, '../../..')

// eslint-disable-next-line @typescript-eslint/no-floating-promises
; (async () => {
  const pkgs = await findWorkspacePackages(repoRoot, { engineStrict: false })
  const pkgsDir = path.join(repoRoot, 'packages')
  const lockfile = await readWantedLockfile(repoRoot, { ignoreIncompatible: false })
  for (const { dir, manifest, writeProjectManifest } of pkgs) {
    if (isSubdir(pkgsDir, dir)) {
      await writeProjectManifest(await updateManifest(dir, manifest))
    }
    if (manifest.name === '@pnpm/tsconfig') continue
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
      if (!spec.startsWith('link:') || spec.length === 5) continue
      references.push({ path: spec.substr(5) })
    }
    const tsConfig = await loadJsonFile<Object>(tsconfigLoc)
    await writeJsonFile(tsconfigLoc, {
      ...tsConfig,
      compilerOptions: {
        ...tsConfig['compilerOptions'],
        rootDir: 'src',
      },
      references: references.sort((r1, r2) => r1.path.localeCompare(r2.path)),
    }, { indent: 2 })
    await writeJsonFile(path.join(dir, 'tsconfig.lint.json'), {
      extends: './tsconfig.json',
      include: [
        'src/**/*.ts',
        'test/**/*.ts',
        '../../typings/**/*.d.ts',
      ],
    }, { indent: 2 })
  }
})()

let registryMockPort = 7769

async function updateManifest (dir: string, manifest: ProjectManifest) {
  const usesJest = await exists(path.join(dir, 'jest.config.js'))
  const relative = normalizePath(path.relative(repoRoot, dir))
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
  case 'supi': {
    // supi tests currently works only with port 4873 due to the usage of
    // the next package: pkg-with-tarball-dep-from-registry
    const port = manifest.name === 'supi' ? 4873 : ++registryMockPort
    if (usesJest) {
      scripts = {
        ...manifest.scripts,
        'registry-mock': 'registry-mock',
        'test:jest': 'jest',

        'test:e2e': 'registry-mock prepare && run-p -r registry-mock test:jest',
      }
    } else {
      scripts = {
        ...manifest.scripts,
        'registry-mock': 'registry-mock',
        'test:tap': `cd ../.. && c8 --reporter lcov --reports-dir ${normalizePath(path.join(relative, 'coverage'))} ts-node ${normalizePath(path.join(relative, 'test'))} --type-check`,

        'test:e2e': 'registry-mock prepare && run-p -r registry-mock test:tap',
      }
    }
    scripts.test = 'pnpm run compile && pnpm run _test'
    scripts._test = `cross-env PNPM_REGISTRY_MOCK_PORT=${port} pnpm run test:e2e`
    break
  }
  default:
    if (await exists(path.join(dir, 'test'))) {
      if (manifest.scripts?._test?.includes('jest')) {
        scripts = manifest.scripts
        break
      }
      scripts = {
        ...manifest.scripts,
        _test: `cd ../.. && c8 --reporter lcov --reports-dir ${normalizePath(path.join(relative, 'coverage'))} ts-node ${normalizePath(path.join(relative, 'test'))} --type-check`,
        test: 'pnpm run compile && pnpm run _test',
      }
    } else {
      scripts = {
        ...manifest.scripts,
        test: 'pnpm run compile',
      }
    }
    break
  }
  scripts.compile = 'rimraf lib tsconfig.tsbuildinfo && tsc --build'
  delete scripts.tsc
  let homepage: string
  let repository: string | { type: 'git', url: string }
  if (manifest.name === 'pnpm') {
    homepage = 'https://pnpm.js.org'
    repository = {
      type: 'git',
      url: 'git+https://github.com/pnpm/pnpm.git',
    }
    scripts.compile += ' && rimraf dist && pnpm run bundle && shx cp -r node-gyp-bin dist/node-gyp-bin'
  } else {
    scripts.prepublishOnly = 'pnpm run compile'
    homepage = `https://github.com/pnpm/pnpm/blob/master/${relative}#readme`
    repository = `https://github.com/pnpm/pnpm/blob/master/${relative}`
  }
  if (scripts.lint) {
    if (fs.existsSync(path.join(dir, 'test'))) {
      scripts.lint = 'eslint -c ../../eslint.json src/**/*.ts test/**/*.ts'
    } else {
      scripts.lint = 'eslint -c ../../eslint.json src/**/*.ts'
    }
  }
  const files: string[] = []
  if (manifest.name === 'pnpm') {
    files.push('dist')
    files.push('bin')
  } else {
    // the order is important
    files.push('lib')
    files.push('!*.map')
    if (manifest.bin) {
      files.push('bin')
    }
  }
  return {
    ...manifest,
    author: 'Zoltan Kochan <z@kochan.io> (https://www.kochan.io/)',
    bugs: {
      url: 'https://github.com/pnpm/pnpm/issues',
    },
    engines: {
      node: '>=10.16',
    },
    files,
    funding: 'https://opencollective.com/pnpm',
    homepage,
    license: 'MIT',
    repository,
    scripts,
  }
}
