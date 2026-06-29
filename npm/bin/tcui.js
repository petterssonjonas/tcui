#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const { join } = require("node:path");

const bin = join(__dirname, "..", "vendor", "tcui");
if (!existsSync(bin)) {
  console.error("tcui binary is missing. Reinstall the package.");
  process.exit(1);
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
process.exit(result.status ?? 1);
