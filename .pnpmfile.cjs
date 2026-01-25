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
      // The main pnpm package bundles its dependencies before publishing.
      // Delete dependency fields from the manifest so these dependencies are
      // downloaded twice.
      if (manifest.name === 'pnpm') {
        delete manifest.dependencies
        delete manifest.devDependencies
        delete manifest.optionalDependencies
      }

      return manifest
    }
  }
}
