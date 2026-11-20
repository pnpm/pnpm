const dependencyPath = require('dependency-path')

const registry = 'https://registry.npmjs.org/'

console.log(dependencyPath.isAbsolute('/foo/1.0.0'))

// it is confusing currently because relative starts with /.
// It will be changed in the future to vice versa
console.log(dependencyPath.resolve(registry, '/foo/1.0.0'))

console.log(dependencyPath.relative(registry, 'registry.npmjs.org/foo/1.0.0'))

console.log(dependencyPath.refToAbsolute('1.0.1', 'foo', registry))

console.log(dependencyPath.refToAbsolute('github.com/foo/bar/twe0jger043t0ew', 'foo', registry))

console.log(dependencyPath.refToRelative('1.0.1', 'foo', registry))

console.log(dependencyPath.parse('/foo/2.0.0'))
