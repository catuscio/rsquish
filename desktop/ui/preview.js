'use strict';
// Native-only image reads. No filesystem URLs or upload endpoints enter the UI.
window.RasterPreview=(()=>{
  const thumbs=new Map();let active=0;const pending=[];let generation=0,returnFocus=null,currentId=null,currentOutputPath=null;
  const native=()=>Boolean(window.__TAURI__?.core?.invoke);
  const read=(path,maxEdge)=>window.__TAURI__.core.invoke('preview_image',{path,maxEdge});
  const itemsByButton=new WeakMap();
  const observer=typeof IntersectionObserver==='function'?new IntersectionObserver(entries=>{
    for(const entry of entries)if(entry.isIntersecting&&entry.target.isConnected)loadThumb(entry.target,itemsByButton.get(entry.target));
  },{rootMargin:'200px'}):null;
  function trim(){
    let bytes=[...thumbs.values()].reduce((n,e)=>n+e.bytes,0);
    while(bytes>4*1024*1024||thumbs.size>64){
      const oldest=[...thumbs].find(([,e])=>e.done);if(!oldest)break;
      thumbs.delete(oldest[0]);bytes-=oldest[1].bytes;
    }
  }
  function drain(){while(active<2&&pending.length){
    const task=pending.shift();if(thumbs.get(task.id)!==task.entry){task.resolve(null);continue;}
    const b=task.entry.button,r=b?.getBoundingClientRect();
    if(observer&&(!b?.isConnected||r.bottom < -200||r.top > innerHeight+200)){thumbs.delete(task.id);task.resolve(null);continue;}
    active++;read(task.path,320).then(task.resolve,task.reject).finally(()=>{active--;drain();});
  }}
  function thumbnail(item,button){
    let entry=thumbs.get(item.id);
    if(entry){entry.button=button;thumbs.delete(item.id);thumbs.set(item.id,entry);return entry.promise;}
    entry={done:false,bytes:0,button};
    entry.promise=new Promise((resolve,reject)=>pending.push({id:item.id,path:item.path,entry,resolve,reject})).then(data=>{
      entry.done=true;entry.bytes=(data?.dataUrl?.length||0)*2;trim();return data;
    },error=>{entry.done=true;trim();throw error;});
    thumbs.set(item.id,entry);drain();return entry.promise;
  }
  function loadThumb(b,item){
    if(!item||b.dataset.thumbRequested)return;
    b.dataset.thumbRequested='true';observer?.unobserve(b);
    thumbnail(item,b).then(data=>{
      if(!b.isConnected)return;
      if(!data){delete b.dataset.thumbRequested;observer?.observe(b);return;}
      const img=document.createElement('img');img.src=data.dataUrl;img.alt='';img.width=data.width;img.height=data.height;b.replaceChildren(img);
    },()=>{if(b.isConnected){b.textContent='미리보기 오류';b.classList.add('preview-failed');}});
  }
  function beginRender(){observer?.disconnect();}
  function button(item){
    const b=document.createElement('button');b.type='button';b.className='thumbnail';b.dataset.previewId=item.id;
    b.setAttribute('aria-label',`${item.name} ${item.report?.outputPath?'전후 비교':'크게 보기'}`);b.textContent='불러오는 중';
    b.addEventListener('click',()=>open(item,b));
    if(!native()){b.textContent='앱에서 보기';b.disabled=true;return b;}
    itemsByButton.set(b,item);b.addEventListener('focus',()=>loadThumb(b,item),{once:true});
    if(observer)observer.observe(b);else loadThumb(b,item);
    return b;
  }
  const dialog=document.createElement('dialog');dialog.id='image-preview';dialog.className='image-dialog';dialog.setAttribute('aria-labelledby','image-preview-title');
  dialog.innerHTML='<div class="dialog-head"><h2 id="image-preview-title">사진 보기</h2><button id="preview-close" type="button">닫기</button></div><p id="preview-status" role="status"></p><div id="comparison-stage" class="comparison-stage" hidden><img id="preview-after" alt="변환 결과"><div id="preview-before-layer" class="before-layer"><img id="preview-before" alt="원본 사진"></div><span id="before-label" class="image-label before-label">원본</span><span id="after-label" class="image-label after-label">결과</span><div id="comparison-divider" class="comparison-divider" aria-hidden="true"></div></div><div id="comparison-controls" hidden><label for="comparison-range">원본과 결과 비교</label><input id="comparison-range" type="range" min="0" max="100" value="50" aria-valuetext="원본 50%, 결과 50%"><div class="comparison-ends"><span>결과만</span><span>원본만</span></div></div><p id="preview-hint" class="hint"></p>';
  document.body.append(dialog);const el=id=>document.getElementById(id);const range=el('comparison-range'),stage=el('comparison-stage');
  function slide(){const n=Number(range.value);el('preview-before-layer').style.clipPath=`inset(0 ${100-n}% 0 0)`;el('comparison-divider').style.left=`${n}%`;range.setAttribute('aria-valuetext',`원본 ${n}%, 결과 ${100-n}%`);}
  range.addEventListener('input',slide);
  function drag(e){if(el('comparison-controls').hidden)return;const r=stage.getBoundingClientRect();range.value=String(Math.round(Math.max(0,Math.min(100,(e.clientX-r.left)/r.width*100))));slide();}
  stage.addEventListener('pointerdown',e=>{if(el('comparison-controls').hidden)return;stage.setPointerCapture(e.pointerId);drag(e);});stage.addEventListener('pointermove',e=>{if(stage.hasPointerCapture(e.pointerId))drag(e);});
  el('preview-close').addEventListener('click',()=>dialog.close());
  dialog.addEventListener('close',()=>{generation++;el('preview-before').removeAttribute('src');el('preview-after').removeAttribute('src');stage.hidden=true;el('comparison-controls').hidden=true;const old=currentId;currentId=null;currentOutputPath=null;const focus=returnFocus?.isConnected?returnFocus:[...document.querySelectorAll('[data-preview-id]')].find(x=>x.dataset.previewId===old);(focus||document.getElementById('add'))?.focus();returnFocus=null;});
  async function open(item,trigger){
    if(!native())return;const token=++generation;currentId=item.id;currentOutputPath=item.report?.outputPath||null;returnFocus=trigger;el('image-preview-title').textContent=item.name;el('preview-status').textContent='사진을 불러오고 있습니다.';el('preview-hint').textContent='';stage.hidden=true;el('comparison-controls').hidden=true;el('preview-before').removeAttribute('src');el('preview-after').removeAttribute('src');if(!dialog.open)dialog.showModal();
    try{
      const resultPath=item.report?.outputPath;
      const [original,result]=await Promise.all([read(item.path,1600),resultPath?read(resultPath,1600):Promise.resolve(null)]);
      if(token!==generation||!dialog.open)return;
      el('preview-before').src=original.dataUrl;el('preview-before').width=original.width;el('preview-before').height=original.height;
      el('preview-after').hidden=!result;el('after-label').hidden=!result;el('comparison-divider').hidden=!result;el('comparison-controls').hidden=!result;
      if(result){el('preview-after').src=result.dataUrl;el('preview-after').width=result.width;el('preview-after').height=result.height;range.value='50';slide();}else el('preview-before-layer').style.clipPath='none';
      stage.hidden=false;el('preview-status').textContent=result?'선을 좌우로 움직여 같은 위치를 비교하세요.':'원본 사진';
      el('preview-hint').textContent=result?'화면에 맞춘 미리보기입니다. 원본과 결과를 같은 영역에 비율대로 표시하며, 크기 차이로 여백이 생길 수 있습니다.':'화면에 맞춘 미리보기입니다. 원본 파일은 변경하지 않습니다.';
    }catch(e){if(token===generation&&dialog.open){el('preview-status').textContent=`미리보기를 불러오지 못했습니다: ${String(e)}`;el('preview-hint').textContent='파일이 이동되었거나 지원하지 않는 이미지일 수 있습니다. 닫은 뒤 다시 열어 주세요.';}}
  }
  function remove(id){thumbs.delete(id);if(currentId===id&&dialog.open)dialog.close();}
  function clear(){observer?.disconnect();thumbs.clear();if(dialog.open)dialog.close();}
  function sync(item){if(dialog.open&&currentId===item.id&&item.report?.outputPath&&currentOutputPath!==item.report.outputPath)open(item,returnFocus);}
  return{button,open,remove,clear,sync,beginRender};
})();
