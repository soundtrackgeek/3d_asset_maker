const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

const state = { jobs: new Map(), selected: null, opts: null, presets: [], engines: [], thumbs: new Map(), update: null };
const UPDATE_INTERVAL_MS = 30 * 60 * 1000;

// ---------- options form ----------
const SAMPLER_FIELDS = [["steps", 1, 1], ["guidance_strength", 0, 0.1], ["guidance_rescale", 0, 0.05], ["rescale_t", 0, 0.1]];
const SAMPLER_GROUPS = [["sparse_structure", "Sparse structure"], ["shape", "Shape"], ["texture", "Texture"]];

function buildSamplers() {
  $("#samplers").innerHTML = SAMPLER_GROUPS.map(([g, label]) =>
    `<h3>${label}</h3>` + SAMPLER_FIELDS.map(([f, min, step]) =>
      `<label>${f.replace(/_/g, " ")} <input type="number" data-g="${g}" data-f="${f}" min="${min}" step="${step}" placeholder="model default" /></label>`).join("")).join("");
  document.querySelectorAll("#samplers input").forEach((i) => i.addEventListener("input", () => {
    state.opts[i.dataset.g][i.dataset.f] = i.value === "" ? null : Number(i.value);
  }));
}

function fillForm() {
  const o = state.opts;
  $("#o-preset").value = o.preset;
  $("#o-engine").value = o.engine;
  $("#o-resolution").value = o.resolution;
  $("#o-seed").value = o.seed;
  $("#o-random").checked = o.randomize_seed;
  $("#o-decimation").value = o.decimation_target;
  $("#o-texsize").value = String(o.texture_size);
  $("#o-remesh").checked = o.remesh;
  $("#o-webp").checked = o.webp_textures;
  $("#o-lowvram").checked = o.low_vram;
  $("#o-bg").checked = o.preprocess_image;
  $("#o-tokens").value = o.max_num_tokens;
  $("#o-attn").value = o.attn_backend;
  document.querySelectorAll(".o-fmt").forEach((c) => (c.checked = o.formats.includes(c.value)));
  document.querySelectorAll("#samplers input").forEach((i) => (i.value = o[i.dataset.g][i.dataset.f] ?? ""));
  const p = state.presets.find((p) => p.name === o.preset);
  $("#preset-desc").textContent = p ? p.description : "";
  const e = state.engines.find((e) => e.name === o.engine);
  $("#engine-desc").textContent = e ? e.description : "";
}

function bindForm() {
  const bind = (sel, key, conv = (v) => v, ev = "input") =>
    $(sel).addEventListener(ev, (e) => {
      state.opts[key] = conv(e.target.type === "checkbox" ? e.target.checked : e.target.value);
    });
  bind("#o-resolution", "resolution");
  bind("#o-seed", "seed", Number);
  bind("#o-random", "randomize_seed");
  bind("#o-decimation", "decimation_target", Number);
  bind("#o-texsize", "texture_size", Number);
  bind("#o-remesh", "remesh");
  bind("#o-webp", "webp_textures");
  bind("#o-lowvram", "low_vram");
  bind("#o-bg", "preprocess_image");
  bind("#o-tokens", "max_num_tokens", Number);
  bind("#o-attn", "attn_backend");
  $("#o-preset").addEventListener("change", (e) => {
    const p = state.presets.find((p) => p.name === e.target.value);
    const engine = state.opts.engine;
    state.opts = structuredClone(p.options);
    state.opts.engine = engine;
    fillForm();
  });
  $("#o-engine").addEventListener("change", (e) => {
    state.opts.engine = e.target.value;
    // Engine budgets only ever lower the values; shown immediately so it is not a surprise.
    const caps = { threejs: [150000, 2048], unity: [100000, 2048], godot: [100000, 2048], unreal: [200000, 4096] }[e.target.value];
    if (caps) {
      state.opts.decimation_target = Math.min(state.opts.decimation_target, caps[0]);
      state.opts.texture_size = Math.min(state.opts.texture_size, caps[1]);
    }
    fillForm();
  });
  document.querySelectorAll(".o-fmt").forEach((c) => c.addEventListener("change", () => {
    state.opts.formats = [...document.querySelectorAll(".o-fmt")].filter((x) => x.checked).map((x) => x.value);
  }));
}

const useMock = () => $("#o-mock").checked;

// ---------- jobs ----------
async function thumb(job) {
  if (!state.thumbs.has(job.id)) {
    state.thumbs.set(job.id, null);
    try {
      state.thumbs.set(job.id, await invoke("read_asset", { id: job.id, kind: "input", file: null }));
      renderJobs();
    } catch {}
  }
  return state.thumbs.get(job.id);
}

function renderJobs() {
  const list = [...state.jobs.values()].sort((a, b) => b.created - a.created);
  list.forEach((j) => { if (!state.thumbs.has(j.id)) thumb(j); });
  $("#jobs").innerHTML = list.map((j) => {
    const t = state.thumbs.get(j.id);
    return `<div class="job ${j.id === state.selected ? "sel" : ""}" data-id="${esc(j.id)}">
      ${t ? `<img src="${t}" alt="">` : `<img alt="">`}
      <div class="meta"><div class="name">${esc(j.name)}</div>
      <div class="sub"><span class="st-${j.status}">${j.status}</span> · ${esc(j.stage)}</div>
      ${j.status === "running" ? `<div class="bar"><i style="width:${Math.round(j.progress * 100)}%"></i></div>` : ""}</div></div>`;
  }).join("");
  document.querySelectorAll(".job").forEach((el) => el.addEventListener("click", () => select(el.dataset.id)));
}

async function select(id) {
  state.selected = id;
  renderJobs();
  await renderStage();
}

async function renderStage() {
  const j = state.jobs.get(state.selected);
  const viewer = $("#viewer"), img = $("#input-preview");
  if (!j) {
    viewer.hidden = img.hidden = true;
    $("#stage-empty").hidden = false;
    $("#job-detail").innerHTML = "";
    return;
  }
  $("#stage-empty").hidden = true;
  const glb = j.outputs.find((f) => f.endsWith(".glb"));
  if (j.status === "done" && glb) {
    img.hidden = true;
    viewer.hidden = false;
    if (viewer.dataset.key !== j.id + j.elapsed_secs) {
      viewer.dataset.key = j.id + j.elapsed_secs;
      viewer.src = await invoke("read_asset", { id: j.id, kind: "output", file: glb });
    }
  } else {
    viewer.hidden = true;
    img.hidden = false;
    img.src = (await thumb(j)) || "";
  }
  $("#job-detail").innerHTML = `
    <b>${esc(j.name)}</b> <span class="st-${j.status}">${j.status}</span> — ${esc(j.stage)}
    · ${j.options.resolution} · seed ${j.options.seed} · ${j.options.texture_size}px · ${j.options.decimation_target.toLocaleString()} tris
    ${j.mock ? " · <i>mock</i>" : ""} ${j.elapsed_secs ? ` · ${j.elapsed_secs.toFixed(0)}s` : ""}
    ${j.error ? `<pre>${esc(j.error)}</pre>` : ""}
    ${j.outputs.length ? `<div class="hint">Files: ${j.outputs.map(esc).join(", ")}, asset.json</div>` : ""}
    <div class="actions">
      <button id="d-run" ${j.status === "running" ? "disabled" : ""}>${j.status === "done" ? "Regenerate with current options" : "Generate"}</button>
      <button id="d-cancel" ${j.status === "running" || j.status === "queued" ? "" : "disabled"}>Cancel</button>
      <button id="d-open" ${j.outputs.length ? "" : "disabled"}>Open output folder</button>
      <button id="d-del">Delete</button>
    </div>`;
  $("#d-run").onclick = () => runJob(j.id);
  $("#d-cancel").onclick = () => invoke("cancel_job", { id: j.id });
  $("#d-open").onclick = () => invoke("reveal_job", { id: j.id });
  $("#d-del").onclick = async () => {
    await invoke("delete_job", { id: j.id });
    state.jobs.delete(j.id);
    state.selected = null;
    renderJobs();
    renderStage();
  };
}

async function runJob(id) {
  const opts = structuredClone(state.opts);
  const j = state.jobs.get(id);
  if (j) j.mock = useMock();
  try {
    await invoke("start_job", { id, opts });
  } catch (e) {
    alert(e);
  }
}

function addJobs(jobs) {
  jobs.forEach((j) => {
    state.jobs.set(j.id, j);
    thumb(j);
  });
  if (jobs.length) state.selected = jobs[0].id;
  renderJobs();
  renderStage();
}

async function ingestPaths(paths) {
  try {
    addJobs(await invoke("create_jobs_from_paths", { pathsIn: paths, opts: state.opts, mock: useMock() }));
  } catch (e) { alert(e); }
}

async function ingestFiles(files) {
  const uploads = [];
  for (const f of files) {
    const buf = new Uint8Array(await f.arrayBuffer());
    let bin = "";
    for (let i = 0; i < buf.length; i += 0x8000) bin += String.fromCharCode(...buf.subarray(i, i + 0x8000));
    uploads.push({ name: f.name, data_b64: btoa(bin) });
  }
  try {
    addJobs(await invoke("create_jobs_from_bytes", { files: uploads, opts: state.opts, mock: useMock() }));
  } catch (e) { alert(e); }
}

// ---------- updates ----------
async function checkUpdate(manual) {
  const btn = $("#btn-update");
  if (manual) { btn.disabled = true; btn.textContent = "Checking…"; }
  try {
    const u = await invoke("check_update");
    state.update = u;
    const b = $("#update-banner");
    if (u.available) {
      b.hidden = false;
      b.innerHTML = `Update ${esc(u.latest)} available <a id="upd-dl">${u.download_url ? "Download &amp; install" : "Download"}</a> <a id="upd-notes">Release notes</a>`;
      $("#upd-dl").onclick = async () => {
        if (!u.download_url) return invoke("open_url", { url: u.release_url });
        b.textContent = "Downloading installer…";
        try { await invoke("install_update", { url: u.download_url }); } catch (e) { b.textContent = "Update failed: " + e; }
      };
      $("#upd-notes").onclick = () => invoke("open_url", { url: u.release_url });
    } else {
      b.hidden = !manual;
      b.textContent = `You're up to date (v${u.current})`;
      if (manual) setTimeout(() => (b.hidden = true), 4000);
    }
  } catch (e) {
    if (manual) { const b = $("#update-banner"); b.hidden = false; b.textContent = "Update check failed: " + e; setTimeout(() => (b.hidden = true), 6000); }
  } finally {
    if (manual) { btn.disabled = false; btn.textContent = "Check for updates"; }
  }
}

// ---------- diagnostics ----------
function pill(sel, ok, text) {
  const el = $(sel);
  el.textContent = text;
  el.className = "pill " + (ok ? "ok" : "bad");
}

async function refreshDoctor(showDialog) {
  const r = await invoke("run_doctor");
  pill("#pill-gpu", !!r.gpu, r.gpu ? `${r.gpu} · ${(r.vram_mb / 1024).toFixed(0)} GB` : "no NVIDIA GPU");
  pill("#pill-rt", r.ready_for_real_generation, r.ready_for_real_generation ? "TRELLIS.2 ready" : "runtime not installed (mock only)");
  const info = await invoke("app_info");
  pill("#pill-hf", info.hf_token, info.hf_token ? "HF token ok" : "no HF token");
  $("#doctor-body").innerHTML = r.checks.map((c) =>
    `<div class="chk"><b>${esc(c.name)}</b><div class="${c.ok ? "ok" : "no"}">${c.ok ? "OK" : "!"} ${esc(c.detail)}${!c.ok && c.hint ? `<small>${esc(c.hint)}</small>` : ""}</div></div>`).join("") +
    `<p class="hint">Recommended preset: <b>${esc(r.recommended_preset)}</b> · data folder: ${esc(r.home)}</p>`;
  if (showDialog) $("#doctor-dialog").showModal();
}

// ---------- init ----------
async function init() {
  const info = await invoke("app_info");
  $("#ver").textContent = "v" + info.version;
  const o = await invoke("get_options");
  state.presets = o.presets;
  state.engines = o.engines;
  $("#o-preset").innerHTML = o.presets.map((p) => `<option>${esc(p.name)}</option>`).join("");
  $("#o-engine").innerHTML = o.engines.map((e) => `<option>${esc(e.name)}</option>`).join("");
  state.opts = structuredClone(o.presets.find((p) => p.name === "balanced").options);
  buildSamplers();
  bindForm();
  fillForm();

  (await invoke("list_jobs")).forEach((j) => { state.jobs.set(j.id, j); thumb(j); });
  renderJobs();

  await listen("job-updated", (e) => {
    const j = e.payload;
    state.jobs.set(j.id, j);
    renderJobs();
    if (j.id === state.selected) renderStage();
  });
  await listen("setup-line", (e) => {
    const log = $("#setup-log");
    log.hidden = false;
    log.textContent += e.payload + "\n";
    log.scrollTop = log.scrollHeight;
  });
  await listen("tauri://drag-drop", (e) => { $("#dropzone").classList.remove("over"); ingestPaths(e.payload.paths); });
  await listen("tauri://drag-enter", () => $("#dropzone").classList.add("over"));
  await listen("tauri://drag-leave", () => $("#dropzone").classList.remove("over"));

  $("#file-input").addEventListener("change", (e) => { ingestFiles([...e.target.files]); e.target.value = ""; });
  $("#btn-generate").onclick = () => state.selected && runJob(state.selected);
  $("#btn-generate-all").onclick = () => [...state.jobs.values()].filter((j) => j.status === "queued").forEach((j) => runJob(j.id));
  $("#btn-update").onclick = () => checkUpdate(true);
  $("#btn-doctor").onclick = () => refreshDoctor(true);
  $("#btn-doctor-refresh").onclick = () => refreshDoctor(false);
  $("#btn-doctor-close").onclick = () => $("#doctor-dialog").close();
  $("#btn-install").onclick = async () => {
    const b = $("#btn-install");
    b.disabled = true;
    $("#setup-log").textContent = "";
    try { await invoke("install_runtime"); } catch (e) { $("#setup-log").textContent += "\nFAILED: " + e; }
    b.disabled = false;
    refreshDoctor(false);
  };

  refreshDoctor(false);
  // Update policy: on startup, then every 30 minutes while open, plus the manual button.
  checkUpdate(false);
  setInterval(() => checkUpdate(false), UPDATE_INTERVAL_MS);
}

init().catch((e) => { document.body.prepend(Object.assign(document.createElement("pre"), { textContent: "Init failed: " + e })); });
