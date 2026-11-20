module.exports = {
  hooks: {
    readPackage: (pkg) => {
      if (pkg.name === '@pnpm.e2e/foo') {
        pkg.dependencies = { ...pkg.dependencies, '@pnpm.e2e/bar': '100.0.0' }
      }
      return pkg
    },
  },
}
