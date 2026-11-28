// Draws the ChrustPort mark: the Chrust palette (red/orange/amber) as a
// rounded badge with a white "port" ring that has an opening on the right,
// echoing the C in the Chrust logo while reading as a new identity.
//
// Writes a PNG of the requested size. Run from anywhere:
//     node assets/chrust-port/make-icon.js 256 assets/chrust-port/chrust-port-256.png

const fs = require("node:fs");

function crc32(buf) {
  let crc = 0xffffffff;
  for (const byte of buf) {
    crc ^= byte;
    for (let k = 0; k < 8; k++) crc = crc & 1 ? 0xedb88320 ^ (crc >>> 1) : crc >>> 1;
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

// Valid zlib data made of stored (uncompressed) deflate blocks: no compressor
// needed, and the output is identical on every machine.
function zlibStored(raw) {
  const parts = [Buffer.from([0x78, 0x01])];
  for (let offset = 0; offset < raw.length; ) {
    const length = Math.min(60000, raw.length - offset);
    const header = Buffer.alloc(5);
    header[0] = offset + length >= raw.length ? 1 : 0;
    header.writeUInt16LE(length, 1);
    header.writeUInt16LE(~length & 0xffff, 3);
    parts.push(header, raw.subarray(offset, offset + length));
    offset += length;
  }
  let a = 1;
  let b = 0;
  for (const byte of raw) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  const adler = Buffer.alloc(4);
  adler.writeUInt32BE(((b << 16) | a) >>> 0);
  parts.push(adler);
  return Buffer.concat(parts);
}

function draw(size) {
  const pixels = Buffer.alloc(size * size * 4);
  const centre = size / 2;
  const half = size * 0.453;
  const corner = size * 0.1875;
  const ringInner = size * 0.293;
  const ringOuter = size * 0.383;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const dx = x - centre;
      const dy = y - centre;
      const radius = Math.hypot(dx, dy);
      let r = 0, g = 0, b = 0, a = 0;
      // Rounded-square badge.
      const qx = Math.max(Math.abs(dx) - (half - corner), 0);
      const qy = Math.max(Math.abs(dy) - (half - corner), 0);
      if (Math.abs(dx) <= half && Math.abs(dy) <= half && Math.hypot(qx, qy) <= corner) {
        // Diagonal gradient, the top-left red of the Chrust mark to amber.
        const t = Math.min(1, Math.max(0, (x + y) / (2 * size)));
        r = 255;
        g = Math.round(0x3d + (0xb0 - 0x3d) * t);
        b = Math.round(0x0c + (0x1f - 0x0c) * t);
        a = 255;
      }
      // White ring, open on the right.
      const angle = Math.atan2(dy, dx);
      const gap = angle > -0.55 && angle < 0.55;
      if (!gap && radius >= ringInner && radius <= ringOuter) {
        r = g = b = 255;
        a = 255;
      }
      const o = (y * size + x) * 4;
      pixels[o] = r; pixels[o + 1] = g; pixels[o + 2] = b; pixels[o + 3] = a;
    }
  }
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    pixels.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;  // bit depth
  ihdr[9] = 6;  // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlibStored(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const size = Number(process.argv[2] || 256);
const out = process.argv[3] || `chrust-port-${size}.png`;
fs.writeFileSync(out, draw(size));
console.log(`${out}: ${size}x${size}`);
