import { readWantedLockfile, Lockfile } from '@pnpm/lockfile-file'
import { ProjectManifest } from '@pnpm/types'
import fs = require('fs')
import isSubdir = require('is-subdir')
import normalizePath = require('normalize-path')
import path = require('path')
import exists = require('path-exists')
import writeJsonFile = require('write-json-file')

export default async (workspaceDir: string) => {
  const pkgsDir = path.join(workspaceDir, 'packages')
  const lockfile = await readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
  if (!lockfile) {
    throw new Error('no lockfile found')
  }
  return {
    'package.json': (manifest: ProjectManifest, dir: string) => {
      if (!isSubdir(pkgsDir, dir)) return manifest
      return updateManifest(workspaceDir, manifest, dir)
    },
    'tsconfig.json': updateTSConfig.bind(null, {
      lockfile,
      workspaceDir,
    }),
  }
}

async function updateTSConfig (
  context: {
    lockfile: Lockfile
    workspaceDir: string
  },
  tsConfig: object,
  dir: string,
  manifest: ProjectManifest
) {
  if (manifest.name === '@pnpm/tsconfig') return tsConfig
  const relative = normalizePath(path.relative(context.workspaceDir, dir))
  const importer = context.lockfile.importers[relative]
  if (!importer) return tsConfig
  const deps = {
    ...importer.dependencies,
    ...importer.devDependencies,
  }
  const references = [] as Array<{ path: string }>
  for (const spec of Object.values(deps)) {
    if (!spec.startsWith('link:') || spec.length === 5) continue
    references.push({ path: spec.substr(5) })
  }
  await writeJsonFile(path.join(dir, 'tsconfig.lint.json'), {
    extends: './tsconfig.json',
    include: [
      'src/**/*.ts',
      'test/**/*.ts',
      '../../typings/**/*.d.ts',
    ],
  }, { indent: 2 })
  return {
    ...tsConfig,
    compilerOptions: {
      ...tsConfig['compilerOptions'],
      rootDir: 'src',
    },
    references: references.sort((r1, r2) => r1.path.localeCompare(r2.path)),
  }
}

let registryMockPort = 7769

async function updateManifest (workspaceDir: string, manifest: ProjectManifest, dir: string) {
  const relative = normalizePath(path.relative(workspaceDir, dir))
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
    scripts = {
      ...(manifest.scripts as Record<string, string>),
      'registry-mock': 'registry-mock',
      'test:jest': 'jest',

      'test:e2e': 'registry-mock prepare && run-p -r registry-mock test:jest',
    }
    scripts.test = 'pnpm run compile && pnpm run _test'
    scripts._test = `cross-env PNPM_REGISTRY_MOCK_PORT=${port} pnpm run test:e2e`
    break
  }
  default:
    if (await exists(path.join(dir, 'test'))) {
      scripts = {
        ...(manifest.scripts as Record<string, string>),
        _test: 'jest',
        test: 'pnpm run compile && pnpm run _test',
      }
    } else {
      scripts = {
        ...(manifest.scripts as Record<string, string>),
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
    scripts.compile += ' && rimraf dist && pnpm run bundle \
&& shx cp -r node-gyp-bin dist/node-gyp-bin \
&& shx cp -r node_modules/@pnpm/tabtab/lib/scripts dist/scripts \
&& shx cp node_modules/ps-list/fastlist.exe dist/fastlist.exe'
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
