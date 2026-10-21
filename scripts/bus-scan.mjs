
import fs from "node:fs";
const fd = fs.openSync("/dev/ttyACM0", "r");
const chunks = [];
const st = fs.createReadStream(null, { fd, autoClose: false });
st.on("data", c => chunks.push(c));
await new Promise(r => setTimeout(r, 3000));
st.destroy();

const all = Buffer.concat(chunks).toString("latin1");
const seen = new Map();
let start = 0, packets = 0;
for (let i = 0; i < all.length; i++) {
  if (all[i] !== "\n") continue;
  const p = all.slice(start, i + 1); start = i + 1;
  packets++;
  // header: SOH BRC LEN(4) ID(2) SESSION(2) SX(2) SY(2) DX(2) DY(2) ROT PORTROT MSGAGE(2) EOB
  const sx = parseInt(p.slice(10, 12), 16);
  const sy = parseInt(p.slice(12, 14), 16);
  const hb = p.indexOf("\u0002010");
  if (hb < 0) continue;
  const body = p.slice(hb + 1, p.indexOf("\u0003", hb));
  const pay = body.slice(4);
  const type  = parseInt(pay.slice(0, 2), 16);
  const hwcfg = parseInt(pay.slice(2, 4), 16);
  const ver   = [2,4,6].map(o => parseInt(pay.slice(o*1+2, o*1+4), 16));
  const vmaj = parseInt(pay.slice(4,6),16), vmin = parseInt(pay.slice(6,8),16), vpat = parseInt(pay.slice(8,10),16);
  const key = `${sx},${sy}`;
  if (!seen.has(key)) seen.set(key, { sx, sy, hwcfg, hbtype: type, fw: `${vmaj}.${vmin}.${vpat}`, count: 0 });
  seen.get(key).count++;
}
console.log("packets:", packets, "distinct module positions:", seen.size);
for (const [k, v] of seen) {
  console.log(`  position ${k.padEnd(8)} hwcfg=0x${v.hwcfg.toString(16).padStart(2,"0")} fw=${v.fw} hb_type=${v.hbtype} heartbeats=${v.count}`);
}
