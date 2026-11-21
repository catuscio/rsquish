'use strict';
window.I18n = (() => {
  const messages = window.RsquishMessages;
  const storageKey = 'rsquish.language';
  const supported = value => value === 'ko' || value === 'en';
  let language;
  try { language = localStorage.getItem(storageKey); } catch { /* Storage can be unavailable. */ }
  if (!supported(language)) language = navigator.language?.toLowerCase().startsWith('ko') ? 'ko' : 'en';
  const recent = new Map();
  const bindings = new Map();
  function t(key, values = {}) {
    const template = language === 'en' && Object.hasOwn(messages, key) ? messages[key] : key;
    const result = template.replace(/\{(\w+)\}/g, (match, name) => String(name === 'error' && values[name] != null ? localize(String(values[name])) : values[name] ?? match));
    // Retain message arguments for live announcements without retaining image state.
    recent.delete(result);
    recent.set(result, { key, values });
    if (recent.size > 256) recent.delete(recent.keys().next().value);
    return result;
  }
  function localize(message = '') {
    if (Object.hasOwn(window.RsquishCoreMessages, message)) return t(window.RsquishCoreMessages[message]);
    if (Object.hasOwn(messages, message)) return t(message);
    for (const [prefix, key] of [
      ['Image saved, but report sidecar could not be saved:', '이미지는 저장했지만 JSON 처리 기록을 저장하지 못했습니다. {error}'],
      ['미리보기 실행 오류:', '미리보기 실행 오류: {error}'],
      ['작업 실행 오류:', '작업 실행 오류: {error}'],
    ]) if (message.startsWith(prefix)) return t(key, { error: message.slice(prefix.length).trim() });
    return message;
  }
  function bindRendered(element, text) {
    bindings.set(element, recent.get(text) ?? { key: text, values: {} });
    element.textContent = text;
  }
  function apply(root = document) {
    for (const element of root.querySelectorAll('[data-i18n]')) element.textContent = t(element.dataset.i18n);
    for (const attribute of ['aria-label', 'alt', 'placeholder', 'content']) {
      for (const element of root.querySelectorAll(`[data-i18n-${attribute}]`)) {
        element.setAttribute(attribute, t(element.getAttribute(`data-i18n-${attribute}`)));
      }
    }
    for (const [element, message] of bindings) {
      if (!element.isConnected) { bindings.delete(element); continue; }
      element.textContent = t(message.key, message.values);
    }
    document.documentElement.lang = language;
  }
  function setLanguage(value) {
    if (!supported(value)) return;
    language = value;
    try { localStorage.setItem(storageKey, value); } catch { /* The current session still changes. */ }
    apply();
    window.dispatchEvent(new Event('rsquish:languagechange'));
  }
  return { t, localize, bindRendered, apply, setLanguage, get language() { return language; } };
})();
