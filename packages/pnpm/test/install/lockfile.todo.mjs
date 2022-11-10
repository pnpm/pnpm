// validate pnpm lockfile
//
// usage: pass lockfile-path as first argument
// default value: pnpm's own lockfile: pnpm/pnpm-lock.yaml
//
// the script returns 0 on success, 1 on error

import path from 'path'
import fs from 'fs'
import { createRequire } from 'module';
const require = createRequire(import.meta.url);

// pnpm/packages/pnpm/test/install/lockfile.todo.mjs
// pnpm install -D ts-json-schema-generator ajv
//

import * as Tsj from 'ts-json-schema-generator'
//import * as Tjs_broken from 'typescript-json-schema' // -> invalid schema
const use_Tjs_broken = false;
//const use_Tjs_broken = true; // -> invalid schema

import Ajv from "ajv"
//import addFormats from 'ajv-formats' // addFormats(ajv)

import readYamlFile from 'read-yaml-file'
/*import { Lockfile } from '@pnpm/lockfile-types'*/

(async () => {

  //const lockfilePath = '/tmp/pnpm-lock.yaml'
  //const lockfilePath = path.resolve(path.dirname(process.argv[1]), '../../../../pnpm-lock.yaml'); // workspace lockfile
  //const lockfilePath = '/home/user/src/javascript/pnpm/git/pnpm/pnpm-lock.yaml'
  //const lockfilePath = process.argv[2];

  const lockfilePath = process.argv[2] || path.resolve(path.dirname(process.argv[1]), '../../../../pnpm-lock.yaml'); // workspace lockfile

  console.log(`lockfilePath = ${lockfilePath}`);
  const lockfile = await readYamlFile/*<Lockfile>*/(lockfilePath)
  //console.dir(lockfile);

  if (lockfile == undefined) {
    console.log(`empty lockfile: ${lockfilePath}`);
    process.exit(0);
  }

  const lockfileVersionMajor = (lockfile.lockfileVersion || lockfile.shrinkwrapVersion || lockfile.version) | 0;
  const lockfileVersionMinor = lockfile.lockfileVersion ? (lockfile.lockfileVersion.toString().split('.')[1] || 0) : (lockfile.shrinkwrapMinorVersion || 0);
  /*
  // lockfileVersionString always has minor version
  const lockfileVersionString = `${lockfileVersionMajor}.${lockfileVersionMinor}`;
  const lockfileVersion = parseFloat(`${lockfileVersionMajor}.${lockfileVersionMinor}`);
  */
  // lockfileVersionString has minor version only when minor > 0
  // same format as in pnpm-lock.yaml
  const lockfileVersion = parseFloat(`${lockfileVersionMajor}.${lockfileVersionMinor}`);
  const lockfileVersionString = `${lockfileVersion}`;
  console.log(`lockfileVersion = ${lockfileVersionString}`);

  //const { version: lockfileTypesVersion } = JSON.parse(fs.readFileSync(require.resolve('@pnpm/lockfile-types/package.json')));
  //const lockfileTypesVersionMajor = parseInt(lockfileTypesVersion.split('.')[0]);

  //const typesPath = "src/index.ts"; // latest version
  //const typesPath = "src/version/5.3.ts";
  const typesPath = `src/version/${lockfileVersionString}.ts`;

  const lockfileTypesPath = require.resolve(`@pnpm/lockfile-types/${typesPath}`);
  if (!lockfileTypesPath) {
    throw new Error(`no found @pnpm/lockfile-types/${typesPath}`);
  }
  const lockfileTypesSource = fs.readFileSync(require.resolve(`@pnpm/lockfile-types/${typesPath}`), "utf8");
  const lockfileTypesVersion = parseFloat(lockfileTypesSource.match(/const lockfileVersion = ([0-9.]+)[;\n]/)[1]);

  const cacheFilePath = path.resolve(path.dirname(process.argv[1]),  `pnpm-lockfile-schema-v${lockfileVersionString}.json`);

  //const lockfileTypesVersionMajor = lockfileTypesVersion | 0; // float -> int

  if (lockfileVersion != lockfileTypesVersion) {
    //throw new Error(
    console.log(
      `version mismatch. types ${lockfileTypesVersion}, lockfile ${lockfileVersion}`
    );
  }

  const schema = (() => {

    if (fs.existsSync(cacheFilePath)) {
      console.log(`using cached schema ${cacheFilePath}`);
      const json = fs.readFileSync(cacheFilePath, "utf8");
      return JSON.parse(json);
    }

    console.log(`generating schema ${cacheFilePath} from @pnpm/lockfile-types ... this may take a minute`);

    //const lockfileTypesPath = require.resolve("@pnpm/lockfile-types/lib/index.d.ts")
    //const lockfileTypesPath = require.resolve("@pnpm/lockfile-types/src/index.ts") // not in the comiled version
    //const lockfileTypesPath = path.resolve(path.dirname(process.argv[1]), '../../../lockfile-types/src/index.ts');
    //const lockfileTypesPath = path.resolve(path.dirname(process.argv[1]), '../../../lockfile-types/src/version/5.3.ts');
    const lockfileTypesPath = path.resolve(path.dirname(process.argv[1]), `../../../lockfile-types/src/version/${lockfileVersionString}.ts`);
    console.log(`lockfileTypesPath = ${lockfileTypesPath}`)

    let schema;

    if (use_Tjs_broken == false) {
      // https://github.com/vega/ts-json-schema-generator#programmatic-usage

      const config = {
        //path: require.resolve("@pnpm/lockfile-types/lib/index.d.ts"),
        path: require.resolve(`@pnpm/lockfile-types/${typesPath}`),
        tsconfig: require.resolve("@pnpm/lockfile-types/tsconfig.json"),
        type: "Lockfile", // * or <type-name> if you want to generate schema for that one type only
      };
      var t1 = performance.now();
      schema = Tsj.createGenerator(config).createSchema(config.type);
      var t2 = performance.now();
      var dt = (t2 - t1) / 1000;
      console.log(`schema was generated in ${dt} seconds`) // 80 seconds
    }

    else {
      schema = getSchemaTjs_broken();
    }

    fs.writeFileSync(cacheFilePath, JSON.stringify(schema, null, 2), "utf8");
    console.log(`done ${cacheFilePath}`)

    return schema;
  })();

  console.log('new Ajv')
  const ajv = new Ajv({
    strict: true,
    allErrors: true,
  })

  console.log('ajv.validateSchema')
  ajv.validateSchema(schema);
  if (ajv.errors) {
    console.dir(ajv.errors, { depth: null });
    throw new Error("The schema is not valid");
  }

  console.log('ajv.compile')
  const validate = ajv.compile(schema)

  console.log('validate')
  const valid = validate(lockfile)

  function unescapeJsonPointer(str) {
    return str.replace(/~1/g, "/").replace(/~0/g, "~")
  }

  if (!valid) {
    console.log(
      validate.errors
      .slice(0, 20) // show only the first 20 errors
      .map(err => {
        // split path to array
        // example
        // in: /dependenciesMeta/@react-spring~1core
        // out: [ 'dependenciesMeta', '@react-spring/core' ]
        err.instancePath = err.instancePath.slice(1).split("/").map(unescapeJsonPointer);
        return err;
      })
    );
    process.exit(1);
  }

})()



function getSchemaTjs_broken() {

  // https://github.com/YousefED/typescript-json-schema/blob/master/test/schema.test.ts

  // optionally pass argument to schema generator
  const settings /*: Tjs_broken.PartialArgs*/ = {
    //required: true,
  };

  // optionally pass ts compiler options
  const compilerOptions/*: Tjs_broken.CompilerOptions*/ = {
    //strictNullChecks: true,
  };

  // optionally pass a base path
  //const basePath = "./my-dir";

  //../lockfile-types/lib/index.d.ts
  /*
  console.log(path.resolve("@pnpm/lockfile-types/lib/index.d.ts"))
  console.log(path.resolve("../../../lockfile-types/lib/index.d.ts"))
  */

  console.log('Tjs_broken.getProgramFromFiles')
  const program = Tjs_broken.getProgramFromFiles(
    [lockfileTypesPath],
    compilerOptions,
    //basePath
  );

  // We can either get the schema for one file and one type...
  // TODO cache schema. Tjs_broken.generateSchema is slow
  console.log('Tjs_broken.generateSchema')
  const schema = Tjs_broken.generateSchema(program, "Lockfile", settings);
  return schema;
}
