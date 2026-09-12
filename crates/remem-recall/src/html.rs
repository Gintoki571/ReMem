//! Self-contained HTML knowledge-graph export: one file, no CDN, no server.
//! Canvas rendering with a spring-embedder layout, click-for-content, kind filter.

use anyhow::Result;
use serde_json::Value as JsonValue;

/// Build the single-file HTML document with node/edge JSON embedded.
/// `nodes`: [{id, kind, snippet}], `edges`: [{from, rel, to, prov}].
pub fn graph_html(nodes: &JsonValue, edges: &JsonValue) -> Result<String> {
    let nodes = serde_json::to_string(nodes)?;
    let edges = serde_json::to_string(edges)?;
    // Note: </script> inside the JSON would break the document; memories are
    // plain text, and serde_json does not escape "/" — replace defensively.
    let nodes = nodes.replace("</", "<\\/");
    let edges = edges.replace("</", "<\\/");
    Ok(format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>remem graph</title>
<style>
body{{font:13px system-ui,sans-serif;margin:0;display:flex;height:100vh}}
#side{{width:280px;padding:10px;border-right:1px solid #ccc;overflow:auto}}
#info{{white-space:pre-wrap;color:#333;margin-top:8px}}
.kf{{margin:2px;padding:2px 6px;border:1px solid #888;border-radius:4px;cursor:pointer;display:inline-block}}
.kf.on{{background:#2a6;color:#fff}}
canvas{{flex:1}}
</style></head><body>
<div id="side"><b>filter by kind</b><div id="filters"></div><div id="info">click a node</div></div>
<canvas id="c"></canvas>
<script>
const NODES={nodes};
const EDGES={edges};
const c=document.getElementById('c'),ctx=c.getContext('2d');
let show=new Set(NODES.map(n=>n.kind));
const colors={{Agent:'#c60',Session:'#06c'}};
function color(k){{return colors[k]||'#333'}}
// filters
const kinds=[...new Set(NODES.map(n=>n.kind))];
const fdiv=document.getElementById('filters');
for(const k of kinds){{
  const b=document.createElement('span');b.className='kf on';b.textContent=k;
  b.onclick=()=>{{show.has(k)?show.delete(k):show.add(k);b.classList.toggle('on');}};
  fdiv.appendChild(b);
}}
// layout: radial start + spring/repulsion loop
const pos=NODES.map((n,i)=>{{const a=2*Math.PI*i/NODES.length,r=200;
  return {{x:c.width/2+r*Math.cos(a),y:c.height/2+r*Math.sin(a),vx:0,vy:0}};}});
function resize(){{c.width=innerWidth-300;c.height=innerHeight;}}
resize();addEventListener('resize',resize);
function tick(){{
  for(let i=0;i<NODES.length;i++)for(let j=i+1;j<NODES.length;j++){{
    const a=pos[i],b=pos[j];let dx=a.x-b.x,dy=a.y-b.y,d2=dx*dx+dy*dy+1;
    if(d2<40000){{const f=2000/d2;a.vx+=dx*f;a.vy+=dy*f;b.vx-=dx*f;b.vy-=dy*f;}}
  }}
  for(const e of EDGES){{
    const a=pos[NODES.findIndex(n=>n.id===e.from)],b=pos[NODES.findIndex(n=>n.id===e.to)];
    if(!a||!b)continue;let dx=b.x-a.x,dy=b.y-a.y,d=Math.hypot(dx,dy)||1;
    const f=(d-120)*0.02;a.vx+=dx/d*f*d;a.vy+=dy/d*f*d;b.vx-=dx/d*f*d;b.vy-=dy/d*f*d;
  }}
  for(const p of pos){{
    p.vx*=.85;p.vy*=.85;p.x+=Math.max(-8,Math.min(8,p.vx));p.y+=Math.max(-8,Math.min(8,p.vy));
    p.x=Math.max(20,Math.min(c.width-20,p.x));p.y=Math.max(20,Math.min(c.height-20,p.y));
  }}
}}
const idx=Object.fromEntries(NODES.map((n,i)=>[n.id,i]));
function visible(i){{return show.has(NODES[i].kind)}}
function draw(){{
  tick();
  ctx.clearRect(0,0,c.width,c.height);
  ctx.strokeStyle='#aaa';
  for(const e of EDGES){{
    const a=pos[idx[e.from]],b=pos[idx[e.to]];
    if(!a||!b||!visible(idx[e.from])||!visible(idx[e.to]))continue;
    ctx.beginPath();ctx.moveTo(a.x,a.y);ctx.lineTo(b.x,b.y);ctx.stroke();
    ctx.fillStyle='#888';ctx.font='9px sans-serif';
    ctx.fillText(e.rel+(e.prov&&e.prov!=='manual'?' ('+e.prov+')':''),(a.x+b.x)/2,(a.y+b.y)/2);
  }}
  for(let i=0;i<NODES.length;i++){{
    if(!visible(i))continue;const p=pos[i];
    ctx.fillStyle=color(NODES[i].kind);
    ctx.beginPath();ctx.arc(p.x,p.y,6,0,2*Math.PI);ctx.fill();
    ctx.fillStyle='#000';ctx.font='10px sans-serif';
    ctx.fillText(NODES[i].id.slice(0,8),p.x+8,p.y+3);
  }}
  requestAnimationFrame(draw);
}}
c.onclick=ev=>{{
  const {{x,y}}=ev;
  for(let i=0;i<NODES.length;i++){{
    if(!visible(i))continue;const p=pos[i];
    if(Math.hypot(p.x-x,p.y-y)<8){{
      document.getElementById('info').textContent=
        NODES[i].id+'\n['+NODES[i].kind+']\n'+NODES[i].snippet;
      return;
    }}
  }}
}};
draw();
</script></body></html>"#,
        nodes = nodes,
        edges = edges
    ))
}
