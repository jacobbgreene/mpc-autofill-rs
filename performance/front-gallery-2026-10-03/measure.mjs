import {spawn} from 'node:child_process';
import {createServer} from 'node:http';
import {readFile,writeFile,mkdir,mkdtemp} from 'node:fs/promises';
import {dirname,resolve,extname} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
const here=dirname(fileURLToPath(import.meta.url));
const phase=process.argv[2]||'baseline';
const repeats=Number(process.argv[3]||6);
const fixtures=JSON.parse(await readFile(here+'/fixtures.json','utf8'));
const images=await Promise.all(fixtures.jpeg_files.map(f=>readFile(f)));
const journey=await readFile(here+'/journey.js','utf8');
const server=createServer(async(req,res)=>{try{const path=decodeURIComponent(new URL(req.url,'http://localhost').pathname);if(path.startsWith('/image/')){const index=Number(path.match(/(\d+)\.jpg$/)?.[1]||0)%images.length;res.writeHead(200,{'Content-Type':'image/jpeg','Cache-Control':'public, max-age=3600'});res.end(images[index]);return;}const match=path.match(/^\/(baseline|candidate)\/(.*)$/);if(!match){res.writeHead(404).end();return;}const file='/tmp/mpc-front-scroll/'+match[1]+'-ui/'+(match[2]||'index.html');const body=await readFile(file);res.writeHead(200,{'Content-Type':({'.html':'text/html','.js':'application/javascript','.css':'text/css','.map':'application/json'})[extname(file)]||'application/octet-stream'});res.end(body);}catch(e){res.writeHead(404).end(String(e));}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const origin='http://127.0.0.1:'+server.address().port;
const profile=await mkdtemp('/tmp/mpc-gallery-chrome-');
const chrome=spawn('chromium',['--headless','--disable-gpu','--no-first-run','--no-default-browser-check','--disable-background-networking','--disable-component-update','--enable-precise-memory-info','--remote-debugging-pipe','--user-data-dir='+profile],{stdio:['ignore','ignore','pipe','pipe','pipe']});
let serial=0,buffer='',session;const pending=new Map(),errors=[];
chrome.stderr.on('data',d=>{if(String(d).includes('FATAL'))process.stderr.write(d)});
chrome.stdio[4].on('data',data=>{buffer+=data;let end;while((end=buffer.indexOf('\0'))>=0){const msg=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1);if(msg.id){const p=pending.get(msg.id);pending.delete(msg.id);if(p)msg.error?p.reject(Error(JSON.stringify(msg.error))):p.resolve(msg.result);}else if(msg.method==='Runtime.exceptionThrown')errors.push(msg.params.exceptionDetails);}});
function cdp(method,params={},target=session){return new Promise((resolve,reject)=>{const id=++serial;pending.set(id,{resolve,reject});chrome.stdio[3].write(JSON.stringify({id,method,params,...(target?{sessionId:target}:{})})+'\0');});}
async function evaluate(expression){const r=await cdp('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value;}
async function waitFor(expression){for(let i=0;i<400;i++){if(await evaluate(expression))return;await new Promise(r=>setTimeout(r,10));}throw Error('Timed out: '+expression);}
async function rendererRss(){const p=await cdp('SystemInfo.getProcessInfo',{},null);const values=await Promise.all(p.processInfo.filter(p=>p.type==='renderer').map(async p=>{try{return Number((await readFile('/proc/'+p.id+'/status','utf8')).match(/^VmRSS:\s+(\d+)/m)?.[1]||0)*1024}catch{return 0}}));return values.reduce((a,b)=>a+b,0);}
const configs=[{id:'normal',kind:'card',options:72,width:1100,height:820},{id:'demanding',kind:'card',options:288,width:720,height:560},{id:'backs',kind:'cardback',options:72,width:1100,height:820}];
const rows=[];await mkdir(here+'/raw',{recursive:true});
async function run(variant,config,condition,index,profiling=false){
 const target=await cdp('Target.createTarget',{url:'about:blank'},null);session=(await cdp('Target.attachToTarget',{targetId:target.targetId,flatten:true},null)).sessionId;
 try{
  await cdp('Runtime.enable');await cdp('Page.enable');await cdp('Performance.enable');await cdp('Network.enable');await cdp('Network.clearBrowserCache');
  await cdp('Emulation.setCPUThrottlingRate',{rate:4});await cdp('Emulation.setDeviceMetricsOverride',{width:config.width,height:config.height,deviceScaleFactor:1,mobile:false});
  await cdp('Page.addScriptToEvaluateOnNewDocument',{source:'window.__perfConfig='+JSON.stringify(config)+';'+journey});
  await cdp('Page.navigate',{url:origin+'/'+variant+'/'});
  await waitFor("document.querySelector('.welcome-import-actions button')&&!document.querySelector('.welcome-import-actions button').disabled");
  if(condition==='warm')await evaluate('runGalleryJourney(__perfConfig)');
  if(profiling){await cdp('Profiler.enable');await cdp('Profiler.start');}
  const before=(await cdp('Performance.getMetrics')).metrics;
  let peakRss=await rendererRss(),polling=false;
  const sampler=setInterval(async()=>{if(polling)return;polling=true;try{peakRss=Math.max(peakRss,await rendererRss())}finally{polling=false}},100);
  let result;
  try{result=await evaluate('runGalleryJourney(__perfConfig)')}finally{clearInterval(sampler);while(polling)await new Promise(r=>setTimeout(r,10));}
  const after=(await cdp('Performance.getMetrics')).metrics;
  if(profiling){const cpu=await cdp('Profiler.stop');await writeFile(here+'/raw/'+variant+'-cpu-profile.json',JSON.stringify(cpu));}
  const metric=(items,key)=>items.find(m=>m.name===key)?.value||0;
  result={variant,workload:config.id,condition,index,profiling,...result,peakRss,taskMs:(metric(after,'TaskDuration')-metric(before,'TaskDuration'))*1000,layoutMs:(metric(after,'LayoutDuration')-metric(before,'LayoutDuration'))*1000,scriptMs:(metric(after,'ScriptDuration')-metric(before,'ScriptDuration'))*1000};
  assert.equal(result.chosen.selection.drive_id,config.kind+'-'+(config.kind==='card'?'Island':'')+'-1');assert.equal(result.chosen.selection.face,config.kind==='card'?'front':'back');assert.equal(result.peakSearch,1);assert.ok(result.peakPreview<=8);
  const filename=variant+'-'+config.id+'-'+condition+'-'+index+(profiling?'-profile':'')+'.json';await writeFile(here+'/raw/'+filename,JSON.stringify(result,null,2));
  console.log(JSON.stringify({file:filename,ms:Math.round(result.durationMs),pageWait:Math.round(result.pageWaitMs),cpu:Math.round(result.taskMs),rssMiB:Math.round(result.peakRss/1048576),heapMiB:Math.round(result.peakHeap/1048576),seen:result.seen.length,pending:result.pending}));
  return result;
 }finally{await cdp('Target.closeTarget',{targetId:target.targetId},null);}
}
try{
 if(phase==='verify'){
  const {verify}=await import('./verify.mjs');
  await verify({cdp,evaluate,waitFor,rendererRss,origin,here,journey,setSession:value=>session=value});
 }else{
 for(const config of configs)for(const condition of ['cold','warm'])for(let i=0;i<repeats;i++){
  if(phase==='followup'&&(config.id!=='demanding'||condition!=='cold'))continue;
  const paired=phase==='final'||phase==='followup';
  const variants=paired?(i%2?['candidate','baseline']:['baseline','candidate']):['baseline'];
  for(const variant of variants)rows.push(await run(variant,config,condition,phase==='followup'?i+106:paired?i+100:i));
 }
 if(phase==='baseline')await run('baseline',configs[0],'cold',99,true);
 assert.deepEqual(errors,[]);
 await writeFile(here+'/'+phase+'-results.json',JSON.stringify(rows,null,2));
 console.log('DONE '+phase+': '+rows.length+' samples');
 }
}finally{await cdp('Browser.close',{},null).catch(()=>{});chrome.kill();server.close();}
