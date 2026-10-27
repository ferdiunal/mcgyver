// The site as served: src/ copied to dist/, its scripts and stylesheets minified on the way, and
// the stylesheet written into each page. Wrangler runs this before `dev` and `deploy`
// (wrangler.jsonc "build").
import { cp, readdir, readFile, rm, writeFile } from "node:fs/promises";
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

// The first paint waits for the page alone, not for a second request: on a phone's slow line
// (PageSpeed's mobile run) styles.css shared it with the photo, the scripts and the fonts.
const css = (await readFile(`${root}dist/styles.css`, "utf8")).trim();
const link = '<link rel="stylesheet" href="/styles.css">';
for (const page of (await readdir(`${root}dist`, { recursive: true })).filter((path) => path.endsWith(".html"))) {
  const html = await readFile(`${root}dist/${page}`, "utf8");
  if (!html.includes(link)) throw new Error(`${page} doesn't link ${link}`);
  await writeFile(`${root}dist/${page}`, html.replace(link, () => `<style>${css}</style>`));
}
