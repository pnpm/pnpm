#!/usr/bin/env node
import fs from 'fs'
import esbuild from 'esbuild'
import pathLib from 'path'
import childProcess from 'child_process'
import { createRequire } from 'module'
import { findWorkspacePackagesNoCheck } from '@pnpm/workspace.find-packages'
import { findWorkspaceDir } from '@pnpm/find-workspace-dir'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'

const pnpmPackageJson = JSON.parse(fs.readFileSync(pathLib.join(import.meta.dirname, 'package.json'), 'utf8'))

;(async () => {
  const workspaceDir = await findWorkspaceDir(import.meta.dirname)
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)
  const pkgs = await findWorkspacePackagesNoCheck(workspaceDir, { patterns: workspaceManifest.packages })
  const localPackages = pkgs.map(pkg => pkg.manifest.name)
  const dirByPackageName = pkgs.reduce((acc, { manifest, rootDirRealPath }) => {
    acc[manifest.name] = rootDirRealPath
    return acc
  })

  // This plugin rewrites imports to reference the `src` dir instead of `lib` so
  // esbuild can compile the original TypeScript
  const spnpmImportsPlugin = {
    name: 'spnpmImports',
    setup: (build) => {
      // E.g. @pnpm/config -> /<some_dir>/pnpm/packages/config/src/index.ts
      build.onResolve({ filter: /@pnpm\// }, ({ path }) => {
        // Bail if the package isn't present locally
        if (!localPackages.includes(path)) {
          return
        }

        const newPath = pathLib.resolve(dirByPackageName[path], 'src', 'index.ts')
        return {
          path: newPath
        }
      })

      build.onResolve({filter: /js-yaml/}, ({ path, resolveDir }) => {
        if (path === 'js-yaml' && resolveDir.includes('lockfile/fs')) {
          // Force esbuild to use the resolved js-yaml from within lockfile-file,
          // since it seems to pick the wrong one otherwise.
          const lockfileFileProject = pathLib.resolve(import.meta.dirname, '../../lockfile/fs/index.js')
          const resolvedJsYaml = createRequire(lockfileFileProject).resolve('js-yaml')
          return {
            path: resolvedJsYaml
          }
        }
      })
    }
  }

  const banner = { js: `import { createRequire as _cr } from 'module';const require = _cr(import.meta.url); const __filename = import.meta.filename; const __dirname = import.meta.dirname` }
  await esbuild.build({
    entryPoints: [pathLib.resolve(import.meta.dirname, '../../worker/src/worker.ts')],
    bundle: true,
    banner,
    platform: 'node',
    format: 'esm',
    outfile: pathLib.resolve(import.meta.dirname, 'dist/worker.js'),
    loader: {
      '.node': 'copy',
    },
  })

  await esbuild.build({
    bundle: true,
    platform: 'node',
    format: 'esm',
    banner,
    target: 'node14',
    entryPoints: [pathLib.resolve(import.meta.dirname, '../src/pnpm.ts')],
    outfile: pathLib.resolve(import.meta.dirname, 'dist/pnpm.mjs'),
    external: [
      'node-gyp',
      './get-uid-gid.js', // traces back to: https://github.com/npm/uid-number/blob/6e9bdb302ae4799d05abf12e922ccdb4bd9ea023/uid-number.js#L31
    ],
    define: {
      'process.env.npm_package_name': JSON.stringify(pnpmPackageJson.name),
      'process.env.npm_package_version': JSON.stringify(pnpmPackageJson.version),
    },
    sourcemap: true, // nice for local debugging
    logLevel: 'warning', // keeps esbuild quiet unless there's a problem
    plugins: [spnpmImportsPlugin],
    loader: {
      '.node': 'binary',
    }
  })

  const nodeBin = process.argv[0]

  // Invoke the script just built by esbuild, with Node's sourcemaps enabled
  const { status } = childProcess.spawnSync(nodeBin, [
    '--enable-source-maps',
    pathLib.resolve(import.meta.dirname, 'dist/pnpm.mjs'),
    '--config.manage-package-manager-versions=false',
    ...process.argv.slice(2),
  ], {
    stdio: 'inherit',
    env: {
      ...process.env,
      // During local development we don't want to switch to another version of pnpm
      // NOTE: Disabling through env variable stopped working for some reasone!
      // We need to check why. We set it through CLI argument for now.
      npm_config_manage_package_manager_versions: false,
    },
  })
  process.exit(status)
})()
