module.exports = {
  hooks: { readPackage }
}

function readPackage(pkg) {
  if (pkg.name === 'foo') {
    pkg.dependencies = { bar: '100.0.0' };
  }
  return pkg;
}