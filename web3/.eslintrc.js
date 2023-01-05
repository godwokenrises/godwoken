
module.exports = {
  "parser": "@typescript-eslint/parser",
  "parserOptions": {
    "sourceType": "module"
  },
  "plugins": [
    "@typescript-eslint"
  ],
  "env": {
    "browser": true,
    "es2020": true
  },
  "globals": {
    "Atomics": "readonly",
    "SharedArrayBuffer": "readonly"
  },
  "rules": {
    "no-var": "error",
    "@typescript-eslint/no-unused-vars": [
      "error",
      {
        "argsIgnorePattern": "^_|^args$",
        "varsIgnorePattern": "^_"
      }
    ]
  }
};
