const baseConfig = require('../jest-with-registry.config.js')

module.exports = {
  ...baseConfig,

  // The preset option is set to "ts-jest" in the base config. The ts-jest docs
  // recommend not using a preset if the transform block is configured.
  //
  // https://kulshekhar.github.io/ts-jest/docs/getting-started/presets
  preset: undefined,

  transform: {
    ...baseConfig.transform,

    // Point towards tsconfig.test.json to fix code blocks.
    '^.+\\.tsx?$': ['ts-jest', { tsconfig: "tsconfig.test.json" }],
  },
}
