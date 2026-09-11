const liveEl = document.getElementById("live");
const totalsEl = document.getElementById("totals");
const queueCountEl = document.getElementById("queue-count");
const queueListEl = document.getElementById("queue-list");
const queueNameEl = document.getElementById("queue-name");
const stageTitle = document.getElementById("stage-title");
const mDepth = document.getElementById("m-depth");
const mRate = document.getElementById("m-rate");
const fillBar = document.getElementById("fill-bar");
const laneEl = document.getElementById("lane");

const SOFT_CAP = 32;
const PEEK_N = 40;

let selectedQueue = "demo";
let rpcId = 1;
let lastDepthByQueue = new Map();
let lastError = "";

function escapeHtml(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

async function rpc(method, params) {
  const request = { jsonrpc: "2.0", method, id: rpcId++ };
  if (params !== undefined) request.params = params;
  const res = await fetch("/rpc", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(request),
  });
  const response = JSON.parse(await res.text());
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  if (response.error) {
    throw new Error(`${response.error.code}: ${response.error.message}`);
  }
  return response.result;
}

function setLive(ok, detail = "") {
  liveEl.textContent = ok ? "live" : detail || "offline";
  liveEl.className = `live ${ok ? "on" : "err"}`;
}

function fillPct(depth, ceiling) {
  if (ceiling <= 0) return 0;
  return Math.min(100, Math.round((depth / ceiling) * 100));
}

function eventSummary(event) {
  if (!event || typeof event !== "object") return { type: "event", keys: "" };
  const type = event.type ?? event.kind ?? event.name ?? "event";
  const keys = Object.keys(event)
    .filter((k) => k !== "type" && k !== "kind" && k !== "name")
    .slice(0, 3)
    .map((k) => `${k}=${formatVal(event[k])}`)
    .join(" · ");
  return { type: String(type), keys };
}

function formatVal(v) {
  if (v === null || v === undefined) return "null";
  if (typeof v === "object") return Array.isArray(v) ? `[${v.length}]` : "{…}";
  const s = String(v);
  return s.length > 12 ? `${s.slice(0, 10)}…` : s;
}

function shortId(id) {
  if (!id || id.length < 12) return id || "";
  return `…${id.slice(-8)}`;
}

function selectQueue(name) {
  selectedQueue = name;
  queueNameEl.value = name;
  for (const btn of queueListEl.querySelectorAll(".q-item")) {
    btn.classList.toggle("active", btn.dataset.queue === name);
  }
}

function renderQueueList(queues) {
  const maxDepth = Math.max(SOFT_CAP, ...queues.map((q) => q.depth), 1);
  queueCountEl.textContent = String(queues.length);

  if (!queues.length) {
    queueListEl.innerHTML =
      '<p class="empty">No queues yet — post an event to create one.</p>';
    return;
  }

  queueListEl.innerHTML = "";
  for (const q of queues) {
    const pct = fillPct(q.depth, maxDepth);
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `q-item${q.name === selectedQueue ? " active" : ""}`;
    btn.dataset.queue = q.name;
    btn.innerHTML = `
      <div class="q-row">
        <span class="q-name">${escapeHtml(q.name)}</span>
        <span class="q-depth">${q.depth}</span>
      </div>
      <div class="q-track"><div class="q-fill" style="width:${pct}%"></div></div>
    `;
    btn.addEventListener("click", () => {
      selectQueue(q.name);
      refresh(true);
    });
    queueListEl.appendChild(btn);
  }
}

function renderLane(events, depth) {
  stageTitle.textContent = selectedQueue || "Select a queue";
  const ceiling = Math.max(SOFT_CAP, depth);
  const pct = fillPct(depth, ceiling);
  mDepth.textContent = String(depth);
  fillBar.style.width = `${pct}%`;

  if (!events.length) {
    laneEl.innerHTML = '<p class="empty">Queue is empty.</p>';
    return;
  }

  laneEl.innerHTML = "";
  events.forEach((item, i) => {
    const { type, keys } = eventSummary(item.event);
    const card = document.createElement("article");
    card.className = `msg${i === 0 ? " head" : ""}`;
    card.innerHTML = `
      <div class="msg-pos">#${i + 1}${i === 0 ? " · head" : ""}</div>
      <div class="msg-type" title="${escapeHtml(type)}">${escapeHtml(type)}</div>
      <div class="msg-id" title="${escapeHtml(item.id)}">${escapeHtml(shortId(item.id))}</div>
      <div class="msg-keys" title="${escapeHtml(keys)}">${escapeHtml(keys || "—")}</div>
    `;
    laneEl.appendChild(card);
  });
}

function updateRate(depth) {
  const prev = lastDepthByQueue.get(selectedQueue);
  const delta = prev === undefined ? 0 : depth - prev;
  mRate.textContent = delta === 0 ? "0" : delta > 0 ? `+${delta}` : String(delta);
  lastDepthByQueue.set(selectedQueue, depth);
}

async function refresh(forcePeek = false) {
  try {
    await rpc("ping");
    const listed = await rpc("list_queues");
    const queues = listed.queues || [];
    const totalDepth = queues.reduce((s, q) => s + q.depth, 0);
    totalsEl.textContent = `${queues.length} queues · ${totalDepth} msgs`;
    renderQueueList(queues);
    setLive(true);

    const name = (queueNameEl.value.trim() || selectedQueue).trim();
    if (!name) {
      laneEl.innerHTML = '<p class="empty">Pick a queue on the left to see ordering.</p>';
      return;
    }
    if (name !== selectedQueue || forcePeek) selectQueue(name);

    const qInfo = queues.find((q) => q.name === selectedQueue);
    const depth = qInfo ? qInfo.depth : 0;
    const peeked = await rpc("peek_events", {
      queue: selectedQueue,
      count: PEEK_N,
    });
    renderLane(peeked.events || [], depth);
    updateRate(depth);
    lastError = "";
  } catch (err) {
    lastError = err.message;
    setLive(false, "offline");
    totalsEl.textContent = lastError;
  }
}

async function enqueueSimple() {
  const queue = queueNameEl.value.trim() || "demo";
  const type = document.getElementById("post-type").value.trim() || "event";
  const n = Number(document.getElementById("post-n").value) || 0;
  selectQueue(queue);
  await rpc("post_event", { queue, event: { type, n } });
  document.getElementById("post-n").value = String(n + 1);
  await refresh(true);
}

async function enqueueJson() {
  const queue = queueNameEl.value.trim() || "demo";
  let event;
  try {
    event = JSON.parse(document.getElementById("post-event").value);
  } catch (err) {
    setLive(false, "bad JSON");
    totalsEl.textContent = err.message;
    return;
  }
  if (!event || typeof event !== "object" || Array.isArray(event)) {
    setLive(false, "bad event");
    return;
  }
  selectQueue(queue);
  await rpc("post_event", { queue, event });
  await refresh(true);
}

async function dequeue() {
  const queue = queueNameEl.value.trim() || selectedQueue;
  if (!queue) return;
  const count = Number(document.getElementById("get-count").value) || 1;
  selectQueue(queue);
  await rpc("get_events", { queue, count });
  await refresh(true);
}

document.getElementById("post-btn").addEventListener("click", () => {
  enqueueSimple().catch((e) => {
    setLive(false, "error");
    totalsEl.textContent = e.message;
  });
});
document.getElementById("post-json-btn").addEventListener("click", () => {
  enqueueJson().catch((e) => {
    setLive(false, "error");
    totalsEl.textContent = e.message;
  });
});
document.getElementById("get-btn").addEventListener("click", () => {
  dequeue().catch((e) => {
    setLive(false, "error");
    totalsEl.textContent = e.message;
  });
});
document.getElementById("refresh-btn").addEventListener("click", () => refresh(true));
queueNameEl.addEventListener("change", () => {
  selectQueue(queueNameEl.value.trim() || "demo");
  refresh(true);
});

refresh(true);
setInterval(() => refresh(false), 1500);
