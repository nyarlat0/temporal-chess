// Exercise the real page bootstrap without starting a browser or rendering UI.
const { readFileSync } = require('node:fs');
const { runInNewContext } = require('node:vm');
const assert = require('node:assert/strict');
const html = readFileSync('crates/bevy_front/web/index.html.in', 'utf8');
const bootstrap = html.match(/<script>([\s\S]*?)<\/script>/)[1];
function boot(saved, languages, language = 'en', unavailable = false) {
  const context = {
    document: { documentElement: { lang: 'en' } },
    navigator: { languages, language },
    localStorage: {
      getItem(key) {
        assert.equal(key, 'temporal-chess.language');
        if (unavailable) throw new Error('Storage blocked');
        return saved;
      },
      setItem() { assert.fail('Automatic detection must not persist a preference'); },
    },
  };
  runInNewContext(bootstrap, context);
  return context;
}
for (const [saved, languages, expected] of [
  [null, ['ru-RU', 'en-US'], 'ru'],
  [null, ['en-GB', 'ru'], 'en'],
  [null, ['de-DE', 'ru-UA', 'en'], 'ru'],
  [null, ['fr', 'de'], 'en'],
  ['en', ['ru'], 'en'],
  ['ru', ['en'], 'ru'],
  ['invalid', ['ru'], 'ru'],
  [null, ['RU-ru'], 'ru'],
]) assert.equal(boot(saved, languages).document.documentElement.lang, expected);
assert.equal(boot(null, undefined, 'ru-RU').document.documentElement.lang, 'ru');
assert.equal(boot(null, [], 'en-US').document.documentElement.lang, 'en');
assert.equal(boot(null, ['ru'], 'en', true).document.documentElement.lang, 'ru');
const page = boot('en', ['ru']);
assert.equal(page.pageText('Русский', 'English'), 'English');
page.document.documentElement.lang = 'ru';
assert.equal(page.pageText('Русский', 'English'), 'Русский');
console.log('Language detection, saved preference, blocked storage and live page text: passed.');
