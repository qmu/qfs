// In-house bundler config for the cluster console. The
// `"app"` target: ONE self-contained `es` bundle with the
// plgg family (plgg, plgg-view) inlined, since
// a browser cannot resolve a bare `import "plgg"`.
// `scripts/build-cluster-console.sh` then wraps it in the
// single `index.html` the qfs binary embeds.
export default {
  target: "app",
  root: import.meta.dirname,
  rootDir: "src",
  outDir: "dist",
  entries: [
    {
      name: "main",
      input: "entrypoints/main.ts",
    },
  ],
  formats: ["es"],
  fileNamePattern: "[name].[format].js",
  alias: {
    prefix: "#cluster-console",
    srcRoot: "src",
  },
};
