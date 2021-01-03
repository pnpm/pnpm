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

async function updater (
  repoRoot: string,
  opts: {
    filter?: (manifest: ProjectManifest, dir: string) => boolean
    update: Record<string, (obj: object, dir: string, manifest: ProjectManifest) => object | Promise<object>>
  }
) {
  let pkgs = await findWorkspacePackages(repoRoot, { engineStrict: false })
  if (opts.filter) {
    pkgs = pkgs.filter((pkg) => opts.filter(pkg.manifest, pkg.dir))
  }
  for (const { dir, manifest, writeProjectManifest } of pkgs) {
    for (const [p, updateFn] of Object.entries(opts.update)) {
      if (p === 'package.json') {
        await writeProjectManifest(await updateFn(manifest, dir, manifest))
        continue
      }
      if (!p.endsWith('.json')) continue
      const fp = path.join(dir, p)
      if (!await exists(fp)) {
        continue
      }
      const obj = await loadJsonFile(fp)
      await writeJsonFile(fp, await updateFn(obj as object, dir, manifest), { detectIndent: true })
    }
  }
}

// eslint-disable-next-line @typescript-eslint/no-floating-promises
; (async () => {
  const pkgsDir = path.join(repoRoot, 'packages')
  const lockfile = await readWantedLockfile(repoRoot, { ignoreIncompatible: false })
  await updater(repoRoot, {
    update: {
      'package.json': (manifest, dir) => {
        if (!isSubdir(pkgsDir, dir)) return manifest
        return updateManifest(manifest, dir)
      },
      'tsconfig.json': async (tsConfig, dir, manifest) => {
        if (manifest.name === '@pnpm/tsconfig') return tsConfig
        const relative = path.relative(repoRoot, dir)
        const importer = lockfile.importers[relative]
        if (!importer) return
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
      },
    },
  })
})()

let registryMockPort = 7769

async function updateManifest (manifest: ProjectManifest, dir: string) {
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
    scripts = {
      ...manifest.scripts,
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
        ...manifest.scripts,
        _test: 'jest',
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
