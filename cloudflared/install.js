import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { Readable } from "node:stream";
import { t } from "tar";

const binaries = {
  "darwin-arm64.tgz": {
    targets: ["aarch64-apple-darwin"],
    decompress: true,
  },
  "darwin-amd64.tgz": {
    targets: [],
    decompress: true,
  },
  "windows-386.exe": {
    targets: [],
    decompress: false,
  },
  "windows-amd64.exe": {
    targets: [],
    decompress: false,
  },
};

for (const [filename, { targets, decompress }] of Object.entries(binaries)) {
  if (!targets.length) continue;
  console.log(`downloading binary ${filename}`);

  const res = await fetch(
    `https://github.com/cloudflare/cloudflared/releases/download/2025.7.0/cloudflared-${filename}`,
  );
  if (!res.ok) throw new Error(`unable to fetch binary: ${await res.text()}`);

  let body = await res.bytes();
  if (decompress) {
    console.log("decompressing");

    await new Promise((res, rej) => {
      Readable.from(body)
        .pipe(
          t({
            gzip: true,
            onentry(entry) {
              const chunks = [];

              entry.on("data", (chunk) => {
                chunks.push(chunk);
              });

              entry.on("end", () => {
                body = Buffer.concat(chunks);
              });
            },
          }),
        )
        .on("end", () => res())
        .on("error", rej);
    });
  }

  for (const target of targets) {
    const dest = resolve(import.meta.dirname, `cloudflared-${target}`);
    await writeFile(dest, body);
  }
}
