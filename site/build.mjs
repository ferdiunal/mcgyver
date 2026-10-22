// The site as served: src/ copied to dist/, its scripts and stylesheets minified on the way.
// Wrangler runs this before `dev` and `deploy` (wrangler.jsonc "build").
import { cp, readdir, rm, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const root = fileURLToPath(new URL(".", import.meta.url));
const minified = /\.(js|css)$/;
// Dot files and folders never ship: tools leave theirs (.omc, .DS_Store) wherever they run.
const hidden = /(^|\/)\.[^/]*$/;

await rm(`${root}dist`, { recursive: true, force: true });
await cp(`${root}src`, `${root}dist`, { recursive: true, filter: (path) => !minified.test(path) && !hidden.test(path) });
// And should one land in dist/ after the build, Wrangler leaves it out of the upload.
await writeFile(`${root}dist/.assetsignore`, ".*\n");
// Every script and stylesheet in src/, so none is left out of dist/ by the copy above.
const sources = (await readdir(`${root}src`, { recursive: true }))
  .filter((path) => minified.test(path) && !path.split(/[\\/]/).some((part) => part.startsWith(".")))
  .map((path) => `src/${path}`);
await build({
  absWorkingDir: root,
  entryPoints: sources,
  outbase: "src",
  outdir: "dist",
  minify: true,
  legalComments: "none",
  logLevel: "info",
});
