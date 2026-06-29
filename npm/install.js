const { createWriteStream, existsSync, mkdirSync } = require("node:fs");
const { chmod, rm } = require("node:fs/promises");
const { get } = require("node:https");
const { join } = require("node:path");
const { spawnSync } = require("node:child_process");
const { createHash } = require("node:crypto");

const repo = process.env.TCUI_REPO || "petterssonjonas/TermChatUI";
const version = process.env.npm_package_version;
const vendor = join(__dirname, "vendor");
const archive = join(vendor, "tcui.tar.gz");
const binary = join(vendor, "tcui");
const url = `https://github.com/${repo}/releases/download/v${version}/tcui-x86_64-unknown-linux-gnu.tar.gz`;
const sumsUrl = `https://github.com/${repo}/releases/download/v${version}/SHA256SUMS`;

if (process.platform !== "linux" || process.arch !== "x64") {
  console.error("tcui npm package currently supports linux x64.");
  process.exit(1);
}

if (existsSync(binary)) {
  process.exit(0);
}

mkdirSync(vendor, { recursive: true });

download(sumsUrl, join(vendor, "SHA256SUMS"), () => {
  download(url, archive, async () => {
    const expected = require("node:fs")
      .readFileSync(join(vendor, "SHA256SUMS"), "utf8")
      .split("\n")
      .find((line) => line.endsWith("tcui-x86_64-unknown-linux-gnu.tar.gz"))
      ?.split(/\s+/)[0];
    if (!expected) {
      console.error("missing checksum for tcui binary");
      process.exit(1);
    }
    const data = require("node:fs").readFileSync(archive);
    const actual = createHash("sha256").update(data).digest("hex");
    if (actual !== expected) {
      console.error("tcui binary checksum verification failed");
      process.exit(1);
    }

    const tar = spawnSync("tar", ["-xzf", archive, "-C", vendor], { stdio: "inherit" });
    await rm(archive, { force: true });
    await rm(join(vendor, "SHA256SUMS"), { force: true });
    if (tar.status !== 0) {
      process.exit(tar.status ?? 1);
    }
    await chmod(binary, 0o755);
  });
});

function download(downloadUrl, destination, done) {
  get(downloadUrl, (response) => {
  if (response.statusCode !== 200) {
    console.error(`failed to download tcui binary: HTTP ${response.statusCode}`);
    process.exit(1);
  }
  response.pipe(createWriteStream(destination).on("finish", done));
  }).on("error", (error) => {
  console.error(error.message);
  process.exit(1);
  });
}
