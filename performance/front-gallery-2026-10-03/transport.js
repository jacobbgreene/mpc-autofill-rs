const cfg=window.__perfConfig;
const events=new Map();
const delay=ms=>new Promise(r=>setTimeout(r,ms));
const state=window.__perf={calls:[],pending:0,searchActive:0,previewActive:0,previewWaiting:0,peakSearch:0,peakPreview:0,peakPending:0,chosen:null,failPage:null,holdPage:null,release:null,missingPreview:null};
const original=()=>({name:'Gallery performance deck',quantity:2,stock:'(S33) Superior Smooth',foil:false,path:'/fixture.xml',dirty:false,cards:[{face:'front',drive_id:'original',name:'Island.png',query:'Island',slots:[0,1],cardback:false,thumb:'/image/original.jpg'},{face:'back',drive_id:'original-back',name:'Common back',slots:[0,1],cardback:true,thumb:'/image/original-back.jpg'}]});
let current=original();
const previewQueue=[];
export function listen(name,handler){const set=events.get(name)||new Set();set.add(handler);events.set(name,set);return Promise.resolve(()=>set.delete(handler));}
const emit=(name,payload)=>{for(const fn of events.get(name)||[])fn({payload});};
export const getCurrentWebview=()=>({onDragDropEvent:fn=>listen('drop',fn)});
export const getCurrentWindow=()=>({onCloseRequested:fn=>listen('close',({payload})=>fn(payload)),close:async()=>{}});
export const convertFileSrc=path=>path;
export const open=async()=>'/fixture.xml';
export const save=async()=>'/fixture.xml';
export const confirm=async()=>true;
async function preview(id){
 if(state.previewActive>=8){state.previewWaiting++;await new Promise(r=>previewQueue.push(r));state.previewWaiting--;}
 state.previewActive++;state.peakPreview=Math.max(state.peakPreview,state.previewActive);
 try{await delay(35);if(state.missingPreview===id)throw Error('Preview fixture unavailable');return '/image/'+id+'.jpg';}
 finally{state.previewActive--;previewQueue.shift()?.();}
}
export async function invoke(cmd,args={}){
 const call={cmd,args,start:performance.now()};state.calls.push(call);state.pending++;state.peakPending=Math.max(state.peakPending,state.pending);
 try{
 if(cmd==='load_order'){current=original();state.chosen=null;return structuredClone(current);}
 if(cmd==='thumbnails'){for(const card of current.cards)if(card.thumb)emit('thumb-ready',{drive_id:card.drive_id,path:card.thumb});return;}
 if(cmd==='artwork_dpi')return Object.fromEntries(args.driveIds.map(id=>[id,1200]));
 if(cmd==='artwork_thumbnail')return await preview(args.driveId);
 if(cmd==='search_artwork'){
  state.searchActive++;state.peakSearch=Math.max(state.peakSearch,state.searchActive);
  try{
   if(state.holdPage===args.page)await new Promise(r=>state.release=r);
   await delay(120);
   if(state.failPage===args.page)throw Error('Search fixture temporarily unavailable');
   const count=state.count??cfg.options;
   return {count,next_page:(args.page+1)*24<count?args.page+1:null,cards:Array.from({length:Math.max(0,Math.min(24,count-args.page*24))},(_,i)=>{const index=args.page*24+i;const actual=state.duplicate&&args.page===1&&i===0?23:index;return {drive_id:args.kind+'-'+args.query+'-'+actual,name:'Island art '+actual+'.png',label:'Island art '+actual,source:'Artist '+actual,dpi:actual%2?800:1200,thumb:''};})};
  }finally{state.searchActive--;}
 }
 if(cmd==='change_artwork'){
  state.chosen=structuredClone(args);
  current={...current,dirty:true,cards:current.cards.map(c=>c.drive_id===args.driveId&&c.face===args.selection.face?{...c,drive_id:args.selection.drive_id,name:args.selection.name,query:args.selection.query,thumb:undefined}:c)};
  return structuredClone(current);
 }
 if(cmd==='save_order'){current={...current,dirty:false};return structuredClone(current);}
 throw Error('Unexpected fixture command '+cmd);
 }finally{call.end=performance.now();state.pending--;}
}
