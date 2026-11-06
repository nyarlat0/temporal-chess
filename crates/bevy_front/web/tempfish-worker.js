import init, { Session } from './tempfish.js';
let session;
let epoch = 0;
let count = 0;
const ready = init().then(() => { session = new Session(); });
function pump(job) {
    if (job !== epoch) return;
    for (let id = 0; id < count; id++) {
        const work = session.next_work(id);
        if (work !== 'null') self.postMessage({ type: 'work', job, worker: id, work });
    }
    self.postMessage({ type: 'result', job, result: session.result() });
}
self.onmessage = async ({ data }) => {
    try {
        await ready;
        if (data.type === 'start') {
            epoch = data.job; count = data.count;
            session.start(data.record, data.nodes, data.coordinator);
            if (data.coordinator) {
                // Yield while generating the root so a replaced job can cancel.
                const prepare = () => {
                    if (epoch !== data.job) return;
                    if (session.prepare()) self.postMessage({ type: 'ready', job: epoch });
                    else setTimeout(prepare, 0);
                };
                prepare();
            } else self.postMessage({ type: 'ready', job: epoch });
        } else if (data.job === epoch) {
            if (data.type === 'pump') pump(epoch);
            if (data.type === 'work') self.postMessage({ type: 'report', job: epoch, report: session.work(data.work) });
            if (data.type === 'report') { session.accept(data.worker, data.report); pump(epoch); }
        }
    } catch (error) {
        self.postMessage({ type: 'error', job: data.job, error: String(error) });
    }
};
