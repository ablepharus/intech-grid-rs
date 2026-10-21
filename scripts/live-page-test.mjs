
import fs from "node:fs";
const SOH=1,BRC=0x0f,EOB=0x17,STX=2,ETX=3,EOT=4,LF=0x0a;
let idc=0x80;
function enc(lua, dx=127, dy=127){
  const payload=lua.length.toString(16).padStart(4,"0")+lua;
  const id=(idc=(idc+1)&0xff)||1;
  let s=String.fromCharCode(SOH)+String.fromCharCode(BRC)+"0000"
   +id.toString(16).padStart(2,"0")+"77"+"0000"
   +dx.toString(16).padStart(2,"0")+dy.toString(16).padStart(2,"0")+"00"+"00"
   +String.fromCharCode(EOB)
   +String.fromCharCode(STX)+"085"+"e"+payload+String.fromCharCode(ETX)
   +String.fromCharCode(EOT)+"00"+String.fromCharCode(LF);
  const b=Buffer.from(s,"latin1");
  b.write((b.length-3).toString(16).padStart(4,"0"),2,"latin1");
  let x=0;for(let i=0;i<b.length-3;i++)x^=b[i];
  b.write(x.toString(16).padStart(2,"0"),b.length-3,"latin1");
  return b;
}
const fd=fs.openSync("/dev/ttyACM0","r+");
let chunks=[];
const st=fs.createReadStream(null,{fd,autoClose:false});
st.on("data",c=>chunks.push(c));
const sleep=ms=>new Promise(r=>setTimeout(r,ms));

function pages(){
  const all=Buffer.concat(chunks).toString("latin1");
  const m=new Map();
  let start=0;
  for(let i=0;i<all.length;i++){
    if(all[i]!=="\n")continue;
    const p=all.slice(start,i+1); start=i+1;
    const sx=parseInt(p.slice(10,12),16), sy=parseInt(p.slice(12,14),16);
    let idx=-1;
    while((idx=p.indexOf("\u0002030",idx+1))>=0){
      const pay=p.slice(idx+1,p.indexOf("\u0003",idx)).slice(4);
      m.set(`${sx},${sy}`, parseInt(pay.slice(0,2),16));
    }
  }
  return [...m.entries()].map(([k,v])=>`${k}=page${v}`).join("  ");
}

async function obs(label){ chunks=[]; await sleep(1500); console.log(`[${label}] ${pages()}`); }

await sleep(800);
await obs("before");

console.log("-> sending WRAPPED page_next to master (dx=dy=127)");
fs.writeSync(fd, enc("<?lua gpn() ?>"));
await obs("after wrapped gpn()");

console.log("-> sending UNWRAPPED page_next (the old broken form)");
fs.writeSync(fd, enc("gpn()"));
await obs("after unwrapped gpn()");

console.log("-> restoring page 0 (wrapped)");
fs.writeSync(fd, enc("<?lua gpl(0) ?>"));
await obs("after restore");
st.destroy();
