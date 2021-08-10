import fs from 'fs'
import path from 'path'
import { readWantedLockfile, Lockfile } from '@pnpm/lockfile-file'
import { ProjectManifest } from '@pnpm/types'
import isSubdir from 'is-subdir'
import loadJsonFile from 'load-json-file'
import normalizePath from 'normalize-path'
import exists from 'path-exists'
import writeJsonFile from 'write-json-file'

export default async (workspaceDir: string) => {
  const pnpmManifest = loadJsonFile.sync(path.join(workspaceDir, 'packages/pnpm/package.json'))
  const artifactVersion = `0.0.2-${pnpmManifest!['version']}` // eslint-disable-line
  const pkgsDir = path.join(workspaceDir, 'packages')
  const lockfile = await readWantedLockfile(workspaceDir, { ignoreIncompatible: false })
  if (lockfile == null) {
    throw new Error('no lockfile found')
  }
  return {
    'package.json': (manifest: ProjectManifest, dir: string) => {
      if (!isSubdir(pkgsDir, dir)) {
        return manifest
      }
      if (dir.includes('artifacts') || manifest.name === '@pnpm/beta') {
        manifest.version = artifactVersion
        if (manifest.name === '@pnpm/beta') {
          for (const depName of ['@pnpm/linux-x64', '@pnpm/linuxstatic-x64', '@pnpm/win-x64', '@pnpm/macos-x64', '@pnpm/macos-arm64']) {
            manifest.optionalDependencies![depName] = `workspace:${artifactVersion}`
          }
        }
        return manifest
      }
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
  if (tsConfig == null) return tsConfig
  if (manifest.name === '@pnpm/tsconfig') return tsConfig
  const relative = normalizePath(path.relative(context.workspaceDir, dir))
  const importer = context.lockfile.importers[relative]
  if (!importer) return tsConfig
  const deps = {
    ...importer.dependencies,
    ...importer.devDependencies,
  }
  const references = [] as Array<{ path: string }>
  for (const [depName, spec] of Object.entries(deps)) {
    if (!spec.startsWith('link:') || spec.length === 5) continue
    const relativePath = spec.substr(5)
    if (!await exists(path.join(dir, relativePath, 'tsconfig.json'))) continue
    if (
      depName === '@pnpm/package-store' && (
        manifest.name === '@pnpm/git-fetcher' ||
        manifest.name === '@pnpm/tarball-fetcher' ||
        manifest.name === '@pnpm/package-requester'
      )
    ) {
      // This is to avoid a circular graph (which TypeScript references do not support.
      continue
    }
    references.push({ path: relativePath })
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
    if (manifest.name === '@pnpm/headless') {
      scripts._test = `ts-node test/pretest && ${scripts._test}`
    }
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
  if (scripts._test) {
    if (scripts.pretest) {
      scripts._test = `pnpm pretest && ${scripts._test}`
    }
    if (scripts.posttest) {
      scripts._test = `${scripts._test} && pnpm posttest`
    }
  }
  scripts.compile = 'rimraf lib tsconfig.tsbuildinfo && tsc --build && pnpm run lint -- --fix'
  delete scripts.tsc
  let homepage: string
  let repository: string | { type: 'git', url: string }
  if (manifest.name === 'pnpm') {
    homepage = 'https://pnpm.io'
    repository = {
      type: 'git',
      url: 'git+https://github.com/pnpm/pnpm.git',
    }
    scripts.compile += ' && rimraf dist bin/nodes && pnpm run bundle \
&& shx cp -r node-gyp-bin dist/node-gyp-bin \
&& shx cp -r node_modules/@pnpm/tabtab/lib/scripts dist/scripts \
&& shx cp -r node_modules/ps-list/vendor dist/vendor \
&& pkg ./dist/pnpm.cjs --out-path=../artifacts/win-x64 --targets=node14-win-x64 \
&& pkg ./dist/pnpm.cjs --out-path=../artifacts/linux-x64 --targets=node14-linux-x64 \
&& pkg ./dist/pnpm.cjs --out-path=../artifacts/linuxstatic-x64 --targets=node14-linuxstatic-x64 \
&& pkg ./dist/pnpm.cjs --out-path=../artifacts/macos-x64 --targets=node14-macos-x64 \
&& pkg ./dist/pnpm.cjs --out-path=../artifacts/macos-arm64 --targets=node14-macos-arm64'
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
  if (manifest.name === 'pnpm' || manifest.name?.endsWith('/pnpm')) {
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
    bugs: {
      url: 'https://github.com/pnpm/pnpm/issues',
    },
    engines: {
      node: '>=12.17',
    },
    files,
    funding: 'https://opencollective.com/pnpm',
    homepage,
    license: 'MIT',
    repository,
    scripts,
  }
}
