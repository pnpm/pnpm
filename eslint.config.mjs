import eslintConfig from "@pnpm/eslint-config";

export default [{
    ignores: ["**/fixtures", "**/__fixtures__"],
}, ...eslintConfig];
