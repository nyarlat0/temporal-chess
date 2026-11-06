// Functional worker test using real WASM and real threads, without rendering.
// Usage: node tools/test-tempfish-workers.mjs dist/web/releases/<release>
import assert from 'node:assert/strict';
import { Worker as NodeWorker, parentPort, workerData, isMainThread } from 'node:worker_threads';
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
if (!isMainThread) {
    globalThis.self = globalThis;
    globalThis.postMessage = value => parentPort.postMessage(value);
    const originalFetch = globalThis.fetch;
    globalThis.fetch = async (url, options) => {
        if (String(url).startsWith('file:')) return new Response(await readFile(new URL(url)), { headers: { 'Content-Type': 'application/wasm' } });
        return originalFetch(url, options);
    };
    await import(workerData.url);
    parentPort.on('message', data => self.onmessage({ data }));
} else {
    const location = pathToFileURL(resolve(process.argv[2]) + '/');
    Object.defineProperty(globalThis, 'navigator', { value: { hardwareConcurrency: 5 } });
    globalThis.Worker = class {
        constructor(url) {
            this.worker = new NodeWorker(new URL(import.meta.url), { workerData: { url: String(url) } });
            this.worker.on('message', data => this.onmessage?.({ data }));
            this.worker.on('error', error => this.onerror?.({ message: error.message }));
        }
        postMessage(value) { this.worker.postMessage(value); }
        terminate() { this.worker.terminate(); }
    };
    const pool = await import(new URL('tempfish-pool.js', location));
    const header = 'NOLAN-CHESS 4\nchain_info_activation off\nglobal_reaction_reset off\n';
    async function wait(id, predicate) {
        const deadline = Date.now() + 60000;
        for (;;) {
            const result = JSON.parse(pool.poll(id));
            assert.equal(result.error, null, result.error);
            if (predicate(result)) return result;
            assert.ok(Date.now() < deadline, 'worker search timed out');
            await new Promise(resolve => setTimeout(resolve, 10));
        }
    }
    try {
        let record = header + 'start standard\n';
        for (let i = 0; i < 3; i++) {
            const id = pool.start(record, 200, [1, 3, 6][i]);
            const result = await wait(id, r => r.done);
            assert.ok(result.nodes <= 200);
            assert.ok(result.command);
            assert.ok(result.depth >= 1);
            record += result.command + '\n';
            pool.cancel(id); // Keep idle workers; next start reuses the journal.
        }
        const obsolete = pool.start(record, 100000);
        const replacement = pool.start(header + 'start time_war 5\n', 100000);
        assert.throws(() => pool.poll(obsolete), /stale/);
        const progress = await wait(replacement, r => r.depth >= 1);
        pool.stop(replacement);
        const stopped = JSON.parse(pool.poll(replacement));
        assert.equal(stopped.done, true);
        assert.ok(stopped.nodes <= 100000);
        assert.equal(stopped.command, progress.command);
        assert.ok(stopped.command);
        const id = pool.start(header + 'start time_war 5\n', 50, 1);
        const final = await wait(id, r => r.done);
        assert.ok(final.nodes <= 50 && final.command);
        console.log('Worker/WASM checks passed: parallel search, replay reuse, global budgets, cancellation, stale replies, Time War.');
    } finally { pool.shutdown(); }
}
