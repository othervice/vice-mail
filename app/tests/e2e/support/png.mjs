// Makes the README screenshots small enough (docs/images/README.md: about 500 KB each) without a
// tool or package the project does not have: the PNG of the web view is re-encoded losslessly
// (RGB instead of RGBA, a filter per row, zlib level 9), and only if that is still too big it gets
// a palette of 256 colours. The most frequent colours (backgrounds, text) stay exact; the rest,
// mostly the edges of anti-aliased text, go to their nearest colour of a palette from median cut
// and a few rounds of k-means. Node only, no dependencies.
import fs from "node:fs";
import zlib from "node:zlib";

const SIGNATURE = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
/** Palette entries kept for the most frequent colours exactly as they are. */
const EXACT = 96;

/**
 * 8-bit RGB or RGBA, not interlaced (what WebKit's snapshot writes), as RGB rows, with the lowest
 * alpha value (WebKit leaves a few pixels at 253 or 254 where layers meet).
 */
export function decodePng(buffer) {
  if (!buffer.subarray(0, 8).equals(SIGNATURE)) throw new Error("not a PNG");
  let pos = 8;
  let header = null;
  const data = [];
  while (pos < buffer.length) {
    const length = buffer.readUInt32BE(pos);
    const type = buffer.toString("latin1", pos + 4, pos + 8);
    const body = buffer.subarray(pos + 8, pos + 8 + length);
    if (type === "IHDR") {
      header = { width: body.readUInt32BE(0), height: body.readUInt32BE(4), depth: body[8], colorType: body[9], interlace: body[12] };
    } else if (type === "IDAT") {
      data.push(body);
    }
    pos += 12 + length;
  }
  const channels = { 2: 3, 6: 4 }[header?.colorType];
  if (!header || header.depth !== 8 || !channels || header.interlace !== 0) throw new Error(`unsupported PNG: ${JSON.stringify(header)}`);
  const { width, height } = header;
  const raw = zlib.inflateSync(Buffer.concat(data));
  const stride = width * channels;
  const line = Buffer.alloc(stride);
  const previous = Buffer.alloc(stride);
  const rgb = Buffer.alloc(width * height * 3);
  let minAlpha = 255;
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const source = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x += 1) {
      const a = x >= channels ? line[x - channels] : 0;
      const b = previous[x];
      const c = x >= channels ? previous[x - channels] : 0;
      let predicted = 0;
      if (filter === 1) predicted = a;
      else if (filter === 2) predicted = b;
      else if (filter === 3) predicted = (a + b) >> 1;
      else if (filter === 4) predicted = paeth(a, b, c);
      line[x] = (source[x] + predicted) & 255;
    }
    for (let x = 0; x < width; x += 1) {
      rgb[(y * width + x) * 3] = line[x * channels];
      rgb[(y * width + x) * 3 + 1] = line[x * channels + 1];
      rgb[(y * width + x) * 3 + 2] = line[x * channels + 2];
      if (channels === 4 && line[x * 4 + 3] < minAlpha) minAlpha = line[x * 4 + 3];
    }
    line.copy(previous);
  }
  return { width, height, rgb, minAlpha };
}

function paeth(a, b, c) {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  return pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function chunk(type, body) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(body.length, 0);
  head.write(type, 4, "latin1");
  let crc = 0xffffffff;
  for (const byte of head.subarray(4)) crc = CRC_TABLE[(crc ^ byte) & 255] ^ (crc >>> 8);
  for (const byte of body) crc = CRC_TABLE[(crc ^ byte) & 255] ^ (crc >>> 8);
  const tail = Buffer.alloc(4);
  tail.writeUInt32BE((crc ^ 0xffffffff) >>> 0, 0);
  return Buffer.concat([head, body, tail]);
}

/**
 * Encodes rows of `bytesPerPixel` bytes; `palette` (RGB triples) makes it an indexed PNG. RGB rows
 * get the filter with the smallest sum of absolute differences per row; palette rows none.
 */
function encodePng(width, height, pixels, bytesPerPixel, palette = null) {
  const stride = width * bytesPerPixel;
  const out = Buffer.alloc((stride + 1) * height);
  const candidates = Array.from({ length: 5 }, () => Buffer.alloc(stride));
  for (let y = 0; y < height; y += 1) {
    const row = pixels.subarray(y * stride, (y + 1) * stride);
    const above = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : null;
    let best = 0;
    let bestSum = Infinity;
    for (let filter = 0; filter < (palette ? 1 : 5); filter += 1) {
      const target = candidates[filter];
      let sum = 0;
      for (let x = 0; x < stride; x += 1) {
        const a = x >= bytesPerPixel ? row[x - bytesPerPixel] : 0;
        const b = above ? above[x] : 0;
        const c = above && x >= bytesPerPixel ? above[x - bytesPerPixel] : 0;
        const predicted = [0, a, b, (a + b) >> 1, paeth(a, b, c)][filter];
        const value = (row[x] - predicted) & 255;
        target[x] = value;
        sum += value < 128 ? value : 256 - value;
      }
      if (sum < bestSum) {
        bestSum = sum;
        best = filter;
      }
    }
    out[y * (stride + 1)] = best;
    candidates[best].copy(out, y * (stride + 1) + 1);
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = palette ? 3 : 2;
  return Buffer.concat([
    SIGNATURE,
    chunk("IHDR", header),
    ...(palette ? [chunk("PLTE", Buffer.from(palette))] : []),
    chunk("IDAT", zlib.deflateSync(out, { level: 9, memLevel: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const distance = (a, b) => (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2;

/** 256 colours for `colors` (`[r, g, b, count]`): the most frequent exactly, the rest by median cut and k-means. */
function buildPalette(colors) {
  const sorted = [...colors].sort((a, b) => b[3] - a[3]);
  const exact = sorted.slice(0, EXACT).map((c) => [c[0], c[1], c[2]]);
  const rest = sorted.slice(EXACT);
  const boxes = [rest];
  while (boxes.length < 256 - EXACT) {
    // Split the box with the largest spread times weight along its widest channel.
    let pick = -1;
    let score = 0;
    let axis = 0;
    boxes.forEach((box, index) => {
      if (box.length < 2) return;
      const ranges = [0, 1, 2].map((ch) => Math.max(...box.map((c) => c[ch])) - Math.min(...box.map((c) => c[ch])));
      const widest = ranges.indexOf(Math.max(...ranges));
      const weight = box.reduce((sum, c) => sum + c[3], 0);
      if (ranges[widest] * Math.sqrt(weight) > score) {
        score = ranges[widest] * Math.sqrt(weight);
        pick = index;
        axis = widest;
      }
    });
    if (pick < 0) break;
    const box = boxes[pick].sort((a, b) => a[axis] - b[axis]);
    const total = box.reduce((sum, c) => sum + c[3], 0);
    let acc = 0;
    let cut = 1;
    for (; cut < box.length - 1; cut += 1) {
      acc += box[cut - 1][3];
      if (acc >= total / 2) break;
    }
    boxes.splice(pick, 1, box.slice(0, cut), box.slice(cut));
  }
  const mean = (box) => {
    const weight = box.reduce((sum, c) => sum + c[3], 0);
    return [0, 1, 2].map((ch) => Math.round(box.reduce((sum, c) => sum + c[ch] * c[3], 0) / weight));
  };
  let free = boxes.filter((box) => box.length > 0).map(mean);
  // k-means on the remaining colours; the exact entries take part in the assignment only.
  for (let round = 0; round < 4 && free.length > 0; round += 1) {
    const sums = free.map(() => [0, 0, 0, 0]);
    for (const c of rest) {
      let best = 0;
      let bestDistance = Infinity;
      free.forEach((p, i) => {
        const d = distance(c, p);
        if (d < bestDistance) {
          bestDistance = d;
          best = i;
        }
      });
      const s = sums[best];
      s[0] += c[0] * c[3];
      s[1] += c[1] * c[3];
      s[2] += c[2] * c[3];
      s[3] += c[3];
    }
    free = free.map((p, i) => (sums[i][3] > 0 ? [0, 1, 2].map((ch) => Math.round(sums[i][ch] / sums[i][3])) : p));
  }
  return [...exact, ...free];
}

/** Quantises the RGB pixels to a palette; returns the indexed PNG and the mean error per channel. */
function toPalette(image) {
  const { width, height, rgb } = image;
  const counts = new Map();
  for (let i = 0; i < rgb.length; i += 3) {
    const key = (rgb[i] << 16) | (rgb[i + 1] << 8) | rgb[i + 2];
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const colors = [...counts].map(([key, count]) => [key >> 16, (key >> 8) & 255, key & 255, count]);
  const palette = colors.length <= 256 ? colors.map((c) => [c[0], c[1], c[2]]) : buildPalette(colors);
  const index = new Map();
  for (const c of colors) {
    let best = 0;
    let bestDistance = Infinity;
    palette.forEach((p, i) => {
      const d = distance(c, p);
      if (d < bestDistance) {
        bestDistance = d;
        best = i;
      }
    });
    index.set((c[0] << 16) | (c[1] << 8) | c[2], best);
  }
  const pixels = Buffer.alloc(width * height);
  let error = 0;
  for (let i = 0, p = 0; i < rgb.length; i += 3, p += 1) {
    const entry = index.get((rgb[i] << 16) | (rgb[i + 1] << 8) | rgb[i + 2]);
    pixels[p] = entry;
    const q = palette[entry];
    error += Math.abs(q[0] - rgb[i]) + Math.abs(q[1] - rgb[i + 1]) + Math.abs(q[2] - rgb[i + 2]);
  }
  return { png: encodePng(width, height, pixels, 1, palette.flat()), colors: colors.length, meanError: error / (width * height * 3) };
}

/**
 * Rewrites `file` smaller: lossless if that reaches `maxBytes`, else with a palette. Returns what
 * was done. `height` keeps only that many rows from the top (a window with a title bar of its own
 * snapshots a strip below the page). The alpha channel is dropped; a picture with real
 * transparency is refused (the web view's are opaque but for a few pixels at 253 or 254).
 */
export function optimizePng(file, maxBytes, { height = null } = {}) {
  const image = decodePng(fs.readFileSync(file));
  if (height !== null && height < image.height) {
    image.rgb = image.rgb.subarray(0, image.width * height * 3);
    image.height = height;
  }
  if (image.minAlpha < 250) throw new Error(`${file} has transparent pixels (alpha ${String(image.minAlpha)})`);
  const lossless = encodePng(image.width, image.height, image.rgb, 3);
  if (lossless.length <= maxBytes) {
    fs.writeFileSync(file, lossless);
    return { method: "lossless", bytes: lossless.length };
  }
  const indexed = toPalette(image);
  fs.writeFileSync(file, indexed.png);
  return { method: "palette", bytes: indexed.png.length, colors: indexed.colors, meanError: Math.round(indexed.meanError * 1000) / 1000 };
}
