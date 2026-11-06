// Persistent independent WASM workers: no SharedArrayBuffer or COOP/COEP needed.
let workers = [];
let sequence = 0;
let current;
const blank = () => ({ done: false, command: null, score: 0, depth: 0, nodes: 0, cache_hits: 0, rejected: 0, error: null });
function destroy() { for (const worker of workers) worker.terminate(); workers = []; }
function fail(job, error) {
    if (!current || current.id !== job) return;
    current.result = { ...current.result, done: true, error: `TempFish: ${error}` };
    destroy();
}
function receive(index, data) {
    if (!current || data.job !== current.id || current.result.done) return;
    const job = current.id;
    if (data.type === 'ready') {
        current.ready.add(index);
        if (current.ready.size === workers.length) workers[0].postMessage({ type: 'pump', job });
    } else if (data.type === 'work') {
        workers[data.worker + 1].postMessage({ type: 'work', job, work: data.work });
    } else if (data.type === 'report') {
        workers[0].postMessage({ type: 'report', job, worker: index - 1, report: data.report });
    } else if (data.type === 'result') {
        current.result = JSON.parse(data.result);
    } else if (data.type === 'error') fail(job, data.error);
}
export function start(record, nodes) {
    if (current && !current.result.done) destroy();
    const count = Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 2) - 2));
    current = { id: ++sequence, result: blank(), ready: new Set() };
    try {
        if (!workers.length) {
            for (let i = 0; i <= count; i++) {
                const worker = new Worker(new URL('./tempfish-worker.js', import.meta.url), { type: 'module' });
                workers.push(worker);
                worker.onmessage = ({ data }) => { if (workers[i] === worker) receive(i, data); };
                worker.onerror = event => { if (workers[i] === worker) fail(current?.id, event.message || 'worker failed'); };
            }
        }
        workers.forEach((worker, i) => worker.postMessage({ type: 'start', job: current.id, record, nodes, coordinator: i === 0, count: workers.length - 1 }));
        return current.id;
    } catch (error) { destroy(); current = undefined; throw error; }
}
export function poll(id) {
    if (current?.id !== id) throw new Error('TempFish: stale job');
    return JSON.stringify(current.result);
}
export function stop(id) {
    if (current?.id !== id || current.result.done) return;
    current.result.done = true;
    destroy();
}
export function cancel(id) {
    if (current?.id !== id) return;
    if (!current.result.done) destroy();
    current = undefined;
}
export function shutdown() { destroy(); current = undefined; }
