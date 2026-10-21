
import fs from "node:fs";
const SOH=1,BRC=0x0f,EOB=0x17,STX=2,ETX=3,EOT=4,LF=0x0a;
let idc=0x90;
function enc(lua){
  const payload=lua.length.toString(16).padStart(4,"0")+lua;
  const id=(idc=(idc+1)&0xff)||1;
  let s=String.fromCharCode(SOH)+String.fromCharCode(BRC)+"0000"
   +id.toString(16).padStart(2,"0")+"77"+"0000"+"7f7f"+"00"+"00"
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

async function trial(label, lua){
  chunks=[]; 
  fs.writeSync(fd, enc(lua));
  await sleep(1500);
  const all=Buffer.concat(chunks).toString("latin1");
  const hit = all.includes("ULWMARK");
  const classes=new Set();
  let start=0;
  for(let i=0;i<all.length;i++){
    if(all[i]!=="\n")continue;
    const p=all.slice(start,i+1); start=i+1;
    let idx=-1;
    while((idx=p.indexOf("\u0002",idx+1))>=0) classes.add(p.slice(idx+1,idx+4));
  }
  console.log(`[${label}] marker=${hit}  classes=${[...classes].sort().join(",")}`);
  if(hit){
    const i=all.indexOf("ULWMARK");
    console.log("   >>>", JSON.stringify(all.slice(Math.max(0,i-60), i+25)));
  }
}

await sleep(900);
await trial("WRAPPED print", "<?lua print('ULWMARK') ?>");
await trial("UNWRAPPED print (old broken form)", "print('ULWMARK')");
await trial("WRAPPED pagecount query", "<?lua print('ULWMARK'..gpc()) ?>");
st.destroy();
