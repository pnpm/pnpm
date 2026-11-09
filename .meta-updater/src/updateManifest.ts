import fs from 'node:fs'
import path from 'node:path'

import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import type { ProjectManifest } from '@pnpm/types'
import normalizePath from 'normalize-path'

const CLI_PKG_NAME = 'pnpm'

export const PUBLISH_EXECUTABLE_FILES = [
  './dist/node-gyp-bin/node-gyp',
  './dist/node-gyp-bin/node-gyp.cmd',
  './dist/node_modules/node-gyp/bin/node-gyp.js',
  './dist/node_modules/node-gyp/gyp/gyp_main.py',
  './dist/node_modules/node-gyp/gyp/gyp',
]

const PKGS_NEEDING_CLI_COMPILE = new Set([
  '@pnpm/building.commands',
  '@pnpm/cache.commands',
  '@pnpm/deps.inspection.commands',
  '@pnpm/exec.commands',
  '@pnpm/lockfile.make-dedicated-lockfile',
  '@pnpm/releasing.commands',
  '@pnpm/releasing.exportable-manifest',
  '@pnpm/store.commands',
])

const PKGS_WITHOUT_TEST_COMMANDS = new Set([
  '@pnpm/bins.cmd-shim',
  '@pnpm/lockfile.types',
])

const PKGS_WITH_REGISTRY_TESTS = new Set([
  '@pnpm/building.commands',
  '@pnpm/installing.deps-restorer',
  '@pnpm/installing.env-installer',
  '@pnpm/deps.inspection.outdated',
  '@pnpm/installing.package-requester',
  '@pnpm/cache.commands',
  '@pnpm/plugin-commands-import',
  '@pnpm/installing.commands',
  '@pnpm/deps.inspection.commands',
  '@pnpm/patching.commands',
  '@pnpm/registry-access.commands',
  '@pnpm/releasing.commands',
  '@pnpm/exec.commands',
  '@pnpm/store.commands',
  '@pnpm/deps.compliance.commands',
  CLI_PKG_NAME,
  '@pnpm/installing.deps-installer',
])

const priority = Object.fromEntries([
  'name', 'private', 'version', 'description', 'keywords', 'license', 'author', 'contributors',
  'funding', 'repository', 'homepage', 'bugs', 'type', 'main', 'types', 'module', 'browser',
  'exports', 'files', 'bin', 'man', 'directories', 'unpkg', 'scripts', 'config', 'dependencies',
  'peerDependencies', 'optionalDependencies', 'devDependencies', 'bundledDependencies', 'engines',
  'os', 'cpu', 'pnpm', 'packageManager',
].map((key, index) => [key, index]))

export function updateManifest (
  workspaceDir: string,
  manifest: ProjectManifest,
  dir: string,
  nextTag: string
): ProjectManifest {
  const relative = normalizePath(path.relative(workspaceDir, dir))
  const { preset, scripts } = createScripts(manifest, dir, nextTag)
  const { homepage, repository } = createRepository(manifest, relative)
  const files = createFiles(manifest)
  moveRamdaTypeDependency(manifest)
  addJestPreset(manifest, scripts, preset)
  return sortKeysByPriority({ priority }, {
    ...manifest,
    type: 'module',
    bugs: { url: 'https://github.com/pnpm/pnpm/issues' },
    engines: { node: '>=22.13' },
    files,
    funding: 'https://opencollective.com/pnpm',
    homepage,
    license: manifest.license ?? 'MIT',
    repository,
    scripts,
    exports: {
      ...manifest.exports,
      '.': manifest.name === CLI_PKG_NAME ? './package.json' : './lib/index.js',
    },
  })
}

function createScripts (
  manifest: ProjectManifest,
  dir: string,
  nextTag: string
): { preset: string, scripts: Record<string, string> } {
  const scripts = { ...manifest.scripts } as Record<string, string>
  let preset = '@pnpm/jest-config'
  if (PKGS_WITH_REGISTRY_TESTS.has(manifest.name ?? '')) {
    preset = '@pnpm/jest-config/with-registry'
    setRegistryTestScripts(manifest, scripts)
  } else if (!PKGS_WITHOUT_TEST_COMMANDS.has(manifest.name ?? '')) {
    setDefaultTestScripts(manifest, scripts, dir)
  }
  setCompileScripts(manifest, scripts)
  setPublishScripts(manifest, scripts, nextTag)
  setLintScripts(manifest, scripts, dir)
  wrapTestScripts(manifest, scripts)
  return { preset, scripts }
}

function setRegistryTestScripts (manifest: ProjectManifest, scripts: Record<string, string>): void {
  scripts.test = 'pn compile && pn .test'
  if (manifest.name === '@pnpm/installing.deps-installer') {
    const testEnv = 'cross-env PNPM_REGISTRY_MOCK_PORT=7769 NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169"'
    scripts['.test'] = 'pn .test:heavy && pn .test:rest'
    scripts['.test:heavy'] = `${testEnv} jest test/install/deepRecursive.ts`
    scripts['.test:rest'] = `${testEnv} jest "^(?!.*deepRecursive)"`
  } else {
    scripts['.test'] = 'cross-env NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169" jest'
  }
}

function setDefaultTestScripts (manifest: ProjectManifest, scripts: Record<string, string>, dir: string): void {
  if (!fs.existsSync(path.join(dir, 'test'))) {
    scripts.test = 'pn compile'
    return
  }
  scripts['.test'] = 'cross-env NODE_OPTIONS="$NODE_OPTIONS --experimental-vm-modules --disable-warning=ExperimentalWarning --disable-warning=DEP0169" jest'
  scripts.test = 'pn compile && pn .test'
}

function wrapTestScripts (manifest: ProjectManifest, scripts: Record<string, string>): void {
  if (!scripts['.test']) return
  if (manifest.scripts?.pretest) scripts['.test'] = `pn pretest && ${scripts['.test']}`
  if (manifest.scripts?.posttest) scripts['.test'] += ' && pn posttest'
  if (manifest.name === '@pnpm/server') scripts['.test'] += ' --detectOpenHandles'
}

function setCompileScripts (manifest: ProjectManifest, scripts: Record<string, string>): void {
  if (manifest.name && PKGS_NEEDING_CLI_COMPILE.has(manifest.name)) {
    scripts.test = 'pn compile && pn --filter=pnpm compile && pn .test'
  }
  delete scripts._test
  delete scripts._compile
  scripts.compile = 'tsgo --build && pn lint --fix'
  delete scripts.tsc
  if (scripts.start?.includes('tsc --watch')) scripts.start = scripts.start.replace('tsc --watch', 'tsgo --watch')
  if (scripts['.compile']?.includes('tsc --build')) scripts['.compile'] = scripts['.compile'].replace('tsc --build', 'tsgo --build')
}

function setLintScripts (manifest: ProjectManifest, scripts: Record<string, string>, dir: string): void {
  if (!scripts.lint) return
  scripts.lint = fs.existsSync(path.join(dir, 'test')) && manifest.name !== '@pnpm/bins.cmd-shim'
    ? 'eslint "src/**/*.ts" "test/**/*.ts"'
    : 'eslint "src/**/*.ts"'
}

function setPublishScripts (
  manifest: ProjectManifest,
  scripts: Record<string, string>,
  nextTag: string
): void {
  if (manifest.name === CLI_PKG_NAME) {
    manifest.publishConfig!.tag = nextTag
    manifest.publishConfig!.executableFiles = [...PUBLISH_EXECUTABLE_FILES]
    scripts.build = 'tsgo --build && rimraf dist bin/nodes && pn bundle \
&& shx cp -r node-gyp-bin dist/node-gyp-bin \
&& shx cp -r node_modules/@pnpm/tabtab/lib/templates dist/templates \
&& shx cp -r node_modules/ps-list/vendor dist/vendor \
&& shx cp pnpmrc dist/pnpmrc'
    scripts.compile = 'pn lint --fix && pn build'
    scripts.prepublishOnly = 'pn build && node bundle-deps.ts'
    return
  }
  scripts.prepublishOnly = 'tsgo --build'
}

function createRepository (manifest: ProjectManifest, relative: string): {
  homepage: string
  repository: string | { type: 'git', url: string, directory: 'pnpm11/pnpm' }
} {
  if (manifest.name === CLI_PKG_NAME) {
    return {
      homepage: 'https://pnpm.io',
      repository: {
        type: 'git',
        url: 'git+https://github.com/pnpm/pnpm.git',
        directory: 'pnpm11/pnpm',
      },
    }
  }
  const repoPath = `https://github.com/pnpm/pnpm/tree/main/${relative}`
  return { homepage: `${repoPath}#readme`, repository: repoPath }
}

function createFiles (manifest: ProjectManifest): string[] {
  if (manifest.name === CLI_PKG_NAME || manifest.name?.endsWith('/pnpm')) {
    return ['dist', '!dist/**/*.map', 'bin']
  }
  const files = ['lib', '!*.map']
  if (manifest.bin) files.push('bin')
  if (manifest.name === '@pnpm/exec.npm-lifecycle') files.push('node-gyp-bin')
  return files
}

function moveRamdaTypeDependency (manifest: ProjectManifest): void {
  if (!manifest.dependencies?.['@types/ramda']) return
  manifest.devDependencies = {
    ...manifest.devDependencies,
    '@types/ramda': manifest.dependencies['@types/ramda'],
  }
  delete manifest.dependencies['@types/ramda']
}

function addJestPreset (manifest: ProjectManifest, scripts: Record<string, string>, preset: string): void {
  if (!scripts.test || manifest.name === '@pnpm/bins.cmd-shim') return
  Object.assign(manifest, { jest: { ...(manifest as any).jest, preset } }) // eslint-disable-line
}

export function sortKeysInManifest (manifest: ProjectManifest): ProjectManifest {
  return sortKeysByPriority({ priority }, manifest)
}
