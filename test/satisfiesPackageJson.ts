import test = require('tape')
import {satisfiesPackageJson} from 'pnpm-shrinkwrap'

test('satisfiesPackageJson()', t => {
  t.ok(satisfiesPackageJson({dependencies: {foo: '1.0.0'}, specifiers: {foo: '^1.0.0'}}, {dependencies: {foo: '^1.0.0'}}))
  t.ok(satisfiesPackageJson({dependencies: {foo: '1.0.0'}, devDependencies: {}, specifiers: {foo: '^1.0.0'}}, {dependencies: {foo: '^1.0.0'}}))
  t.ok(satisfiesPackageJson({devDependencies: {foo: '1.0.0'}, specifiers: {foo: '^1.0.0'}}, {devDependencies: {foo: '^1.0.0'}}))
  t.ok(satisfiesPackageJson({optionalDependencies: {foo: '1.0.0'}, specifiers: {foo: '^1.0.0'}}, {optionalDependencies: {foo: '^1.0.0'}}))
  t.notOk(satisfiesPackageJson({dependencies: {foo: '1.0.0'}, specifiers: {foo: '^1.0.0'}}, {optionalDependencies: {foo: '^1.0.0'}}), 'dep type differs')
  t.notOk(satisfiesPackageJson({dependencies: {foo: '1.0.0'}, specifiers: {foo: '^1.0.0'}}, {dependencies: {foo: '^1.1.0'}}), 'spec does not match' )
  t.notOk(satisfiesPackageJson({dependencies: {foo: '1.0.0'}, specifiers: {foo: '^1.0.0'}}, {dependencies: {foo: '^1.0.0', bar: '2.0.0'}}), 'dep spec missing')
  t.end()
})
