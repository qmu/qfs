// Wrap the app bundle in one self-contained HTML page.
// `</script` inside the module text is escaped so the
// inline <script> cannot be closed early.
import {
  readFileSync,
  writeFileSync,
} from "node:fs";

const [input, output] = process.argv.slice(2);
if (input === undefined || output === undefined) {
  throw new Error(
    "usage: wrap-html.mjs <bundle.js> <index.html>",
  );
}
const js = readFileSync(input, "utf8").replace(
  /<\/script/gi,
  "<\\/script",
);
const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>qfs cluster console</title>
</head>
<body>
<div id="root"></div>
<script type="module">
${js}
</script>
</body>
</html>
`;
writeFileSync(output, html);
