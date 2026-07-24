// bun2nix: prepare the project for offline install.
//
// Both steps exist because bun re-resolves any dependency whose recorded
// state drifted from package.json — and re-resolving git/github/
// remote-tarball deps downloads them unconditionally, which fails in the Nix
// sandbox no matter how complete the cache is:
//
// 1. Resolve `catalog:` specifiers to exact versions (older bun re-resolves
//    them against the registry on every `bun install`). Non-registry catalog
//    values (github:/git+/tarball URLs) are left as `catalog:` — bun resolves
//    those natively from the lockfile's catalog section, and rewriting them
//    would itself register as a changed spec and force a re-resolve.
// 2. Detect `trustedDependencies` / `patchedDependencies` drift between the
//    root package.json and bun.lock, and fail with an actionable message.
//    Projects routinely commit a bun.lock whose copies of these sections lag
//    package.json; any mismatch makes bun distrust the lockfile mapping
//    wholesale, and the resulting re-resolution surfaces as a wall of
//    ConnectionRefused errors long after the actual cause.
//
// Invoked as: bun resolve-catalog.ts <bunRoot>

import { join, resolve } from "node:path";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

type Deps = Record<string, string>;
type DepHolder = Partial<
  Record<
    | "dependencies"
    | "devDependencies"
    | "peerDependencies"
    | "optionalDependencies",
    Deps
  >
>;
interface BunLock {
  workspaces?: Record<string, DepHolder>;
  catalog?: Deps;
  catalogs?: Record<string, Deps>;
  packages?: Record<string, [string, ...unknown[]]>;
  trustedDependencies?: string[];
  patchedDependencies?: Deps;
}

const depSections = [
  "dependencies",
  "devDependencies",
  "peerDependencies",
  "optionalDependencies",
] as const;

const root = process.argv[2] ?? ".";
const lockPath = join(root, "bun.lock");

if (!existsSync(lockPath)) process.exit(0);
const hasCatalogRefs = readFileSync(lockPath, "utf8").includes('"catalog:');

// bun.lock is JSON-with-trailing-commas. Bun's module loader has a built-in
// JSONC parser (used for tsconfig.json / bun.lock) that we can reach via
// `import(..., { with: { type: "jsonc" } })`. This works on every bun
// version supported by nixos-25.11+ (>= 1.3.3), unlike `Bun.JSONC` which
// only appeared in 1.3.6.
const lock = (
  (await import(pathToFileURL(resolve(lockPath)).href, {
    with: { type: "jsonc" },
  })) as { default: BunLock }
).default;

const catalog = lock.catalog ?? {};
const catalogs = lock.catalogs ?? {};
const packages = lock.packages ?? {};
const workspaces = lock.workspaces ?? {};

// Build name -> exact-version map from .packages. Only keep entries whose
// spec starts with "<name>@", i.e. the top-level resolution for that name.
const resolved: Deps = {};
for (const [name, entry] of Object.entries(packages)) {
  const spec = entry?.[0];
  if (typeof spec !== "string") continue;
  const prefix = `${name}@`;
  if (!spec.startsWith(prefix)) continue;
  resolved[name] = spec.slice(prefix.length);
}

// A spec whose resolution is not a plain registry version. Rewriting a
// `catalog:` reference to one of these would change the dependency's spec
// string and force bun to re-resolve (= re-download) it; bun resolves these
// natively from the lockfile's catalog section, so leave them alone.
function isNonRegistrySpec(spec: string): boolean {
  return (
    spec.startsWith("github:") ||
    spec.startsWith("git+") ||
    spec.startsWith("http://") ||
    spec.startsWith("https://") ||
    spec.startsWith("file:")
  );
}

function cresolve(name: string, spec: string): string {
  const cname = spec.slice("catalog:".length);
  const table = cname === "" ? catalog : (catalogs[cname] ?? {});
  const cv = table[name];
  const rv = resolved[name];
  if (typeof cv === "string" && cv.startsWith("workspace:")) return cv;
  if (typeof rv === "string" && rv.startsWith("workspace:"))
    return "workspace:*";
  if (typeof cv === "string" && isNonRegistrySpec(cv)) return spec;
  if (typeof rv === "string" && isNonRegistrySpec(rv)) return spec;
  if (typeof rv === "string") return rv;
  if (typeof cv === "string") return cv;
  return spec;
}

function rewriteDeps(holder: DepHolder): boolean {
  let changed = false;
  for (const section of depSections) {
    const deps = holder[section];
    if (!deps || typeof deps !== "object") continue;
    for (const [name, spec] of Object.entries(deps)) {
      if (typeof spec === "string" && spec.startsWith("catalog:")) {
        deps[name] = cresolve(name, spec);
        changed = true;
      }
    }
  }
  return changed;
}

let lockChanged = false;

if (hasCatalogRefs) {
  console.log("bun2nix: resolving catalog: specifiers from bun.lock");

  // Rewrite the lockfile's workspaces section.
  for (const ws of Object.values(workspaces)) {
    if (rewriteDeps(ws)) lockChanged = true;
  }

  // Rewrite every workspace package.json (root "" + each workspace dir).
  for (const wsDir of Object.keys(workspaces)) {
    const pkgJson = join(root, wsDir, "package.json");
    if (!existsSync(pkgJson)) continue;
    const text = readFileSync(pkgJson, "utf8");
    if (!text.includes('"catalog:')) continue;
    const pkg = JSON.parse(text) as DepHolder;
    if (rewriteDeps(pkg)) {
      writeFileSync(pkgJson, JSON.stringify(pkg, null, 2) + "\n");
    }
  }
}

if (lockChanged) {
  writeFileSync(lockPath, JSON.stringify(lock, null, 2) + "\n");
}

// Fail fast on trustedDependencies / patchedDependencies drift between the
// root package.json and bun.lock. Compared as a set / as key-value pairs so
// pure ordering differences don't trip the check.
const rootPkgPath = join(root, "package.json");
if (existsSync(rootPkgPath)) {
  const rootPkg = JSON.parse(readFileSync(rootPkgPath, "utf8")) as {
    trustedDependencies?: string[];
    patchedDependencies?: Deps;
  };

  const drift: string[] = [];

  const pkgTrusted = [...(rootPkg.trustedDependencies ?? [])].sort();
  const lockTrusted = [...(lock.trustedDependencies ?? [])].sort();
  if (JSON.stringify(pkgTrusted) !== JSON.stringify(lockTrusted)) {
    const missing = pkgTrusted.filter((n) => !lockTrusted.includes(n));
    const extra = lockTrusted.filter((n) => !pkgTrusted.includes(n));
    drift.push(
      `trustedDependencies differ` +
        (missing.length
          ? `; missing from bun.lock: ${missing.join(", ")}`
          : "") +
        (extra.length ? `; only in bun.lock: ${extra.join(", ")}` : ""),
    );
  }

  const pkgPatched = rootPkg.patchedDependencies ?? {};
  const lockPatched = lock.patchedDependencies ?? {};
  const patchKeys = [
    ...new Set([...Object.keys(pkgPatched), ...Object.keys(lockPatched)]),
  ].sort();
  const patchDiffs = patchKeys.filter((k) => pkgPatched[k] !== lockPatched[k]);
  if (patchDiffs.length) {
    drift.push(`patchedDependencies differ for: ${patchDiffs.join(", ")}`);
  }

  if (drift.length) {
    console.error(`
bun2nix: error: bun.lock is out of sync with package.json:
${drift.map((d) => `  - ${d}`).join("\n")}

bun re-resolves dependencies when these sections drift, and re-resolving
git/github/tarball dependencies requires network access, which is not
available in the Nix sandbox. Run \`bun install\` to refresh bun.lock,
commit the result, and regenerate bun.nix.
`);
    process.exit(1);
  }
}
