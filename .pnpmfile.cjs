module.exports = {
  hooks: {
    readPackage (pkg) {
      if (typeof pkg.repository === 'string' && pkg.repository.startsWith('https://github.com/pnpm/pnpm/')) {
        pkg.devDependencies[pkg.name] = 'link:'
      }
      return pkg
    }
  }
}
