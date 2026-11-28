// Packages the ChrustPort mark as a multi-resolution Windows .ico, so the
// taskbar, Alt+Tab, Explorer and the exe all get a frame drawn at their own
// size instead of a downscaled 256.
//
//     node assets/chrust-port/make-ico.js

const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const here = __dirname;
const sizes = [16, 24, 32, 48, 64, 128, 256];
const frames = [];

for (const size of sizes) {
  // Render each size straight from the generator, then read it back.
  const tmp = path.join(os.tmpdir(), `chrust-port-${size}.png`);
  execFileSync(process.execPath, [path.join(here, "make-icon.js"), String(size), tmp], { stdio: "ignore" });
  frames.push({ size, png: fs.readFileSync(tmp) });
  fs.unlinkSync(tmp);
}

// ICO header: reserved(0), type(1 = icon), image count.
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(frames.length, 4);

// One directory entry per frame, then the PNG payloads (PNG-compressed ICO
// entries are understood by Windows Vista and later).
const entries = [];
let offset = 6 + frames.length * 16;
for (const { size, png } of frames) {
  const entry = Buffer.alloc(16);
  entry[0] = size >= 256 ? 0 : size; // 0 means 256
  entry[1] = size >= 256 ? 0 : size;
  entry[2] = 0; // palette
  entry[3] = 0; // reserved
  entry.writeUInt16LE(1, 4); // colour planes
  entry.writeUInt16LE(32, 6); // bits per pixel
  entry.writeUInt32LE(png.length, 8);
  entry.writeUInt32LE(offset, 12);
  entries.push(entry);
  offset += png.length;
}

const out = path.join(here, "chrust-port.ico");
fs.writeFileSync(out, Buffer.concat([header, ...entries, ...frames.map((f) => f.png)]));
console.log(`${out}: ${frames.length} sizes (${sizes.join(", ")})`);
