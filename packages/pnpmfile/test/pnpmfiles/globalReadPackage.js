module.exports = {
  hooks: { readPackage }
}

function readPackage (pkg) {
  pkg.global = true
  return pkg
}
