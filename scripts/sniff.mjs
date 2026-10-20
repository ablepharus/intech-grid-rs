
import fs from "node:fs";
const path = process.argv[2] ?? "/dev/ttyACM0";
const ms = Number(process.argv[3] ?? 3000);
const fd = fs.openSync(path, "r");
const buf = Buffer.alloc(65536);
let acc = Buffer.alloc(0);
const stream = fs.createReadStream(null, { fd, highWaterMark: 4096 });
const chunks = [];
stream.on("data", (c) => chunks.push(c));
setTimeout(() => {
  stream.destroy();
  const all = Buffer.concat(chunks);
  console.log("bytes:", all.length);
  // split on LF (0x0a) = end of packet
  const parts = [];
  let start = 0;
  for (let i = 0; i < all.length; i++) if (all[i] === 0x0a) { parts.push(all.subarray(start, i + 1)); start = i + 1; }
  console.log("packets:", parts.length);
  const seen = new Map();
  for (const p of parts.slice(0, 400)) {
    const s = p.toString("latin1");
    // class code = 3 hex after STX
    const stx = s.indexOf("\x02");
    const cls = stx >= 0 ? s.slice(stx + 1, stx + 4) : "???";
    if (!seen.has(cls)) seen.set(cls, []);
    if (seen.get(cls).length < 2) seen.get(cls).push(s);
  }
  for (const [cls, ex] of seen) {
    console.log("=== class", cls, "count-sample");
    for (const e of ex) console.log("   ", JSON.stringify(e));
  }
}, ms);
