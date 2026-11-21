'use strict';
const t = I18n.t;
const $ = (id) => document.getElementById(id);
const native = Boolean(window.__TAURI__?.core?.invoke);
const invoke = (...args) => window.__TAURI__.core.invoke(...args);
const state = { items: [], outputDir: '', busy: false, stop: false, batchIds: new Set(), workers: 1 };
const labels = { pending:'대기', running:'처리 중', saved:'저장 완료', constraint_unmet:'조건 미달', cancelled:'취소됨', time_limit:'시간 제한', error:'오류' };
const generalCodecNames={mozjpeg:'JPEG',webp:'WebP','avif-iq':'AVIF','avif-ssim':'AVIF'};
const codecNames = { mozjpeg:'JPEG', webp:'WebP', 'avif-iq':'AVIF · IQ', 'avif-ssim':'AVIF · SSIM' };
function localize(message) { return I18n.localize(message); }
function progressMessage(payload) {
  const codec=generalCodecNames[payload.codec]||payload.codec||'';
  if(payload.stage==='prepare')return t('기준 이미지를 준비하고 있습니다.');
  if(payload.stage==='encode')return t('{codec} 파일을 줄이는 중',{codec});
  if(payload.stage==='evaluate')return t('{codec} 선명함을 확인하는 중',{codec});
  return localize(payload.message);
}
const formatBytes = (n) => n < 1000 ? `${n} B` : n < 1000000 ? `${(n/1000).toFixed(1)} kB` : `${(n/1000000).toFixed(2)} MB`;
function node(tag, className, text) { const el=document.createElement(tag); if(className)el.className=className; if(text!==undefined)el.textContent=text; return el; }
function metrics(values, className='file-note metric-list') {
  const line=node('p',className);
  for(const value of values)line.append(node('span','metric-item',value));
  return line;
}
function error(message) { I18n.bindRendered($('error'),localize(message)); $('error').hidden=!message; }
function announce(message) { I18n.bindRendered($('status'),message); }
function render() {
  $('empty').hidden=state.items.length>0;
  $('queue-count').textContent=t('사진 {count}개',{count:state.items.length});
  window.RasterPreview.beginRender();
  const list=$('queue'); list.replaceChildren();
  for(const item of state.items){
    window.RasterPreview.sync(item);
    const row=node('li','file-row');
    row.append(window.RasterPreview.button(item));
    const body=node('div');body.append(node('div','file-name',item.name));
    body.append(node('p','file-details mono',t('원본 {size}',{size:formatBytes(item.bytes)})));
    const result=node('div','file-result');
    const kind=item.status==='saved'?'success':item.status==='error'?'error':['constraint_unmet','time_limit','cancelled'].includes(item.status)?'warning':'';
    result.append(node('span',`badge ${kind}`,t(labels[item.status] || item.status)));
    const report=item.report;
    if(report?.outputBytes!=null){
      const percent=item.bytes>0?(1-report.outputBytes/item.bytes)*100:0;
      result.append(metrics([formatBytes(report.outputBytes),`${percent>=0?'−':'+'}${Math.abs(percent).toFixed(1)}%`],'mono metric-list'));
    }
    body.append(result);
    if(report){
      const details=[];
      if(report.codec)details.push(generalCodecNames[report.codec]||report.codec);
      details.push(t('{seconds}초',{seconds:(report.elapsedMs/1000).toFixed(1)}));
      body.append(metrics(details));
      const info=node('details','processing-info');info.open=Boolean(item.detailsOpen);info.addEventListener('toggle',()=>{item.detailsOpen=info.open;});info.append(node('summary',null,t('처리 정보')));
      if(report.score!=null)info.append(metrics([`SSIMULACRA2 ${report.score.toFixed(2)}`,codecNames[report.codec]||report.codec||'']));
      if(report.outputPath){const path=node('p','file-note',t('저장 위치 · {path}',{path:report.outputPath}));path.title=report.outputPath;body.append(path);}
      for(const warning of report.warnings||[])info.append(node('p','file-note',localize(warning)));
      info.append(metrics([`${report.width} × ${report.height}px`,t('인코딩 {count}회',{count:report.encodeCount}),t('평가 재사용 {count}회',{count:report.metricCacheHits})]));
      body.append(info);
      if(report.outputPath){const compare=node('button','compare-button',t('전/후 비교'));compare.type='button';compare.addEventListener('click',()=>window.RasterPreview.open(item,compare));body.append(compare);}
      if(report.status==='constraint_unmet')body.append(node('p','file-note',t('탐색한 후보 중 조건에 맞는 결과가 없어 저장하지 않았습니다.')));
    }
    if(item.error)body.append(node('p','file-note',localize(item.error)));
    if(item.status==='running'&&item.progress)body.append(node('p','file-note',progressMessage(item.progress)));
    row.append(body);
    const remove=node('button','quiet remove',t('제거'));remove.type='button';remove.disabled=state.busy;remove.setAttribute('aria-label',t('{name} 목록에서 제거',{name:item.name}));
    remove.addEventListener('click',()=>{window.RasterPreview.remove(item.id);state.items=state.items.filter(x=>x.id!==item.id);render();});row.append(remove);list.append(row);
  }
  const saved=state.items.filter(x=>x.report?.status==='saved');
  const before=saved.reduce((s,x)=>s+x.bytes,0),after=saved.reduce((s,x)=>s+x.report.outputBytes,0);
  $('summary').textContent=saved.length?t('{count}개 저장 · {before} → {after}',{count:saved.length,before:formatBytes(before),after:formatBytes(after)}):t('원본 파일은 변경하지 않습니다.');
  $('add').disabled=!native||state.busy;$('add-empty').disabled=!native||state.busy;
  $('clear').disabled=state.busy||state.items.length===0;
  $('settings-fields').disabled=state.busy;
  $('folder').textContent=t(state.outputDir?'저장 폴더 변경':'저장 폴더 선택');
  $('folder').disabled=!native||state.busy;
  $('start').disabled=!native||state.busy||!state.outputDir||!state.items.some(x=>x.status==='pending');
  $('cancel').hidden=!state.busy;$('cancel').disabled=state.stop;
  $('cancel').textContent=state.stop?t('취소 중…'):t('취소');
  $('start').textContent=state.busy?t('최적화 중…'):t('최적화 시작');
  $('start').setAttribute('aria-busy',String(state.busy));
}
async function addImages(){
  error('');try{
    const files=await invoke('pick_images',{language:I18n.language});
    for(const file of files){if(!state.items.some(x=>x.path===file.path))state.items.push({...file,id:crypto.randomUUID(),status:'pending'});}
    render();if(files.length)announce(t('{count}개의 사진이 준비되었습니다.',{count:state.items.length}));
  }catch(e){error(String(e));}
}
const presets={small:{label:'용량 줄이기',score:75,width:'1600'},balanced:{label:'균형 있게',score:85,width:'2400'},sharp:{label:'선명하게',score:92,width:''}};
const formatCodecs={auto:['webp','avif-iq','avif-ssim'],jpeg:['mozjpeg'],webp:['webp'],avif:['avif-iq','avif-ssim']};
const formatHints={auto:'WebP와 AVIF를 비교해 조건에 맞는 결과를 고릅니다.',jpeg:'오래된 앱과 기기에서도 열기 쉬운 형식입니다.',webp:'웹에 올릴 사진에 널리 쓰이는 형식입니다.',avif:'작은 파일을 만들기 좋지만 일부 앱에서는 열리지 않을 수 있습니다.',custom:'고급 설정에서 압축 방식을 직접 선택했습니다.'};
function updateSummary(){
  const selected=document.querySelector('input[name=preset]:checked');
  const preset=presets[selected?.value||'balanced'];
  const width=$('width').value.trim();
  const codecs=[...document.querySelectorAll('input[name=codec]:checked')].map(x=>x.value).join(',');
  const changed=Number($('target').value)!==preset.score||$('target').value.trim()===''||width!==preset.width||document.querySelector('input[name=mode]:checked').value!=='target'||$('format').value!=='auto'||$('budget').value!=='200'||$('budget-unit').value!=='3'||$('probes').value!=='7'||$('seconds').value!=='120'||codecs!=='webp,avif-iq,avif-ssim';
  $('preset-summary').textContent=`${t(preset.label)}${changed?t(' · 일부 설정 변경'):''} · ${width?t('너비 최대 {width}px',{width:Number(width).toLocaleString(I18n.language)}):t('원본 크기')}`;
  $('format-hint').textContent=t(formatHints[$('format').value]);
}
function markCustom(){updateSummary();}
function setFormat(value){$('format').value=value;for(const checkbox of document.querySelectorAll('input[name=codec]'))checkbox.checked=formatCodecs[value].includes(checkbox.value);updateSummary();}
function applyPreset(key){
  const preset=presets[key];$('target').value=preset.score;$('width').value=preset.width;
  for(const id of ['target','width'])$(id).removeAttribute('aria-invalid');
  updateSummary();
}
function modeChanged(){const budget=document.querySelector('input[name=mode]:checked').value==='budget';$('budget-field').hidden=!budget;$('target-field').hidden=budget;$('mode-hint').hidden=budget;}
function invalidField(id,message){const issue=new Error(message);issue.field=id;throw issue;}
function parseBudgetBytes(value,power){
  const text=value.trim();
  if(!/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(text))invalidField('budget',t('파일 크기는 0보다 큰 숫자로 입력하세요.'));
  const [whole,fraction='']=text.split('.');
  const digits=fraction.replace(/0+$/,'');
  if(digits.length>power)invalidField('budget',t('파일 크기의 소수 자릿수를 줄여 주세요.'));
  const bytes=BigInt(whole||'0')*10n**BigInt(power)+BigInt((digits||'').padEnd(power,'0')||'0');
  if(bytes<1n||bytes>BigInt(Number.MAX_SAFE_INTEGER))invalidField('budget',t('파일 크기를 더 크거나 작은 값으로 조절해 주세요.'));
  return Number(bytes);
}
function requestSettings(){
  const mode=document.querySelector('input[name=mode]:checked').value;
  const targetScore=Number($('target').value);
  if(mode==='budget'&&$('budget').validity.badInput)invalidField('budget',t('파일 크기는 0보다 큰 숫자로 입력하세요.'));
  const maxBytes=mode==='budget'?parseBudgetBytes($('budget').value,Number($('budget-unit').value)):200000;
  const maxWidth=$('width').value.trim()===''?null:Number($('width').value);
  const codecs=[...document.querySelectorAll('input[name=codec]:checked')].map(x=>x.value);
  if(mode==='target'&&($('target').value.trim()===''||$('target').validity.badInput||!Number.isFinite(targetScore)||targetScore<0||targetScore>100))invalidField('target',t('화질 비교 점수는 0~100 사이로 입력하세요.'));
  if($('width').validity.badInput||(maxWidth!==null&&(!Number.isInteger(maxWidth)||maxWidth<8||maxWidth>20000)))invalidField('width',t('최대 너비는 8~20,000px 사이로 입력하세요.'));
  const probes=Number($('probes').value),maxSeconds=Number($('seconds').value);
  if($('probes').value.trim()===''||$('probes').validity.badInput||!Number.isInteger(probes)||probes<3||probes>32)invalidField('probes',t('탐색 횟수는 3~32회 사이로 입력하세요.'));
  if($('seconds').value.trim()===''||$('seconds').validity.badInput||!Number.isInteger(maxSeconds)||maxSeconds<1||maxSeconds>3600)invalidField('seconds',t('탐색 시간은 1~3,600초 사이로 입력하세요.'));
  if(!codecs.length){const issue=new Error(t('고급 설정에서 압축 방식을 하나 이상 선택하세요.'));issue.field='codec';throw issue;}
  return{mode,targetScore:mode==='target'?targetScore:85,maxBytes,maxWidth,codecs,probes,maxSeconds,outputDir:state.outputDir};
}
async function start(){
  error('');for(const id of ['target','budget','width','probes','seconds'])$(id).removeAttribute('aria-invalid');
  let settings;try{settings=requestSettings();}catch(e){error(e.message);if(e.field){if(e.field!=='budget')$('advanced').open=true;const field=e.field==='codec'?document.querySelector('input[name=codec]'):$(e.field);field.setAttribute('aria-invalid','true');field.focus();}return;}
  state.busy=true;state.stop=false;render();
  const batch=state.items.filter(x=>x.status==='pending');state.batchIds=new Set(batch.map(x=>x.id));state.workers=1;
  try{
    const result=await invoke('optimize_batch',{items:batch.map(item=>({id:item.id,request:{...settings,inputPath:item.path}})),concurrency:Number($('concurrency').value)});
    state.workers=result.plan.jobs;
    for(const entry of result.results)applyResult(entry);
    const finished=batch.filter(x=>x.status!=='pending').length;
    announce(state.stop?t('작업을 중지했습니다. {count}개 처리 · 남은 사진은 대기 중입니다.',{count:finished}):t('{count}개 처리 완료 · 최대 {workers}개 동시 처리',{count:finished,workers:state.workers}));
  }catch(e){error(String(e));for(const item of batch)if(item.status==='running'){item.status='error';item.error=String(e);}}
  finally{state.busy=false;state.batchIds.clear();$('progress').hidden=true;render();}
}
function applyResult(entry){
  const item=state.items.find(x=>x.id===entry.id);if(!item||!state.batchIds.has(entry.id))return;
  if(entry.report){item.report=entry.report;item.status=entry.report.status;}else{item.status='error';item.error=entry.error;}
}
function batchStatus(){
  const batch=state.items.filter(x=>state.batchIds.has(x.id));
  const running=batch.filter(x=>x.status==='running').length;
  const done=batch.filter(x=>!['pending','running'].includes(x.status)).length;
  if(!state.stop)announce(t('{done}/{total}개 완료 · {running}개 처리 중 · 최대 {workers}개 동시 처리',{done,total:batch.length,running,workers:state.workers}));
}

$('add').addEventListener('click',addImages);$('add-empty').addEventListener('click',addImages);
$('clear').addEventListener('click',()=>{window.RasterPreview.clear();state.items=[];error('');announce(t('사진 목록을 비웠습니다.'));render();});
$('folder').addEventListener('click',async()=>{error('');try{const path=await invoke('pick_output_dir',{language:I18n.language});if(path){state.outputDir=path;$('folder-path').textContent=path;render();}}catch(e){error(String(e));}});
$('start').addEventListener('click',start);
$('cancel').addEventListener('click',async()=>{state.stop=true;render();announce(t('현재 작업의 취소를 요청했습니다.'));try{await invoke('cancel_current');}catch(e){error(t('취소 요청 실패: {error}',{error:localize(String(e))}));}});
for(const radio of document.querySelectorAll('input[name=mode]'))radio.addEventListener('change',()=>{modeChanged();markCustom();});
for(const radio of document.querySelectorAll('input[name=preset]'))radio.addEventListener('change',()=>applyPreset(radio.value));
$('format').addEventListener('change',()=>{setFormat($('format').value);markCustom();});
for(const checkbox of document.querySelectorAll('input[name=codec]'))checkbox.addEventListener('change',()=>{$('format').value='custom';checkbox.removeAttribute('aria-invalid');markCustom();});
for(const id of ['target','budget','width','probes','seconds'])$(id).addEventListener('input',()=>{$(id).removeAttribute('aria-invalid');markCustom();});
$('budget-unit').addEventListener('change',markCustom);
async function initialize(){
  if(native){
    try{await window.__TAURI__.event.listen('optimizer-progress',({payload})=>{
      const item=state.items.find(x=>x.id===payload.id);if(!state.busy||!item||!state.batchIds.has(item.id)||!['pending','running'].includes(item.status))return;
      item.status='running';item.progress=payload;batchStatus();
      $('progress').hidden=true;
      render();
    });
    await window.__TAURI__.event.listen('optimizer-result',({payload})=>{if(!state.busy)return;applyResult(payload);batchStatus();render();});
    await window.__TAURI__.event.listen('batch-plan',({payload})=>{if(state.busy){state.workers=payload.jobs;batchStatus();}});
    }catch(e){error(t('진행 상태 연결 실패: {error}',{error:localize(String(e))}));}
  }else{$('preview').hidden=false;}
  modeChanged();updateSummary();render();
}
I18n.apply();
$('language').value=I18n.language;
$('language').addEventListener('change',e=>I18n.setLanguage(e.target.value));
window.addEventListener('rsquish:languagechange',()=>{$('language').value=I18n.language;updateSummary();render();});
initialize();
