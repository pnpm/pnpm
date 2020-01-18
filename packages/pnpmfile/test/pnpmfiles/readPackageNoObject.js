module.exports = {
  hooks: {readPackage}
}

function readPackage(pkg) {
  pkg.dependencies = ''
  return pkg
}
