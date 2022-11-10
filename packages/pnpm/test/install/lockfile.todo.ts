
import path from 'path'
import * as TJS from 'typescript-json-schema'
import readYamlFile from 'read-yaml-file'
import { Lockfile } from '@pnpm/lockfile-types'
import { createRequire } from 'module';

(async () => {

  const lockfilePath = '/home/user/src/javascript/nodejs-lockfile-parser/nodejs-lockfile-parser/test/fixtures/goof/pnpm-lock.v6.yaml'
  const lockfile = await readYamlFile<Lockfile>(lockfilePath)
  //console.dir(lockfile);

  // optionally pass argument to schema generator
  const settings: TJS.PartialArgs = {
    required: true,
  };

  // optionally pass ts compiler options
  const compilerOptions: TJS.CompilerOptions = {
    strictNullChecks: true,
  };

  // optionally pass a base path
  const basePath = "./my-dir";

  //../lockfile-types/lib/index.d.ts
  console.log(path.resolve("@pnpm/lockfile-types/lib/index.d.ts"))
  console.log(path.resolve("../../../lockfile-types/lib/index.d.ts"))

  const dependencyAsset = await import.meta.resolve('component-lib/asset.js');

  const require = createRequire(import.meta.url);
  const pathName = require.resolve('vue.runtime.esm.js');



  const program = TJS.getProgramFromFiles(
    [path.resolve("my-file.ts")],
    compilerOptions,
    basePath
  );

  // We can either get the schema for one file and one type...
  const schema = TJS.generateSchema(program, "MyType", settings);

  // ... or a generator that lets us incrementally get more schemas

  const generator = TJS.buildGenerator(program, settings);

  // generator can be also reused to speed up generating the schema if usecase allows:
  const schemaWithReusedGenerator = TJS.generateSchema(program, "MyType", settings, [], generator);

  // all symbols
  const symbols = generator.getUserSymbols();

  // Get symbols for different types from generator.
  generator.getSchemaForSymbol("MyType");
  generator.getSchemaForSymbol("AnotherType");

})()
