// Deterministic Web Audio lifecycle checks; no rendering or audio device needed.
import assert from 'node:assert/strict';
const listeners = new Map();
globalThis.window = { addEventListener: (name, fn) => listeners.set(name, fn) };
const sources = [];
let context;
globalThis.AudioContext = class {
    constructor() { context = this; this.state = 'suspended'; this.destination = {}; }
    resume() { this.state = 'running'; return Promise.resolve(); }
    decodeAudioData(bytes) { return Promise.resolve(bytes); }
    createBufferSource() {
        const source = { connect() {}, start() { sources.push(this); }, stop() { this.stopped = true; }, end() { this.onended?.(); } };
        return source;
    }
};
globalThis.fetch = async url => ({ ok: true, arrayBuffer: async () => String(url).split('/').at(-1) });
const audio = await import('../crates/bevy_front/web/sound.js');
const flush = () => new Promise(resolve => setImmediate(resolve));
audio.setMusic('no_future');
assert.equal(sources.length, 0);
listeners.get('pointerdown')({ isTrusted: true });
await flush();
assert.equal(context.state, 'running');
assert.equal(sources.at(-1).buffer, 'no_future.ogg');
sources.at(-1).end(); await flush();
assert.equal(sources.at(-1).buffer, 'no_future_reversed.ogg');
sources.at(-1).end(); await flush();
assert.equal(sources.at(-1).buffer, 'no_future.ogg');
const playing = sources.at(-1);
audio.setMusic(''); assert.equal(playing.stopped, true);
const count = sources.length;
playing.end(); await flush(); assert.equal(sources.length, count);
audio.setMusic('no_future'); await flush();
assert.equal(sources.at(-1).buffer, 'no_future.ogg');
audio.setMusic('victory_reversed'); await flush();
assert.equal(sources.at(-1).buffer, 'victory_reversed.ogg');
sources.at(-1).end(); await flush();
const finished = sources.length;
listeners.get('keydown')({ isTrusted: true }); await flush();
assert.equal(sources.length, finished, 'finished finale must not restart on input');
audio.movement(false); audio.movement(true); await flush();
assert.match(sources.at(-2).buffer, /^move_0[1-4]\.ogg$/);
assert.match(sources.at(-1).buffer, /^move_reversed_0[1-4]\.ogg$/);
// Cancel a track while its asynchronous loading is still pending.
audio.setMusic('no_future'); audio.setMusic(''); await flush();
assert.equal(sources.at(-1).buffer.startsWith('move_'), true);
console.log('Audio checks passed: gesture unlock, both move banks, alternating music, stop/restart, one-shot finales, cancelled loads.');
