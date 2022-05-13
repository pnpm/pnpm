#!/usr/bin/env node
const fs = require('fs')
const esbuild = require('esbuild')
const pathLib = require('path')
const packagesDir = pathLib.resolve(__dirname, '../..')

const localPackages = fs.readdirSync(packagesDir)

const pnpmPackageJson = JSON.parse(fs.readFileSync(pathLib.join(__dirname, 'package.json'), 'utf8'))

// This plugin rewrites imports to reference the `src` dir instead of `lib` so
// esbuild can compile the original TypeScript
const spnpmImportsPlugin = {
  name: 'spnpmImports',
  setup: (build) => {
    // This is an exception to the rule that all local packages start with `@pnpm`
    build.onResolve({ filter: /^dependency-path$/ }, ({path, resolveDir}) => ({
      path: pathLib.resolve(packagesDir, 'dependency-path', 'src', 'index.ts')
    }))

    // E.g. @pnpm/config -> /<some_dir>/pnpm/packages/config/src/index.ts
    build.onResolve({ filter: /@pnpm\// }, ({path, resolveDir}) => {
      const pathParts = path.split('/')
      const packageName = pathParts[1]

      // Bail if the package isn't present locally
      if (!localPackages.includes(packageName)) {
        return
      }

      const newPath = pathLib.resolve(packagesDir, packageName, 'src', 'index.ts')

      return {
        path: newPath
      }
    })
  }
}

;(async () => {
  await esbuild.build({
    bundle: true,
    platform: 'node',
    target: 'node14',
    entryPoints: [pathLib.resolve(__dirname, '../lib/pnpm.js')],
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
  })

  // Require the file just built by esbuild
  require('./dist/pnpm.cjs')
})()
