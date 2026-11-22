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
    }
  }
}
