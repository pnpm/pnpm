module.exports = {
  hooks: {readPackage}
}

function readPackage(pkg) {
  pkg.dependencies = '@oclif/errors'
  return pkg
}
