module.exports = {
  hooks: { readPackage },
}

function readPackage (pkg) {
  pkg.devDependencies = '@oclif/errors'
  return pkg
}
