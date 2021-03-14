module.exports = {
  hooks: {
    readPackage (pkg) {
      switch (pkg.name) {
      case '@babel/parser':
        pkg.peerDependencies['@babel/types'] = '*'
        break
      case 'jest-circus':
        pkg.dependencies['slash'] = '3'
        break
      }
      if (typeof pkg.repository === 'string' && pkg.repository.startsWith('https://github.com/pnpm/pnpm/')) {
        pkg.devDependencies[pkg.name] = 'link:'
      }
      return pkg
    }
  }
}
