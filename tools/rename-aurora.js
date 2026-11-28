// Replaces the old product name inside the Rust sources without touching
// anything else: files are read with the encoding their byte-order mark (or
// content) says, and written back byte-for-byte apart from the ASCII
// "aurora"/"Aurora" text. A round-trip check refuses to write a file that does
// not read back identically.
//
//     node tools/rename-aurora.js

const fs = require("node:fs");
const path = require("node:path");

const dir = path.join(__dirname, "..", "src");

function decode(buffer, file) {
  if (buffer[0] === 0xff && buffer[1] === 0xfe) {
    return { text: buffer.subarray(2).toString("utf16le"), encode: (t) => Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(t, "utf16le")]) };
  }
  if (buffer[0] === 0xef && buffer[1] === 0xbb && buffer[2] === 0xbf) {
    return { text: buffer.subarray(3).toString("utf8"), encode: (t) => Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), Buffer.from(t, "utf8")]) };
  }
  const text = buffer.toString("utf8");
  // U+FFFD means this was not really UTF-8; refuse rather than corrupt it.
  if (text.includes("\uFFFD")) throw new Error(`${file}: not valid UTF-8 and no BOM`);
  return { text, encode: (t) => Buffer.from(t, "utf8") };
}

let changed = 0;
for (const name of fs.readdirSync(dir).filter((f) => f.endsWith(".rs"))) {
  const file = path.join(dir, name);
  const buffer = fs.readFileSync(file);
  const { text, encode } = decode(buffer, name);
  if (!/aurora/i.test(text)) continue;
  const next = text.replace(/aurora/g, "chrustport").replace(/Aurora/g, "ChrustPort");
  // The only difference must be the name.
  if (next.replace(/chrustport/g, "aurora").replace(/ChrustPort/g, "Aurora") !== text) {
    throw new Error(`${name}: rewrite changed more than the name`);
  }
  const out = encode(next);
  if (decode(out, name).text !== next) throw new Error(`${name}: round trip failed`);
  fs.writeFileSync(file, out);
  changed++;
  console.log(`renamed in ${name}`);
}
console.log(`${changed} file(s) changed`);
