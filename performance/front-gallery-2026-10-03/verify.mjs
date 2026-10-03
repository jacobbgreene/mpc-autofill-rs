import { readFile, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';

export async function verify({cdp,evaluate,waitFor,rendererRss,origin,here,journey,setSession}) {
 const config={id:'normal',kind:'card',options:72,width:1100,height:820};
 const target=await cdp('Target.createTarget',{url:'about:blank'},null);
 setSession((await cdp('Target.attachToTarget',{targetId:target.targetId,flatten:true},null)).sessionId);
 try {
  await cdp('Runtime.enable');await cdp('Page.enable');
  await cdp('Emulation.setCPUThrottlingRate',{rate:4});
  await cdp('Emulation.setDeviceMetricsOverride',{width:1100,height:820,deviceScaleFactor:1,mobile:false});
  const edgeScript=await readFile(here+'/edge-tests.js','utf8');
  // Add query changes only to this correctness/lifetime scenario. The timed
  // baseline/candidate journey remains identical to the recorded experiment.
  const alternating=journey.replace('window.runGalleryJourney', 'window.runGalleryAlternatingJourney').replace(
    'const columns=getComputedStyle',
    `if(config.query){const input=document.querySelector('[aria-label="Artwork search name"]');input.value=config.query;input.dispatchEvent(new Event('input',{bubbles:true}));document.querySelector('.gallery-toolbar').requestSubmit();await wait(()=>window.__perf.calls.some(c=>c.cmd==='search_artwork'&&c.args.query===config.query&&c.end)&&document.querySelector('.artwork-gallery .editor-body').getAttribute('aria-busy')==='false','alternating query');await frame();}const columns=getComputedStyle`
  );
  await cdp('Page.addScriptToEvaluateOnNewDocument',{source:'window.__perfConfig='+JSON.stringify(config)+';'+journey+';'+alternating+';'+edgeScript});
  await cdp('Page.navigate',{url:origin+'/candidate/'});
  await waitFor("document.querySelector('.welcome-import-actions button')&&!document.querySelector('.welcome-import-actions button').disabled");
  const edges=await evaluate('runGalleryEdges()');
  await writeFile(here+'/edges-results.json',JSON.stringify(edges,null,2));
  console.log('PASS edges: '+edges.cases.length+' cases');
  await cdp('Page.navigate',{url:origin+'/candidate/'});
  await waitFor("document.querySelector('.welcome-import-actions button')&&!document.querySelector('.welcome-import-actions button').disabled");
  const samples=[];
  for(let cycle=1;cycle<=20;cycle++) {
   const query=cycle%2?'Island':'Forest';
   const result=await evaluate('runGalleryAlternatingJourney({...__perfConfig,query:'+JSON.stringify(query)+'})');
   assert.equal(result.chosen.selection.query,query);
   assert.equal(result.pending,0);assert.equal(result.previewWaiting,0);
   const nodes=await evaluate("document.querySelectorAll('*').length");
   const galleries=await evaluate("document.querySelectorAll('.artwork-gallery').length");
   assert.equal(galleries,0);
   if(cycle===5||cycle===20) {
    await cdp('HeapProfiler.collectGarbage');
    const heap=(await cdp('Runtime.getHeapUsage')).usedSize;
    samples.push({cycle,heap,rss:await rendererRss(),nodes,pending:result.pending});
   }
  }
  const [early,late]=samples;
  const result={scenario:'20 open/browse/choose/close cycles alternating Island and Forest',samples,heapGrowth:late.heap-early.heap,rssGrowth:late.rss-early.rss};
  result.pass=result.heapGrowth<=8*1048576&&result.rssGrowth<=64*1048576&&late.nodes===early.nodes;
  await writeFile(here+'/sustained-results.json',JSON.stringify(result,null,2));
  assert.ok(result.pass,JSON.stringify(result));
  console.log('PASS sustained: '+JSON.stringify(result));
 } finally {await cdp('Target.closeTarget',{targetId:target.targetId},null);}
}
