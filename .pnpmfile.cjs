module.exports = {
  hooks: {
    readPackage: (manifest) => {
      if (manifest.name === '@reflink/reflink') {
        for (const depName of Object.keys(manifest.optionalDependencies)) {
          // We don't need refclone on Linux as Node.js supports reflinks out of the box on Linux
          if (depName.includes('linux')) {
            delete manifest.optionalDependencies[depName]
          }
        }
      }
      return manifest
    },

    beforePacking: (manifest) => {
      // The TypeScript pnpm CLI (v11 and older) bundles its dependencies into
      // dist/ before publishing, so the dependency fields are dropped from the
      // published manifest to avoid installing them a second time. From v12 the
      // `pnpm` name is the Rust wrapper, whose manifest is generated exactly by
      // pacquet/npm/pnpm/scripts/generate-packages.mjs (including intentional
      // optionalDependencies on the natives) and must pass through untouched.
      // Which dependency fields the TS package may declare is enforced by
      // .meta-updater/src/index.ts, not here.
      if (manifest.name === 'pnpm' && parseInt(manifest.version, 10) < 12) {
        delete manifest.dependencies
        delete manifest.devDependencies
      }

      return manifest
    }
  }
}
