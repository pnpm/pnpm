#!/usr/bin/env node
const fs = require('fs')
const esbuild = require('esbuild')
const pathLib = require('path')
const { findWorkspacePackagesNoCheck } = require('@pnpm/workspace.find-packages')
const { findWorkspaceDir } = require('@pnpm/find-workspace-dir')

const pnpmPackageJson = JSON.parse(fs.readFileSync(pathLib.join(__dirname, 'package.json'), 'utf8'))

;(async () => {
  const workspaceDir = await findWorkspaceDir(__dirname)
  const pkgs = await findWorkspacePackagesNoCheck(workspaceDir)
  const localPackages = pkgs.map(pkg => pkg.manifest.name)
  const dirByPackageName = pkgs.reduce((acc, pkg) => {
    acc[pkg.manifest.name] = pkg.dir
    return acc
  })

  // This plugin rewrites imports to reference the `src` dir instead of `lib` so
  // esbuild can compile the original TypeScript
  const spnpmImportsPlugin = {
    name: 'spnpmImports',
    setup: (build) => {
      // E.g. @pnpm/config -> /<some_dir>/pnpm/packages/config/src/index.ts
      build.onResolve({ filter: /@pnpm\// }, ({ path, resolveDir }) => {
        // Bail if the package isn't present locally
        if (!localPackages.includes(path)) {
          return
        }

        const newPath = pathLib.resolve(dirByPackageName[path], 'src', 'index.ts')
        return {
          path: newPath
        }
      })
    }
  }

  await esbuild.build({
    entryPoints: [pathLib.resolve(__dirname, '../../worker/src/worker.ts')],
    bundle: true,
    platform: 'node',
    outfile: pathLib.resolve(__dirname, 'dist/worker.js'),
    loader: {
      '.node': 'copy',
    },
  })

  await esbuild.build({
    bundle: true,
    platform: 'node',
    target: 'node14',
    entryPoints: [pathLib.resolve(__dirname, '../src/pnpm.ts')],
    outfile: pathLib.resolve(__dirname, 'dist/pnpm.cjs'),
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

  // Require the file just built by esbuild
  require('./dist/pnpm.cjs')
})()
