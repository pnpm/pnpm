module.exports = {
  hooks: { readPackage },
}

function readPackage (pkg) {
  pkg.dependencies['is-positive'] = undefined
  return pkg
}
