// In-house bundler config for qfs-viewer. A single
// entry: the domain barrel. Externals (the plgg family +
// node:*) are derived from package.json, so the npm-only
// dependency contract is what controls bundling.
export default {
  root: import.meta.dirname,
  rootDir: "src",
  outDir: "dist",
  entries: [{ name: "index", input: "index.ts" }],
  formats: ["es", "cjs"],
  fileNamePattern: "[name].[format].js",
  // The self-alias, which must agree with package.json's
  // `imports` map (`#qfs-viewer/*` -> `./src/*.ts`).
  // The bundler cannot read that map itself, so this is
  // the one place the alias is restated — and it is
  // restated as a VALUE the bundler needs rather than as
  // a second resolution rule that could disagree at
  // runtime, which is what the old tsconfig `paths` entry
  // was.
  alias: {
    prefix: "#qfs-viewer",
    srcRoot: "src",
  },
};
