// Web Audio is unlocked synchronously by a real input event, before Bevy's
// next frame. Decoding is asynchronous; tokens prevent late music from starting.
let context;
let music;
let desired = '';
let queued = false;
let generation = 0;
const buffers = new Map();
const effects = new Set();
function buffer(name) {
    if (!buffers.has(name)) buffers.set(name, fetch(new URL(`./sounds/${name}.ogg`, import.meta.url))
        .then(response => { if (!response.ok) throw new Error(`Audio ${name}: ${response.status}`); return response.arrayBuffer(); })
        .then(bytes => context.decodeAudioData(bytes))
        .catch(error => { buffers.delete(name); throw error; }));
    return buffers.get(name);
}
function report(error) { console.warn('Temporal Chess audio:', error); }
async function playMusic(name, token) {
    try {
        const decoded = await buffer(name);
        if (generation !== token || !desired) return;
        const source = context.createBufferSource();
        source.buffer = decoded;
        source.connect(context.destination);
        music = source;
        source.onended = () => {
            if (generation !== token) return;
            music = undefined;
            if (desired === 'no_future') {
                void playMusic(name === 'no_future' ? 'no_future_reversed' : 'no_future', token);
            }
        };
        source.start();
    } catch (error) { report(error); }
}
function unlock(event) {
    if (!event.isTrusted) return;
    context ??= new AudioContext();
    void context.resume().then(() => {
        if (queued && desired) { queued = false; void playMusic(desired, ++generation); }
    }).catch(report);
}
window.addEventListener('pointerdown', unlock, { capture: true });
window.addEventListener('keydown', unlock, { capture: true });
export function movement(reversed) {
    if (!context || context.state !== 'running') return;
    const name = `move_${reversed ? 'reversed_' : ''}0${1 + Math.floor(Math.random() * 4)}`;
    void buffer(name).then(decoded => {
        if (context.state !== 'running') return;
        const source = context.createBufferSource();
        source.buffer = decoded;
        source.connect(context.destination);
        effects.add(source);
        source.onended = () => effects.delete(source);
        source.start();
    }).catch(report);
}
export function setMusic(name) {
    generation++;
    if (music) { music.onended = null; music.stop(); music = undefined; }
    desired = name;
    queued = !!name;
    if (desired && context?.state === 'running') { queued = false; void playMusic(desired, generation); }
}
