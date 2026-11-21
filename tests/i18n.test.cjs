const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const {resolve} = require('node:path');
const vm = require('node:vm');
const ui = resolve(__dirname, '../desktop/ui');
function load({stored=null, system='en-US', blocked=false}={}) {
  const events=[];
  const context = vm.createContext({
    navigator:{language:system}, Event:class {constructor(type){this.type=type;}},
    localStorage:{getItem(){if(blocked)throw Error();return stored;},setItem(key,value){if(blocked)throw Error();stored=value;}},
    document:{documentElement:{lang:''},querySelectorAll(){return [];}},
    dispatchEvent(event){events.push(event.type);},
  });
  context.window=context;
  for(const file of ['translations.js','i18n.js']) vm.runInContext(readFileSync(resolve(ui,file),'utf8'),context);
  return {api:context.I18n, messages:context.RsquishMessages,context,events,stored:()=>stored};
}
test('saved preference wins; unsupported and unavailable storage fall back safely',()=>{
  assert.equal(load({stored:'ko',system:'en-US'}).api.language,'ko');
  assert.equal(load({stored:'invalid',system:'ko-KR'}).api.language,'ko');
  assert.equal(load({system:'fr-FR'}).api.language,'en');
  const x=load({blocked:true,system:'ko-KR'});x.api.setLanguage('en');assert.equal(x.api.language,'en');
});
test('switches persist, set document language, and ignore unsupported values',()=>{
  const x=load();x.api.setLanguage('ko');
  assert.equal(x.stored(),'ko');assert.equal(x.context.document.documentElement.lang,'ko');
  assert.deepEqual(x.events,['rsquish:languagechange']);x.api.setLanguage('xx');assert.equal(x.api.language,'ko');
});
test('all translations preserve the same interpolation arguments',()=>{
  const {messages}=load();
  const vars=s=>[...s.matchAll(/\{(\w+)\}/g)].map(m=>m[1]).sort();
  for(const [ko,en] of Object.entries(messages)){
    assert.ok(en.trim(),ko);assert.deepEqual(vars(ko),vars(en),ko);
    assert.ok(!/[가-힣]/.test(en),`Untranslated: ${ko}`);
  }
});
test('all static UI bindings have translations',()=>{
  const {messages}=load();
  for(const file of ['index.html','preview.js']){
    for(const match of readFileSync(resolve(ui,file),'utf8').matchAll(/data-i18n(?:-[a-z-]+)?="([^"]+)"/g)){
      assert.ok(Object.hasOwn(messages,match[1]),`${file}: ${match[1]}`);
    }
  }
});
test('live announcements retain variables across language switches without HTML injection',()=>{
  const {api}=load();const element={isConnected:true,textContent:''};
  api.bindRendered(element,api.t('{count}개의 사진이 준비되었습니다.',{count:3}));
  api.setLanguage('ko');assert.equal(element.textContent,'3개의 사진이 준비되었습니다.');
  api.setLanguage('en');assert.equal(element.textContent,'Images ready: 3.');
  assert.equal(api.t('{name} 목록에서 제거',{name:'<img src=x onerror=bad()>'}),'Remove <img src=x onerror=bad()> from the list');
});
test('native warnings and prefixed task errors are localized',()=>{
  const {api}=load({stored:'ko'});
  assert.equal(api.localize('Input exceeds 256 MiB limit'),'원본 파일이 256 MiB 제한을 초과합니다.');
  api.setLanguage('en');assert.equal(api.localize('작업 실행 오류: details'),'Batch task failed: details');
  assert.equal(api.localize('already English'),'already English');
});

test('nested error messages follow later language switches',()=>{
  const {api}=load({stored:'ko'});const element={isConnected:true,textContent:''};
  api.bindRendered(element,api.t('미리보기를 불러오지 못했습니다: {error}',{error:api.localize('Input exceeds 256 MiB limit')}));
  api.setLanguage('en');assert.equal(element.textContent,'Could not load preview: Input exceeds 256 MiB limit');
});

test('unknown native diagnostics never resolve inherited object properties',()=>{
  const {api}=load();
  for(const message of ['__proto__','constructor','toString']){
    assert.equal(api.localize(message),message);assert.equal(api.t(message),message);
  }
});
