// In-house bundler config for plggmatic. ESM-only output:
// the package declares `"type": "module"`, so a `.cjs.js`
// sibling would be mis-read as ESM by Node. The style
// entry's OUTPUT KEY is `styleEntry` (not `style`) so
// `dist/styleEntry.*` cannot case-collide with a future
// `dist/Style/` declaration tree on a case-insensitive
// filesystem; the published `./style` subpath points at
// it. Externals (the plgg family + node:*) are derived
// from package.json, never listed here.
export default {
  root: import.meta.dirname,
  rootDir: "src",
  outDir: "dist",
  entries: [
    { name: "index", input: "index.ts" },
    {
      name: "styleEntry",
      input: "styleEntry.ts",
    },
  ],
  formats: ["es"],
  fileNamePattern: "[name].[format].js",
  alias: {
    prefix: "plggmatic",
    srcRoot: "src",
  },
};
