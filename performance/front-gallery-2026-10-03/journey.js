window.runGalleryJourney = async function(config) {
 const wait=async(test,label)=>{const start=performance.now();while(!test()){if(performance.now()-start>15000)throw Error('Timeout: '+label);await new Promise(r=>setTimeout(r,8));}};
 const frame=()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
 const buttons=()=>[...document.querySelectorAll('button')];
 const clickText=text=>{const b=buttons().find(b=>b.textContent.trim()===text);if(!b||b.disabled)throw Error('Missing/disabled '+text);b.click();};
 clickText('Open order');await wait(()=>document.querySelector('.card-tile')&&window.__perf.pending===0,'deck');await frame();
 const metrics={frames:[],longTasks:[],seen:[],steps:[],pageWaitMs:0,previewWaitMs:0,pageActivations:0,addedChoices:0,removedChoices:0,peakHeap:0,peakNodes:0};
 const seen=new Set();let running=true,last;
 const observer=new MutationObserver(records=>{for(const record of records)for(const key of ['addedNodes','removedNodes'])for(const node of record[key])if(node.nodeType===1){const count=(node.matches('.artwork-choice')?1:0)+node.querySelectorAll('.artwork-choice').length;metrics[key==='addedNodes'?'addedChoices':'removedChoices']+=count;}});
 observer.observe(document.body,{childList:true,subtree:true});
 const longObserver=new PerformanceObserver(list=>metrics.longTasks.push(...list.getEntries().map(e=>({start:e.startTime,duration:e.duration}))));longObserver.observe({type:'longtask'});
 const sample=t=>{if(!running)return;if(last!==undefined)metrics.frames.push(t-last);last=t;metrics.peakHeap=Math.max(metrics.peakHeap,performance.memory?.usedJSHeapSize??0);metrics.peakNodes=Math.max(metrics.peakNodes,document.querySelectorAll('*').length);requestAnimationFrame(sample);};
 const state=window.__perf;state.calls=[];state.peakSearch=0;state.peakPreview=0;state.peakPending=0;
 const start=performance.now();requestAnimationFrame(sample);
 document.querySelector(config.kind==='cardback'?'.common-back-change':'.card-change-art').click();
 await wait(()=>document.querySelector('.artwork-choice'),'first results');
 const columns=getComputedStyle(document.querySelector('.artwork-grid')).gridTemplateColumns.split(' ').length;
 const getChoice=index=>[...document.querySelectorAll('.artwork-choice')].find(b=>b.querySelector('strong')?.textContent==='Island art '+index);
 async function view(index){
   const step={index,start:performance.now(),pageWait:0,previewWait:0};
   if(!getChoice(index)){
     const before=performance.now();
     if(document.querySelector('.gallery-load-more'))document.querySelector('.artwork-gallery .editor-body').scrollTop=1e8;
     else {const next=buttons().find(b=>b.closest('.artwork-gallery')&&b.textContent.trim()==='Next');if(!next||next.disabled)throw Error('Next page unavailable '+index);metrics.pageActivations++;next.click();}
     await wait(()=>getChoice(index),'option '+index);step.pageWait=performance.now()-before;metrics.pageWaitMs+=step.pageWait;
   }
   getChoice(index).scrollIntoView({block:'center'});await frame();
   const before=performance.now();
   await wait(()=>{
     const bounds=document.querySelector('.artwork-gallery .editor-body').getBoundingClientRect();
     const visible=[...document.querySelectorAll('.artwork-choice')].filter(b=>{const r=b.getBoundingClientRect();return r.top<bounds.bottom&&r.bottom>bounds.top});
     return visible.length&&visible.every(b=>{const img=b.querySelector('img');return img?.complete&&img.naturalWidth>0});
   },'visible previews');
   step.previewWait=performance.now()-before;metrics.previewWaitMs+=step.previewWait;
   await frame();
   const bounds=document.querySelector('.artwork-gallery .editor-body').getBoundingClientRect();
   for(const b of document.querySelectorAll('.artwork-choice')){const r=b.getBoundingClientRect();if(r.top<bounds.bottom&&r.bottom>bounds.top&&b.querySelector('img')?.naturalWidth)seen.add(Number(b.querySelector('strong').textContent.match(/(\d+)$/)[1]));}
   step.end=performance.now();metrics.steps.push(step);
 }
 for(let page=0;page<Math.ceil(config.options/24);page++)for(let row=0;row<24&&page*24+row<config.options;row+=columns)await view(page*24+row);
 if(!getChoice(1)){
   const input=document.querySelector('[aria-label="Gallery page"]');
   if(!input)throw Error('Cannot revisit first artwork');
   input.value=1;input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));metrics.pageActivations++;
   await wait(()=>getChoice(1),'first page return');
 }
 await view(1);
 getChoice(1).click();
 await wait(()=>!document.querySelector('.artwork-gallery')&&state.chosen&&state.pending===0,'selection and async drain');
 await frame();
 metrics.durationMs=performance.now()-start;running=false;observer.disconnect();longObserver.disconnect();
 metrics.seen=[...seen].sort((a,b)=>a-b);metrics.columns=columns;metrics.calls=state.calls;metrics.peakSearch=state.peakSearch;metrics.peakPreview=state.peakPreview;metrics.peakPending=state.peakPending;metrics.pending=state.pending;metrics.previewWaiting=state.previewWaiting;metrics.chosen=state.chosen;metrics.finalName=document.querySelector(config.kind==='cardback'?'.common-back-info strong':'.card-name').textContent;
 if(metrics.seen.length!==config.options)throw Error('Required artwork not viewed: '+metrics.seen.length+'/'+config.options);
 if(state.pending!==0||state.previewWaiting!==0)throw Error('Work left pending');
 return metrics;
};
