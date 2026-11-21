'use strict';
const $ = (id) => document.getElementById(id);
const native = Boolean(window.__TAURI__?.core?.invoke);
const invoke = (...args) => window.__TAURI__.core.invoke(...args);
const state = { items: [], outputDir: '', busy: false, stop: false, batchIds: new Set(), workers: 1 };
const labels = { pending:'대기', running:'처리 중', saved:'저장 완료', constraint_unmet:'조건 미달', cancelled:'취소됨', time_limit:'시간 제한', error:'오류' };
const generalCodecNames={mozjpeg:'JPEG',webp:'WebP','avif-iq':'AVIF','avif-ssim':'AVIF'};
const codecNames = { mozjpeg:'JPEG', webp:'WebP', 'avif-iq':'AVIF · IQ', 'avif-ssim':'AVIF · SSIM' };
const messageTranslations = new Map([
  ['SSIMULACRA2 is an operational score, not a visually lossless guarantee.', 'SSIMULACRA2 점수는 시각적 무손실을 보장하지 않습니다.'],
  ['Output is normalized to sRGB; EXIF/GPS and original metadata are removed.', '결과는 sRGB로 변환하며 EXIF·GPS 등 원본 메타데이터는 제거합니다.'],
  ['Untagged input is interpreted as sRGB.', '색상 프로필이 없는 원본은 sRGB로 해석했습니다.'],
  ['Quality is measured against the resized reference, not the full-resolution input.', '화질 점수는 축소한 기준 이미지와 비교한 값입니다.'],
  ['Time budget reached; saved the best measured feasible candidate. A running codec/metric finishes before checking the deadline.', '탐색 시간에 도달해 조건을 만족한 후보 중 최선의 결과를 저장했습니다. 진행 중인 인코딩·평가가 끝난 뒤 제한을 확인하므로 시간이 초과될 수 있습니다.'],
  ['Selected output is larger than the input; the chosen quality constraint was prioritized.', '설정한 화질 조건을 우선하여 결과가 원본보다 커졌습니다.'],
  ['Input exceeds 256 MiB limit', '원본 파일이 256 MiB 제한을 초과합니다.'],
  ['This version accepts JPEG, PNG and WebP input', 'JPEG, PNG, WebP 파일을 선택하세요.'],
  ['CMYK/YCCK JPEG input requires a separate color-managed path', '현재 CMYK·YCCK JPEG는 지원하지 않습니다. RGB로 변환한 사진을 선택하세요.'],
  ['PNG cICP/HDR color signaling is not supported in this version', '현재 HDR 색상 정보가 있는 PNG는 지원하지 않습니다.'],
  ['Animated input is not supported; no frames were discarded', '움직이는 이미지는 지원하지 않습니다. 원본은 변경하지 않았습니다.'],
  ['Input exceeds 32 megapixel limit', '원본이 3,200만 화소 제한을 초과합니다.'],
  ['16-bit/HDR input is not supported; no precision was discarded', '현재 16비트·HDR 이미지는 지원하지 않습니다. 원본은 변경하지 않았습니다.'],
  ['PNG color metadata needs explicit conversion; use an RGB ICC or sRGB-tagged input', '이 PNG는 색상 변환이 필요합니다. RGB ICC 또는 sRGB 프로필이 있는 파일을 선택하세요.'],
  ['Transparent input requires the future alpha-preserving path', '투명한 이미지는 아직 지원하지 않습니다. 원본은 변경하지 않았습니다.'],
  ['Only RGB ICC profiles are supported in this version', '현재 RGB ICC 색상 프로필만 지원합니다.'],
  ['Metric requires dimensions of at least 8 × 8', '화질 측정에는 가로·세로 각각 8px 이상이 필요합니다.'],
  ['Too many existing output names', '같은 이름의 결과가 너무 많습니다. 다른 저장 폴더를 선택하세요.']
]);
function localize(message) {
  if(messageTranslations.has(message))return messageTranslations.get(message);
  if(message.startsWith('Image saved, but report sidecar could not be saved:'))return '이미지는 저장했지만 JSON 처리 기록을 저장하지 못했습니다. '+message.split(':').slice(1).join(':').trim();
  return message;
}
function progressMessage(payload) {
  const codec=generalCodecNames[payload.codec]||payload.codec||'';
  const suffix='';
  if(payload.stage==='prepare')return '기준 이미지를 준비하고 있습니다.';
  if(payload.stage==='encode')return `${codec} 파일을 줄이는 중${suffix}`;
  if(payload.stage==='evaluate')return `${codec} 선명함을 확인하는 중${suffix}`;
  return localize(payload.message);
}
const formatBytes = (n) => n < 1000 ? `${n} B` : n < 1000000 ? `${(n/1000).toFixed(1)} kB` : `${(n/1000000).toFixed(2)} MB`;
function node(tag, className, text) { const el=document.createElement(tag); if(className)el.className=className; if(text!==undefined)el.textContent=text; return el; }
function metrics(values, className='file-note metric-list') {
  const line=node('p',className);
  for(const value of values)line.append(node('span','metric-item',value));
  return line;
}
function error(message) { $('error').textContent=message; $('error').hidden=!message; }
function announce(message) { $('status').textContent=message; }
function render() {
  $('empty').hidden=state.items.length>0;
  $('queue-count').textContent=`사진 ${state.items.length}개`;
  window.RasterPreview.beginRender();
  const list=$('queue'); list.replaceChildren();
  for(const item of state.items){
    window.RasterPreview.sync(item);
    const row=node('li','file-row');
    row.append(window.RasterPreview.button(item));
    const body=node('div');body.append(node('div','file-name',item.name));
    body.append(node('p','file-details mono',`원본 ${formatBytes(item.bytes)}`));
    const result=node('div','file-result');
    const kind=item.status==='saved'?'success':item.status==='error'?'error':['constraint_unmet','time_limit','cancelled'].includes(item.status)?'warning':'';
    result.append(node('span',`badge ${kind}`,labels[item.status] || item.status));
    const report=item.report;
    if(report?.outputBytes!=null){
      const percent=item.bytes>0?(1-report.outputBytes/item.bytes)*100:0;
      result.append(metrics([formatBytes(report.outputBytes),`${percent>=0?'−':'+'}${Math.abs(percent).toFixed(1)}%`],'mono metric-list'));
    }
    body.append(result);
    if(report){
      const details=[];
      if(report.codec)details.push(generalCodecNames[report.codec]||report.codec);
      details.push(`${(report.elapsedMs/1000).toFixed(1)}초`);
      body.append(metrics(details));
      const info=node('details','processing-info');info.append(node('summary',null,'처리 정보'));
      if(report.score!=null)info.append(metrics([`SSIMULACRA2 ${report.score.toFixed(2)}`,codecNames[report.codec]||report.codec||'']));
      if(report.outputPath){const path=node('p','file-note',`저장 위치 · ${report.outputPath}`);path.title=report.outputPath;body.append(path);}
      for(const warning of report.warnings||[])info.append(node('p','file-note',localize(warning)));
      info.append(metrics([`${report.width} × ${report.height}px`,`인코딩 ${report.encodeCount}회`,`평가 재사용 ${report.metricCacheHits}회`]));
      body.append(info);
      if(report.outputPath){const compare=node('button','compare-button','전/후 비교');compare.type='button';compare.addEventListener('click',()=>window.RasterPreview.open(item,compare));body.append(compare);}
      if(report.status==='constraint_unmet')body.append(node('p','file-note','탐색한 후보 중 조건에 맞는 결과가 없어 저장하지 않았습니다.'));
    }
    if(item.error)body.append(node('p','file-note',localize(item.error)));
    if(item.status==='running'&&item.message)body.append(node('p','file-note',item.message));
    row.append(body);
    const remove=node('button','quiet remove','제거');remove.type='button';remove.disabled=state.busy;remove.setAttribute('aria-label',`${item.name} 목록에서 제거`);
    remove.addEventListener('click',()=>{window.RasterPreview.remove(item.id);state.items=state.items.filter(x=>x.id!==item.id);render();});row.append(remove);list.append(row);
  }
  const saved=state.items.filter(x=>x.report?.status==='saved');
  const before=saved.reduce((s,x)=>s+x.bytes,0),after=saved.reduce((s,x)=>s+x.report.outputBytes,0);
  $('summary').textContent=saved.length?`${saved.length}개 저장 · ${formatBytes(before)} → ${formatBytes(after)}`:'원본 파일은 변경하지 않습니다.';
  $('add').disabled=!native||state.busy;$('add-empty').disabled=!native||state.busy;
  $('clear').disabled=state.busy||state.items.length===0;
  $('settings-fields').disabled=state.busy;
  $('folder').disabled=!native||state.busy;
  $('start').disabled=!native||state.busy||!state.outputDir||!state.items.some(x=>x.status==='pending');
  $('cancel').hidden=!state.busy;$('cancel').disabled=state.stop;
  $('cancel').textContent=state.stop?'취소 중…':'취소';
  $('start').textContent=state.busy?'최적화 중…':'최적화 시작';
  $('start').setAttribute('aria-busy',String(state.busy));
}
async function addImages(){
  error('');try{
    const files=await invoke('pick_images');
    for(const file of files){if(!state.items.some(x=>x.path===file.path))state.items.push({...file,id:crypto.randomUUID(),status:'pending'});}
    render();if(files.length)announce(`${state.items.length}개의 사진이 준비되었습니다.`);
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
  $('preset-summary').textContent=`${preset.label}${changed?' · 일부 설정 변경':''} · ${width?`너비 최대 ${Number(width).toLocaleString('ko-KR')}px`:'원본 크기'}`;
  $('format-hint').textContent=formatHints[$('format').value];
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
  if(!/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(text))invalidField('budget','파일 크기는 0보다 큰 숫자로 입력하세요.');
  const [whole,fraction='']=text.split('.');
  const digits=fraction.replace(/0+$/,'');
  if(digits.length>power)invalidField('budget','파일 크기의 소수 자릿수를 줄여 주세요.');
  const bytes=BigInt(whole||'0')*10n**BigInt(power)+BigInt((digits||'').padEnd(power,'0')||'0');
  if(bytes<1n||bytes>BigInt(Number.MAX_SAFE_INTEGER))invalidField('budget','파일 크기를 더 크거나 작은 값으로 조절해 주세요.');
  return Number(bytes);
}
function requestSettings(){
  const mode=document.querySelector('input[name=mode]:checked').value;
  const targetScore=Number($('target').value);
  if(mode==='budget'&&$('budget').validity.badInput)invalidField('budget','파일 크기는 0보다 큰 숫자로 입력하세요.');
  const maxBytes=mode==='budget'?parseBudgetBytes($('budget').value,Number($('budget-unit').value)):200000;
  const maxWidth=$('width').value.trim()===''?null:Number($('width').value);
  const codecs=[...document.querySelectorAll('input[name=codec]:checked')].map(x=>x.value);
  if(mode==='target'&&($('target').value.trim()===''||$('target').validity.badInput||!Number.isFinite(targetScore)||targetScore<0||targetScore>100))invalidField('target','화질 비교 점수는 0~100 사이로 입력하세요.');
  if($('width').validity.badInput||(maxWidth!==null&&(!Number.isInteger(maxWidth)||maxWidth<8||maxWidth>20000)))invalidField('width','최대 너비는 8~20,000px 사이로 입력하세요.');
  const probes=Number($('probes').value),maxSeconds=Number($('seconds').value);
  if($('probes').value.trim()===''||$('probes').validity.badInput||!Number.isInteger(probes)||probes<3||probes>32)invalidField('probes','탐색 횟수는 3~32회 사이로 입력하세요.');
  if($('seconds').value.trim()===''||$('seconds').validity.badInput||!Number.isInteger(maxSeconds)||maxSeconds<1||maxSeconds>3600)invalidField('seconds','탐색 시간은 1~3,600초 사이로 입력하세요.');
  if(!codecs.length){const issue=new Error('고급 설정에서 압축 방식을 하나 이상 선택하세요.');issue.field='codec';throw issue;}
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
    announce(state.stop?`작업을 중지했습니다. ${finished}개 처리 · 남은 사진은 대기 중입니다.`:`${finished}개 처리 완료 · 최대 ${state.workers}개 동시 처리`);
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
  if(!state.stop)announce(`${done}/${batch.length}개 완료 · ${running}개 처리 중 · 최대 ${state.workers}개 동시 처리`);
}

$('add').addEventListener('click',addImages);$('add-empty').addEventListener('click',addImages);
$('clear').addEventListener('click',()=>{window.RasterPreview.clear();state.items=[];error('');announce('사진 목록을 비웠습니다.');render();});
$('folder').addEventListener('click',async()=>{error('');try{const path=await invoke('pick_output_dir');if(path){state.outputDir=path;$('folder-path').textContent=path;$('folder').textContent='저장 폴더 변경';render();}}catch(e){error(String(e));}});
$('start').addEventListener('click',start);
$('cancel').addEventListener('click',async()=>{state.stop=true;render();announce('현재 작업의 취소를 요청했습니다.');try{await invoke('cancel_current');}catch(e){error(`취소 요청 실패: ${e}`);}});
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
      item.status='running';item.message=progressMessage(payload);batchStatus();
      $('progress').hidden=true;
      render();
    });
    await window.__TAURI__.event.listen('optimizer-result',({payload})=>{if(!state.busy)return;applyResult(payload);batchStatus();render();});
    await window.__TAURI__.event.listen('batch-plan',({payload})=>{if(state.busy){state.workers=payload.jobs;batchStatus();}});
    }catch(e){error(`진행 상태 연결 실패: ${e}`);}
  }else{$('preview').hidden=false;}
  modeChanged();updateSummary();render();
}
initialize();
