/*! Kempt calendar: one dependency-free file that draws a calendar (Day, Week, Month, Year, Schedule).
 *  It knows nothing about storage or the network: the host gives it events and tasks and receives changes.
 *  Kempt embeds it for offline use; the online app can load the same file.
 *
 *    const cal = KemptCalendar.mount(element, {
 *      events: [...],                       // see "event" below (tombstones, x:true, are kept but never drawn)
 *      tasks: () => [{ id, t, due, done, c }],   // optional: checklist items / reminders shown on their day
 *      view: 'month', ws: 1, h12: false, locale: undefined,
 *      onChange(ev, all) {},                // an event was created, edited or deleted (ev.x === true)
 *      onView(view) {}, onTask(task, 'toggle'|'open') {},
 *    });
 *    cal.setEvents(list); cal.refresh(); cal.setView('week'); cal.goto(Date.now()); cal.destroy();
 *
 *  event = { id, t: title, s: start ms, e: end ms, ad: all-day, r: { f: 'd'|'w'|'m'|'y', n: every, by: [0-6], until, count },
 *            ex: ['YYYY-MM-DD'], rem: [minutes before], c: 'w'|'p', d: notes, l: place, u: changed ms, x: deleted }
 *  An all-day event starts at local midnight and ends (exclusive) at local midnight after its last day.
 */
(function (root) {
  'use strict';
  const DAY = 86400000, HOUR = 3600000;
  const VIEWS = [['day', 'Day'], ['week', 'Week'], ['month', 'Month'], ['year', 'Year'], ['schedule', 'Schedule']];
  const REMS = [[0, 'At the time'], [10, '10 minutes before'], [60, '1 hour before'], [1440, '1 day before']];

  const CSS = `
.kc { --kc-bg: var(--bg, #fff); --kc-bg2: var(--bg2, #f6f6f6); --kc-bg3: var(--bg3, #ebebeb); --kc-fg: var(--fg, #1f2328); --kc-dim: var(--fg-dim, #656d76);
  --kc-line: var(--border, #d0d7de); --kc-accent: var(--accent, #0969da); --kc-work: #4c8dff; --kc-pers: #2fb676; --kc-danger: var(--danger, #cf222e);
  display: flex; flex-direction: column; height: 100%; min-height: 0; color: var(--kc-fg); background: var(--kc-bg); font: 13px/1.4 var(--ui, system-ui, sans-serif); outline: none; position: relative; }
.kc *, .kc *::before, .kc *::after { box-sizing: border-box; }
.kc button, .kc input, .kc select, .kc textarea { font: inherit; color: inherit; }
.kc-bar { display: flex; align-items: center; gap: 10px; padding: 10px 16px; border-bottom: 1px solid var(--kc-line); flex-wrap: wrap; }
.kc-title { font-size: 18px; font-weight: 600; letter-spacing: -.01em; min-width: 11ch; }
.kc-sp { flex: 1; }
.kc-btn { border: 1px solid var(--kc-line); background: var(--kc-bg2); border-radius: 8px; padding: 5px 12px; cursor: pointer; line-height: 1.3; }
.kc-btn:hover { background: var(--kc-bg3); } .kc-btn:focus-visible, .kc-cell:focus-visible, .kc-chip:focus-visible, .kc-seg button:focus-visible { outline: 2px solid var(--kc-accent); outline-offset: 1px; }
.kc-btn.pri { background: var(--kc-accent); border-color: var(--kc-accent); color: var(--accent-fg, #fff); font-weight: 600; } .kc-btn.pri:hover { filter: brightness(1.1); background: var(--kc-accent); }
.kc-btn.danger { color: var(--kc-danger); }
.kc-nav { display: inline-flex; gap: 4px; } .kc-nav .kc-btn { padding: 5px 10px; }
.kc-seg { display: inline-flex; border: 1px solid var(--kc-line); border-radius: 8px; overflow: hidden; }
.kc-seg button { border: 0; background: transparent; padding: 5px 12px; cursor: pointer; border-right: 1px solid var(--kc-line); } .kc-seg button:last-child { border-right: 0; }
.kc-seg button:hover { background: var(--kc-bg3); } .kc-seg button[aria-pressed=true] { background: var(--kc-accent); color: var(--accent-fg, #fff); font-weight: 600; }
.kc-body { flex: 1; min-height: 0; overflow: auto; position: relative; }
/* month */
.kc-month { display: grid; grid-template-rows: auto 1fr; height: 100%; min-height: 420px; }
.kc-dow { display: grid; grid-template-columns: repeat(7, 1fr); border-bottom: 1px solid var(--kc-line); }
.kc-dow div { padding: 6px 10px; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--kc-dim); }
.kc-weeks { display: grid; grid-auto-rows: 1fr; min-height: 0; }
.kc-week { display: grid; grid-template-columns: repeat(7, 1fr); min-height: 0; }
.kc-cell { border-right: 1px solid var(--kc-line); border-bottom: 1px solid var(--kc-line); padding: 4px 5px; min-width: 0; min-height: 0; overflow: hidden; cursor: pointer; background: var(--kc-bg); }
.kc-cell:last-child { border-right: 0; } .kc-cell.out { background: var(--kc-bg2); } .kc-cell.out .kc-num { color: var(--kc-dim); } .kc-cell.we:not(.out) { background: color-mix(in srgb, var(--kc-bg2) 55%, var(--kc-bg)); }
.kc-cell:hover { background: color-mix(in srgb, var(--kc-accent) 7%, var(--kc-bg)); }
.kc-num { display: inline-flex; align-items: center; justify-content: center; min-width: 24px; height: 24px; padding: 0 4px; border-radius: 12px; font-size: 12px; font-weight: 500; margin-bottom: 2px; }
.kc-cell.today .kc-num, .kc-dh.today .kc-num { background: var(--kc-accent); color: var(--accent-fg, #fff); font-weight: 700; }
.kc-chip { --c: var(--kc-accent); display: flex; align-items: center; justify-content: flex-start; gap: 5px; height: 20px; flex: none; width: 100%; margin: 0 0 2px; padding: 0 6px; border: 0; border-left: 3px solid var(--c); border-radius: 4px; text-align: left; cursor: pointer;
  background: color-mix(in srgb, var(--c) 16%, transparent); overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: 12px; line-height: 1.5; }
.kc-chip:hover { background: color-mix(in srgb, var(--c) 28%, transparent); } .kc-chip.w { --c: var(--kc-work); } .kc-chip.p { --c: var(--kc-pers); }
.kc-chip.ad { background: var(--c); color: #fff; border-left-color: transparent; } .kc-chip.ad:hover { filter: brightness(1.08); background: var(--c); }
.kc-chip.task { border-left-style: dashed; } .kc-chip.done { opacity: .55; text-decoration: line-through; }
.kc-chip .tm { color: var(--kc-dim); font-variant-numeric: tabular-nums; flex: none; } .kc-chip .tt { overflow: hidden; text-overflow: ellipsis; }
.kc-more { border: 0; background: transparent; color: var(--kc-dim); font-size: 11px; padding: 0 6px; cursor: pointer; } .kc-more:hover { color: var(--kc-accent); text-decoration: underline; }
/* day and week */
.kc-tg { display: grid; grid-template-columns: 54px 1fr; min-width: 560px; }
.kc-tg-head { position: sticky; top: 0; z-index: 4; background: var(--kc-bg); border-bottom: 1px solid var(--kc-line); }
.kc-dhs { display: grid; } .kc-dh { padding: 6px 8px 4px; text-align: center; border-left: 1px solid var(--kc-line); }
.kc-dh small { display: block; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; color: var(--kc-dim); } .kc-dh .kc-num { font-size: 16px; min-width: 30px; height: 30px; border-radius: 15px; }
.kc-allday { display: grid; border-top: 1px solid var(--kc-line); } .kc-allday > div { border-left: 1px solid var(--kc-line); padding: 3px 3px 1px; min-height: 26px; }
.kc-gut { position: relative; font-size: 10.5px; color: var(--kc-dim); text-align: right; padding-right: 8px; } .kc-gut span { position: absolute; right: 8px; transform: translateY(-50%); background: var(--kc-bg); padding: 0 2px; }
.kc-cols { display: grid; position: relative; } .kc-col { position: relative; border-left: 1px solid var(--kc-line); cursor: pointer; }
.kc-col.today { background: color-mix(in srgb, var(--kc-accent) 5%, transparent); }
.kc-hl { position: absolute; left: 0; right: 0; border-top: 1px solid var(--kc-line); pointer-events: none; } .kc-hl.half { border-top-style: dotted; opacity: .5; }
.kc-ev { --c: var(--kc-accent); display: block; position: absolute; padding: 2px 6px; border: 0; border-left: 3px solid var(--c); border-radius: 6px; text-align: left; cursor: pointer; overflow: hidden; font-size: 12px; line-height: 1.3;
  background: color-mix(in srgb, var(--c) 22%, var(--kc-bg)); box-shadow: 0 1px 2px rgba(0,0,0,.12); } .kc-ev:hover { background: color-mix(in srgb, var(--c) 34%, var(--kc-bg)); z-index: 3; }
.kc-ev.w { --c: var(--kc-work); } .kc-ev.p { --c: var(--kc-pers); } .kc-ev b { display: block; font-weight: 600; } .kc-ev .tm { color: var(--kc-dim); font-size: 11px; }
.kc-ev.task { border-left-style: dashed; background: color-mix(in srgb, var(--c) 12%, var(--kc-bg)); } .kc-ev.done { opacity: .55; text-decoration: line-through; }
.kc-now { position: absolute; left: 0; right: 0; height: 2px; background: #e5484d; z-index: 2; pointer-events: none; } .kc-now::before { content: ''; position: absolute; left: -4px; top: -3px; width: 8px; height: 8px; border-radius: 4px; background: #e5484d; }
/* year */
.kc-year { display: grid; grid-template-columns: repeat(auto-fill, minmax(230px, 1fr)); gap: 20px; padding: 18px 22px; }
.kc-mini h4 { margin: 0 0 6px; font-size: 13px; font-weight: 600; } .kc-mini h4 button { border: 0; background: transparent; padding: 0; cursor: pointer; font-weight: 600; } .kc-mini h4 button:hover { color: var(--kc-accent); }
.kc-mg { display: grid; grid-template-columns: repeat(7, 1fr); gap: 1px; text-align: center; font-size: 11px; }
.kc-mg i { font-style: normal; color: var(--kc-dim); padding: 2px 0; } .kc-md { position: relative; border: 0; background: transparent; padding: 4px 0; border-radius: 12px; cursor: pointer; font-size: 11.5px; } .kc-md:hover { background: var(--kc-bg3); }
.kc-md.today { background: var(--kc-accent); color: var(--accent-fg, #fff); font-weight: 700; } .kc-md.out { visibility: hidden; } .kc-md.has::after { content: ''; position: absolute; left: 50%; bottom: 1px; width: 4px; height: 4px; margin-left: -2px; border-radius: 2px; background: var(--kc-accent); }
.kc-md.today.has::after { background: var(--accent-fg, #fff); }
/* schedule */
.kc-sched { max-width: 820px; margin: 0 auto; padding: 8px 20px 40px; }
.kc-sd { display: grid; grid-template-columns: 92px 1fr; gap: 12px; padding: 12px 0; border-bottom: 1px solid var(--kc-line); }
.kc-sdh { text-align: left; } .kc-sdh b { display: block; font-size: 22px; line-height: 1.1; font-weight: 600; } .kc-sdh span { color: var(--kc-dim); font-size: 11.5px; text-transform: uppercase; letter-spacing: .05em; }
.kc-sd.today .kc-sdh b { color: var(--kc-accent); } .kc-sd .kc-chip { height: auto; padding: 5px 10px; font-size: 13px; margin-bottom: 4px; }
.kc-empty { text-align: center; color: var(--kc-dim); padding: 60px 20px; }
/* editor */
.kc-shade { position: absolute; inset: 0; background: rgba(0,0,0,.42); display: flex; align-items: center; justify-content: center; z-index: 20; padding: 16px; }
.kc-dlg { width: min(520px, 100%); max-height: 100%; overflow: auto; background: var(--kc-bg); border: 1px solid var(--kc-line); border-radius: 14px; box-shadow: 0 18px 60px rgba(0,0,0,.45); padding: 18px 20px; }
.kc-dlg h3 { margin: 0 0 12px; font-size: 16px; } .kc-f { display: grid; grid-template-columns: 84px 1fr; gap: 8px 10px; align-items: center; margin-bottom: 8px; } .kc-f > label { color: var(--kc-dim); font-size: 12px; }
.kc-f input[type=text], .kc-f input[type=date], .kc-f input[type=time], .kc-f input[type=number], .kc-f select, .kc-f textarea { width: 100%; padding: 6px 8px; border: 1px solid var(--kc-line); border-radius: 8px; background: var(--kc-bg2); }
.kc-f textarea { min-height: 60px; resize: vertical; } .kc-f input:focus, .kc-f select:focus, .kc-f textarea:focus { outline: 2px solid var(--kc-accent); outline-offset: 0; }
.kc-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; } .kc-row input[type=number] { width: 70px !important; } .kc-row label { display: inline-flex; gap: 5px; align-items: center; color: var(--kc-fg); font-size: 12.5px; }
.kc-dows { display: flex; gap: 4px; } .kc-dows button { width: 30px; height: 30px; border-radius: 15px; border: 1px solid var(--kc-line); background: var(--kc-bg2); cursor: pointer; font-size: 11.5px; } .kc-dows button[aria-pressed=true] { background: var(--kc-accent); color: var(--accent-fg, #fff); border-color: var(--kc-accent); }
.kc-foot { display: flex; gap: 8px; justify-content: flex-end; margin-top: 14px; } .kc-foot .kc-sp { flex: 1; } .kc-err { color: var(--kc-danger); font-size: 12px; min-height: 16px; margin-top: 4px; }
@media (prefers-reduced-motion: no-preference) { .kc-body > * { animation: kcfade .14s ease-out; } @keyframes kcfade { from { opacity: .0; } to { opacity: 1; } } }
@media (max-width: 640px) { .kc-bar { padding: 8px 10px; } .kc-title { font-size: 15px; } .kc-f { grid-template-columns: 1fr; } }
`;

  // ───────────── dates ─────────────
  const sod = ms => { const d = new Date(ms); d.setHours(0, 0, 0, 0); return d.getTime(); };
  const addDays = (ms, n) => { const d = new Date(ms); d.setDate(d.getDate() + n); return d.getTime(); };
  const dkey = ms => { const d = new Date(ms); return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0'); };
  const fromKey = k => { const [y, m, d] = k.split('-').map(Number); return new Date(y, m - 1, d).getTime(); };
  const sameDay = (a, b) => dkey(a) === dkey(b);
  const uid = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const weekStart = (ms, ws) => { const d = new Date(sod(ms)); d.setDate(d.getDate() - ((d.getDay() - ws + 7) % 7)); return d.getTime(); };
  const monthDays = (y, m) => new Date(y, m + 1, 0).getDate();
  const at = (dayMs, h, mi) => { const d = new Date(dayMs); d.setHours(h, mi, 0, 0); return d.getTime(); };

  /** every occurrence of an event that touches [from, to): [{ ev, s, e, key }] (local wall-clock time is kept across daylight saving) */
  function occurrences(ev, from, to) {
    if (!ev || ev.x || typeof ev.s !== 'number') return [];
    const dur = Math.max(0, (typeof ev.e === 'number' ? ev.e : ev.s + (ev.ad ? DAY : HOUR)) - ev.s);
    const ex = new Set(ev.ex || []);
    const out = [];
    const push = s => {
      const key = dkey(s); if (ex.has(key)) return;
      let e = s + dur;
      if (ev.ad) { const days = Math.round(dur / DAY) || 1; e = addDays(s, days); }
      if (s < to && (e > from || (e === s && s >= from))) out.push({ ev, s, e, key });
    };
    const r = ev.r;
    if (!r) { push(ev.s); return out; }
    const st = new Date(ev.s), n = Math.max(1, r.n | 0 || 1), until = typeof r.until === 'number' ? r.until + (ev.ad ? DAY - 1 : 0) : Infinity, cap = r.count ? r.count : Infinity;
    let made = 0, guard = 0;
    const at_ = (y, m, d) => new Date(y, m, d, st.getHours(), st.getMinutes(), 0, 0).getTime();
    const take = s => { if (s < ev.s) return true; if (s > until || s >= to + (ev.ad ? 0 : 0) && s > to) return false; if (made >= cap) return false; made++; push(s); return true; };
    if (r.f === 'd') {
      for (let k = 0; guard++ < 20000; k++) { const s = at_(st.getFullYear(), st.getMonth(), st.getDate() + k * n); if (!take(s)) break; }
    } else if (r.f === 'w') {
      const by = (r.by && r.by.length ? r.by : [st.getDay()]).slice().sort((a, b) => a - b), anchor = addDays(sod(ev.s), -st.getDay());
      outer: for (let w = 0; guard++ < 5000; w++) for (const wd of by) { const base = new Date(anchor); const s = at_(base.getFullYear(), base.getMonth(), base.getDate() + w * 7 * n + wd); if (s < ev.s) continue; if (!take(s)) break outer; }
    } else if (r.f === 'm') {
      for (let k = 0; guard++ < 5000; k++) { const t = new Date(st.getFullYear(), st.getMonth() + k * n, 1); const s = at_(t.getFullYear(), t.getMonth(), Math.min(st.getDate(), monthDays(t.getFullYear(), t.getMonth()))); if (!take(s)) break; }
    } else if (r.f === 'y') {
      for (let k = 0; guard++ < 2000; k++) { const y = st.getFullYear() + k * n; const s = at_(y, st.getMonth(), Math.min(st.getDate(), monthDays(y, st.getMonth()))); if (!take(s)) break; }
    }
    return out;
  }

  // ───────────── the calendar ─────────────
  function mount(host, opts) {
    opts = opts || {};
    if (!document.getElementById('kc-style')) { const s = document.createElement('style'); s.id = 'kc-style'; s.textContent = CSS; document.head.appendChild(s); }
    const S = { view: VIEWS.some(v => v[0] === opts.view) ? opts.view : 'month', cur: Date.now(), events: Array.isArray(opts.events) ? opts.events.slice() : [], ws: opts.ws === 0 ? 0 : 1, h12: !!opts.h12, loc: opts.locale, more: 1, dlg: null };
    const nf = (o) => new Intl.DateTimeFormat(S.loc, o);
    const tfmt = () => nf({ hour: 'numeric', minute: '2-digit', hour12: S.h12 });
    const root_ = document.createElement('div'); root_.className = 'kc'; root_.tabIndex = 0; root_.setAttribute('role', 'application'); root_.setAttribute('aria-label', 'Calendar');
    host.appendChild(root_);
    const tasks = () => { try { return (typeof opts.tasks === 'function' ? opts.tasks() : opts.tasks) || []; } catch (_) { return []; } };
    const live = () => S.events.filter(e => !e.x);
    const items = (from, to) => {   // events (expanded) and tasks that fall in [from, to), sorted
      const out = [];
      for (const ev of live()) for (const o of occurrences(ev, from, to)) out.push({ kind: 'ev', ev, s: o.s, e: o.e, ad: !!ev.ad, key: o.key });
      for (const t of tasks()) if (typeof t.due === 'number' && t.due >= from && t.due < to) out.push({ kind: 'task', task: t, ev: { t: t.t, c: t.c }, s: t.due, e: t.due, ad: false });
      return out.sort((a, b) => (b.ad - a.ad) || a.s - b.s || a.e - b.e);
    };
    const cat = o => (o.ev && (o.ev.c === 'w' || o.ev.c === 'p')) ? o.ev.c : '';
    const T = () => tfmt();
    const timeText = o => o.ad ? '' : T().format(o.s);

    function head() {
      const d = new Date(S.cur);
      if (S.view === 'day') return nf({ weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }).format(d);
      if (S.view === 'year') return String(d.getFullYear());
      if (S.view === 'week') { const a = weekStart(S.cur, S.ws), b = addDays(a, 6), same = new Date(a).getMonth() === new Date(b).getMonth(); return nf({ day: 'numeric', month: 'short' }).format(a) + ' – ' + (same ? nf({ day: 'numeric' }).format(b) : nf({ day: 'numeric', month: 'short' }).format(b)) + ', ' + new Date(b).getFullYear(); }
      return nf({ month: 'long', year: 'numeric' }).format(d);
    }
    function move(dir) {
      const d = new Date(S.cur);
      if (S.view === 'day') d.setDate(d.getDate() + dir); else if (S.view === 'week') d.setDate(d.getDate() + 7 * dir); else if (S.view === 'year') d.setFullYear(d.getFullYear() + dir);
      else if (S.view === 'schedule') d.setDate(d.getDate() + 30 * dir); else { d.setDate(1); d.setMonth(d.getMonth() + dir); }
      S.cur = d.getTime(); S.more = 1; draw();
    }
    function setView(v) { if (!VIEWS.some(x => x[0] === v)) return; S.view = v; S.more = 1; if (typeof opts.onView === 'function') try { opts.onView(v); } catch (_) { } draw(); }
    function goto(ms, view) { S.cur = ms; if (view) S.view = view; S.more = 1; draw(); }

    // ---- toolbar
    function drawBar() {
      const bar = document.createElement('div'); bar.className = 'kc-bar';
      bar.innerHTML = `<button class="kc-btn today" type="button">Today</button><span class="kc-nav"><button class="kc-btn prev" type="button" aria-label="Previous">‹</button><button class="kc-btn next" type="button" aria-label="Next">›</button></span><span class="kc-title" role="heading" aria-level="2" aria-live="polite">${esc(head())}</span><span class="kc-sp"></span><span class="kc-seg" role="group" aria-label="View">${VIEWS.map(([v, n]) => `<button type="button" data-v="${v}" aria-pressed="${S.view === v}">${n}</button>`).join('')}</span><button class="kc-btn pri add" type="button">+ New</button>`;
      bar.querySelector('.today').onclick = () => goto(Date.now());
      bar.querySelector('.prev').onclick = () => move(-1); bar.querySelector('.next').onclick = () => move(1);
      bar.querySelectorAll('.kc-seg button').forEach(b => b.onclick = () => setView(b.dataset.v));
      bar.querySelector('.add').onclick = () => edit(null, S.view === 'month' || S.view === 'year' || S.view === 'schedule' ? at(sod(S.cur), 9, 0) : at(sod(S.cur), Math.max(8, new Date().getHours() + 1), 0));
      return bar;
    }

    // ---- chips
    function chip(o, cls) {
      const b = document.createElement('button'); b.type = 'button';
      b.className = 'kc-chip ' + cat(o) + (o.ad && o.kind === 'ev' ? ' ad' : '') + (o.kind === 'task' ? ' task' : '') + (o.kind === 'task' && o.task.done ? ' done' : '') + (cls ? ' ' + cls : '');
      const tt = o.kind === 'task' ? (o.task.done ? '☑ ' : '☐ ') + o.ev.t : o.ev.t;
      b.innerHTML = (timeText(o) ? `<span class="tm">${esc(timeText(o))}</span>` : '') + `<span class="tt">${esc(tt)}</span>`;
      b.title = (o.kind === 'task' ? 'Task: ' : '') + o.ev.t + (o.ad ? ' (all day)' : o.kind === 'task' ? ' · ' + T().format(o.s) : ' · ' + T().format(o.s) + ' – ' + T().format(o.e));
      b.onclick = e => { e.stopPropagation(); open(o, e); };
      return b;
    }
    function open(o, e) {
      if (o.kind === 'task') { if (typeof opts.onTask === 'function') opts.onTask(o.task, e && (e.shiftKey || e.altKey) ? 'open' : 'toggle'); draw(); return; }
      edit(o.ev, o.s, o);
    }

    // ---- month
    function drawMonth(body) {
      const d = new Date(S.cur), first = new Date(d.getFullYear(), d.getMonth(), 1).getTime(), start = weekStart(first, S.ws), weeks = Math.ceil(((first - start) / DAY + monthDays(d.getFullYear(), d.getMonth())) / 7);
      const all = items(start, addDays(start, weeks * 7)), by = new Map();
      for (const o of all) { // an all-day or multi-day item shows on each of its days
        let k = sod(o.s); const last = o.ad ? addDays(o.e, -1) : sod(o.e > o.s ? o.e - 1 : o.s);
        for (let n = 0; n < 62 && k <= last; n++, k = addDays(k, 1)) { const key = dkey(k); if (!by.has(key)) by.set(key, []); by.get(key).push(o); }
      }
      const wrap = document.createElement('div'); wrap.className = 'kc-month';
      wrap.innerHTML = `<div class="kc-dow">${[0, 1, 2, 3, 4, 5, 6].map(i => `<div>${esc(nf({ weekday: 'short' }).format(addDays(start, i)))}</div>`).join('')}</div><div class="kc-weeks"></div>`;
      const ws = wrap.querySelector('.kc-weeks'), today = dkey(Date.now());
      ws.style.gridTemplateRows = `repeat(${weeks}, minmax(104px, 1fr))`;
      for (let w = 0; w < weeks; w++) {
        const row = document.createElement('div'); row.className = 'kc-week';
        for (let i = 0; i < 7; i++) {
          const day = addDays(start, w * 7 + i), key = dkey(day), dd = new Date(day), cell = document.createElement('div');
          cell.className = 'kc-cell' + (dd.getMonth() !== d.getMonth() ? ' out' : '') + (key === today ? ' today' : '') + ([0, 6].includes(dd.getDay()) ? ' we' : ''); cell.tabIndex = 0;
          cell.setAttribute('aria-label', nf({ dateStyle: 'full' }).format(day));
          const label = dd.getDate() === 1 ? nf({ day: 'numeric', month: 'short' }).format(day) : String(dd.getDate());
          cell.innerHTML = `<span class="kc-num">${esc(label)}</span>`;
          const list = by.get(key) || [], room = 3;
          for (const o of list.slice(0, list.length > room ? room - 1 : room)) cell.appendChild(chip(o));
          if (list.length > room) { const m = document.createElement('button'); m.className = 'kc-more'; m.type = 'button'; m.textContent = `+${list.length - (room - 1)} more`; m.onclick = e => { e.stopPropagation(); goto(day, 'day'); }; cell.appendChild(m); }
          cell.onclick = () => edit(null, at(day, 9, 0)); cell.onkeydown = e => { if (e.key === 'Enter' && e.target === cell) edit(null, at(day, 9, 0)); };
          row.appendChild(cell);
        }
        ws.appendChild(row);
      }
      body.appendChild(wrap);
    }

    // ---- day / week
    function drawGrid(body, days) {
      const PX = 48, start = days[0], end = addDays(days[days.length - 1], 1), all = items(start, end), n = days.length, today = dkey(Date.now());
      const wrap = document.createElement('div'); wrap.className = 'kc-tg'; wrap.style.minWidth = n > 1 ? '620px' : '0';
      const cols = `repeat(${n}, 1fr)`;
      const headHtml = `<div class="kc-tg-head" style="grid-column:1/3"><div style="display:grid;grid-template-columns:54px 1fr"><div></div><div class="kc-dhs" style="grid-template-columns:${cols}">${days.map(d => `<div class="kc-dh${dkey(d) === today ? ' today' : ''}"><small>${esc(nf({ weekday: 'short' }).format(d))}</small><span class="kc-num">${new Date(d).getDate()}</span></div>`).join('')}</div>
        <div></div><div class="kc-allday" style="grid-template-columns:${cols}">${days.map(() => '<div></div>').join('')}</div></div></div>`;
      wrap.innerHTML = headHtml + `<div class="kc-gut" style="height:${24 * PX}px">${Array.from({ length: 23 }, (_, h) => `<span style="top:${(h + 1) * PX}px">${esc(tfmt().format(at(start, h + 1, 0)))}</span>`).join('')}</div><div class="kc-cols" style="grid-template-columns:${cols};height:${24 * PX}px">${days.map(() => '<div class="kc-col"></div>').join('')}</div>`;
      const adCells = wrap.querySelectorAll('.kc-allday > div'), colEls = wrap.querySelectorAll('.kc-col');
      days.forEach((d, i) => {
        const col = colEls[i], key = dkey(d); if (key === today) col.classList.add('today');
        for (let h = 1; h < 24; h++) { const l = document.createElement('div'); l.className = 'kc-hl'; l.style.top = h * PX + 'px'; col.appendChild(l); const hh = document.createElement('div'); hh.className = 'kc-hl half'; hh.style.top = (h - 0.5) * PX + 'px'; col.appendChild(hh); }
        col.onclick = e => { const r = col.getBoundingClientRect(), mins = Math.floor((e.clientY - r.top) / PX * 2) * 30; edit(null, at(d, Math.min(23, Math.floor(mins / 60)), mins % 60)); };
        adCells[i].onclick = () => edit(null, at(d, 0, 0), null, true);
        const dayEnd = addDays(d, 1), mine = all.filter(o => o.s < dayEnd && (o.e > d || o.s >= d) && !(o.kind === 'ev' && !o.ad && o.e <= d && o.s < d));
        for (const o of mine.filter(o => o.ad)) adCells[i].appendChild(chip(o));
        const timed = mine.filter(o => !o.ad).map(o => ({ o, a: Math.max(o.s, d), b: Math.min(Math.max(o.e, o.s + 20 * 60000), dayEnd) })).sort((x, y) => x.a - y.a || y.b - x.b);
        const lanes = []; // greedy columns for overlapping items
        for (const t of timed) { let l = lanes.findIndex(end => end <= t.a); if (l < 0) { l = lanes.length; lanes.push(0); } lanes[l] = t.b; t.l = l; }
        for (const t of timed) {
          const group = timed.filter(u => u.a < t.b && u.b > t.a), nl = Math.max(...group.map(u => u.l)) + 1;
          const b = document.createElement('button'); b.type = 'button'; const o = t.o;
          b.className = 'kc-ev ' + cat(o) + (o.kind === 'task' ? ' task' + (o.task.done ? ' done' : '') : '');
          const top = (t.a - d) / HOUR * PX, h = Math.max(20, (t.b - t.a) / HOUR * PX - 2);
          b.style.cssText = `top:${top}px;height:${h}px;left:calc(${t.l / nl * 100}% + 2px);width:calc(${100 / nl}% - 5px)`;
          b.innerHTML = `<b>${o.kind === 'task' ? (o.task.done ? '☑ ' : '☐ ') : ''}${esc(o.ev.t)}</b>${h > 34 ? `<span class="tm">${esc(T().format(o.s))}${o.e > o.s ? ' – ' + esc(T().format(o.e)) : ''}</span>` : ''}`;
          b.title = o.ev.t + ' · ' + T().format(o.s) + (o.e > o.s ? ' – ' + T().format(o.e) : '');
          b.onclick = e => { e.stopPropagation(); open(o, e); }; col.appendChild(b);
        }
        if (key === today) { const nw = document.createElement('div'); nw.className = 'kc-now'; nw.style.top = (Date.now() - d) / HOUR * PX + 'px'; col.appendChild(nw); }
      });
      body.appendChild(wrap);
      if (!S._scrolled || S._scrolled !== S.view) { S._scrolled = S.view; body.scrollTop = (days.some(d => dkey(d) === today) ? Math.max(0, (new Date().getHours() - 2) * PX) : 7 * PX); }
    }

    // ---- year
    function drawYear(body) {
      const y = new Date(S.cur).getFullYear(), has = new Set(), from = new Date(y, 0, 1).getTime(), to = new Date(y + 1, 0, 1).getTime();
      for (const o of items(from, to)) { let k = sod(o.s); const last = o.ad ? addDays(o.e, -1) : sod(o.s); for (let n = 0; n < 400 && k <= last; n++, k = addDays(k, 1)) has.add(dkey(k)); }
      const wrap = document.createElement('div'); wrap.className = 'kc-year'; const today = dkey(Date.now());
      for (let m = 0; m < 12; m++) {
        const first = new Date(y, m, 1).getTime(), st = weekStart(first, S.ws), cnt = Math.ceil(((first - st) / DAY + monthDays(y, m)) / 7) * 7, box = document.createElement('div'); box.className = 'kc-mini';
        box.innerHTML = `<h4><button type="button" data-m="${m}">${esc(nf({ month: 'long' }).format(first))}</button></h4><div class="kc-mg">${[0, 1, 2, 3, 4, 5, 6].map(i => `<i>${esc(nf({ weekday: 'narrow' }).format(addDays(st, i)))}</i>`).join('')}${Array.from({ length: cnt }, (_, i) => { const d = addDays(st, i), dd = new Date(d), out = dd.getMonth() !== m; return `<button type="button" class="kc-md${out ? ' out' : ''}${dkey(d) === today ? ' today' : ''}${has.has(dkey(d)) && !out ? ' has' : ''}" data-d="${dkey(d)}" ${out ? 'tabindex="-1" aria-hidden="true"' : ''}>${dd.getDate()}</button>`; }).join('')}</div>`;
        box.querySelector('h4 button').onclick = () => goto(new Date(y, m, 1).getTime(), 'month');
        box.querySelectorAll('.kc-md').forEach(b => b.onclick = () => goto(fromKey(b.dataset.d), 'day'));
        wrap.appendChild(box);
      }
      body.appendChild(wrap);
    }

    // ---- schedule
    function drawSchedule(body) {
      const from = sod(S.cur), to = addDays(from, 60 * S.more), all = items(from, to), by = new Map();
      for (const o of all) { let k = Math.max(from, sod(o.s)); const last = o.ad ? addDays(o.e, -1) : sod(o.s); for (let n = 0; n < 62 && k <= last; n++, k = addDays(k, 1)) { const key = dkey(k); if (!by.has(key)) by.set(key, []); by.get(key).push(o); } }
      const wrap = document.createElement('div'); wrap.className = 'kc-sched'; const today = dkey(Date.now());
      if (!by.size) wrap.innerHTML = '<div class="kc-empty">Nothing scheduled in the next ' + 60 * S.more + ' days.<br>Press <b>+ New</b> to add an event.</div>';
      for (const [key, list] of [...by].sort((a, b) => a[0] < b[0] ? -1 : 1)) {
        const d = fromKey(key), row = document.createElement('div'); row.className = 'kc-sd' + (key === today ? ' today' : '');
        row.innerHTML = `<div class="kc-sdh"><b>${new Date(d).getDate()}</b><span>${esc(nf({ weekday: 'short', month: 'short' }).format(d))}</span></div><div class="kc-sdl"></div>`;
        const l = row.querySelector('.kc-sdl'); for (const o of list) l.appendChild(chip(o)); wrap.appendChild(row);
      }
      const more = document.createElement('div'); more.style.textAlign = 'center'; more.style.padding = '16px'; more.innerHTML = '<button class="kc-btn" type="button">Show 60 more days</button>'; more.firstChild.onclick = () => { S.more++; draw(); }; wrap.appendChild(more);
      body.appendChild(wrap);
    }

    // ---- the editor
    function edit(ev, startMs, occ, allDay) {
      closeDlg();
      const isNew = !ev, base = ev ? JSON.parse(JSON.stringify(ev)) : { id: uid(), t: '', s: startMs, e: startMs + HOUR, ad: !!allDay, c: '' };
      if (isNew && base.ad) { base.s = sod(startMs); base.e = addDays(base.s, 1); }
      const ds = ms => dkey(ms), tm = ms => String(new Date(ms).getHours()).padStart(2, '0') + ':' + String(new Date(ms).getMinutes()).padStart(2, '0');
      const endDay = base.ad ? addDays(base.e, -1) : base.e, r = base.r || null;
      const shade = document.createElement('div'); shade.className = 'kc-shade'; shade.setAttribute('role', 'dialog'); shade.setAttribute('aria-modal', 'true'); shade.setAttribute('aria-label', isNew ? 'New event' : 'Edit event');
      const wd = [0, 1, 2, 3, 4, 5, 6].map(i => (i + S.ws) % 7), by = new Set(r && r.by && r.by.length ? r.by : [new Date(base.s).getDay()]);
      shade.innerHTML = `<form class="kc-dlg" novalidate><h3>${isNew ? 'New event' : 'Edit event'}</h3>
        <div class="kc-f"><label for="kc-t">Title</label><input id="kc-t" type="text" maxlength="300" value="${esc(base.t)}" placeholder="Add a title" autocomplete="off"></div>
        <div class="kc-f"><label>When</label><div class="kc-row"><label><input type="checkbox" class="ad" ${base.ad ? 'checked' : ''}> All day</label></div></div>
        <div class="kc-f"><label for="kc-sd">Starts</label><div class="kc-row"><input id="kc-sd" type="date" value="${ds(base.s)}" style="flex:1"><input class="st" type="time" value="${tm(base.s)}" style="flex:1" aria-label="Start time"></div></div>
        <div class="kc-f"><label for="kc-ed">Ends</label><div class="kc-row"><input id="kc-ed" type="date" value="${ds(endDay)}" style="flex:1"><input class="et" type="time" value="${tm(base.e)}" style="flex:1" aria-label="End time"></div></div>
        <div class="kc-f"><label for="kc-r">Repeat</label><div><select id="kc-r"><option value="">Does not repeat</option><option value="d">Daily</option><option value="w">Weekly</option><option value="m">Monthly</option><option value="y">Yearly</option></select></div></div>
        <div class="rp" hidden>
          <div class="kc-f"><label for="kc-n">Every</label><div class="kc-row"><input id="kc-n" type="number" min="1" max="999" value="${r ? r.n || 1 : 1}"><span class="unit"></span></div></div>
          <div class="kc-f wds" hidden><label>On</label><div class="kc-dows">${wd.map(d => `<button type="button" data-d="${d}" aria-pressed="${by.has(d)}" aria-label="${esc(nf({ weekday: 'long' }).format(addDays(weekStart(Date.now(), 0), d)))}">${esc(nf({ weekday: 'narrow' }).format(addDays(weekStart(Date.now(), 0), d)))}</button>`).join('')}</div></div>
          <div class="kc-f"><label for="kc-en">Ends</label><div class="kc-row"><select id="kc-en"><option value="never">Never</option><option value="until">On a date</option><option value="count">After a number of times</option></select><input class="eu" type="date" hidden aria-label="Last date"><input class="ec" type="number" min="1" max="5000" value="10" hidden aria-label="Number of times"></div></div></div>
        <div class="kc-f"><label>Remind me</label><div class="kc-row">${REMS.map(([m, n]) => `<label><input type="checkbox" class="rem" value="${m}" ${(base.rem || []).includes(m) ? 'checked' : ''}> ${n}</label>`).join('')}</div></div>
        <div class="kc-f"><label for="kc-c">Category</label><div><select id="kc-c"><option value="">None</option><option value="w">Work</option><option value="p">Personal</option></select></div></div>
        <div class="kc-f"><label for="kc-l">Place</label><input id="kc-l" type="text" maxlength="300" value="${esc(base.l || '')}"></div>
        <div class="kc-f"><label for="kc-d">Notes</label><textarea id="kc-d" maxlength="5000">${esc(base.d || '')}</textarea></div>
        <div class="kc-err" role="alert"></div>
        <div class="kc-foot">${isNew ? '' : '<button type="button" class="kc-btn danger del">Delete</button>'}<span class="kc-sp"></span><button type="button" class="kc-btn cancel">Cancel</button><button type="submit" class="kc-btn pri">Save</button></div></form>`;
      root_.appendChild(shade); S.dlg = shade;
      const $ = s => shade.querySelector(s), $$ = s => [...shade.querySelectorAll(s)];
      $('#kc-r').value = r ? r.f : ''; $('#kc-c').value = base.c || ''; $('#kc-en').value = r && typeof r.until === 'number' ? 'until' : r && r.count ? 'count' : 'never';
      if (r && typeof r.until === 'number') $('.eu').value = ds(r.until); else $('.eu').value = ds(addDays(base.s, 90)); if (r && r.count) $('.ec').value = r.count;
      const sync = () => {
        const ad = $('.ad').checked; $('.st').hidden = $('.et').hidden = ad;
        const f = $('#kc-r').value; $('.rp').hidden = !f; $('.wds').hidden = f !== 'w'; $('.unit').textContent = { d: 'day(s)', w: 'week(s)', m: 'month(s)', y: 'year(s)' }[f] || '';
        const en = $('#kc-en').value; $('.eu').hidden = en !== 'until'; $('.ec').hidden = en !== 'count';
      };
      $$('.kc-dows button').forEach(b => b.onclick = () => b.setAttribute('aria-pressed', String(b.getAttribute('aria-pressed') !== 'true')));
      ['.ad', '#kc-r', '#kc-en'].forEach(s => $(s).onchange = sync); sync();
      $('.cancel').onclick = closeDlg; shade.onmousedown = e => { if (e.target === shade) closeDlg(); };
      const del = $('.del');
      if (del) del.onclick = () => {
        if (!base.r || !occ) return finishDelete(false);
        const f = $('.kc-foot'); f.innerHTML = '<span style="color:var(--kc-dim)">Delete:</span><span class="kc-sp"></span><button type="button" class="kc-btn danger one">This event only</button><button type="button" class="kc-btn danger all">All events</button><button type="button" class="kc-btn back">Keep</button>';
        f.querySelector('.one').onclick = () => finishDelete(true); f.querySelector('.all').onclick = () => finishDelete(false); f.querySelector('.back').onclick = () => edit(ev, startMs, occ, allDay);
      };
      const finishDelete = one => {
        if (one) { const e2 = Object.assign({}, ev, { ex: (ev.ex || []).concat(occ.key), u: Date.now() }); commit(e2); } else commit({ id: ev.id, x: true, u: Date.now() });
        closeDlg();
      };
      $('form').onsubmit = e => {
        e.preventDefault(); const err = $('.kc-err'); err.textContent = '';
        const title = $('#kc-t').value.trim(); if (!title) { err.textContent = 'Give the event a title.'; $('#kc-t').focus(); return; }
        const ad = $('.ad').checked, sd = $('#kc-sd').value, ed = $('#kc-ed').value || sd; if (!sd) { err.textContent = 'Choose a start date.'; return; }
        let s, en; if (ad) { s = fromKey(sd); en = addDays(fromKey(ed), 1); } else { const [sh, sm] = ($('.st').value || '09:00').split(':').map(Number), [eh, em] = ($('.et').value || '10:00').split(':').map(Number); s = at(fromKey(sd), sh, sm); en = at(fromKey(ed), eh, em); }
        if (en < s) { err.textContent = 'The event cannot end before it starts.'; return; }
        const out = { id: base.id, t: title, s, e: en, u: Date.now() }; if (ad) out.ad = true;
        const f = $('#kc-r').value;
        if (f) {
          const rule = { f, n: Math.max(1, Math.min(999, parseInt($('#kc-n').value, 10) || 1)) };
          if (f === 'w') { const d = $$('.kc-dows button[aria-pressed=true]').map(b => +b.dataset.d); if (!d.length) { err.textContent = 'Choose at least one weekday.'; return; } rule.by = d.sort((a, b) => a - b); }
          const en2 = $('#kc-en').value; if (en2 === 'until') { if (!$('.eu').value) { err.textContent = 'Choose the last date.'; return; } rule.until = fromKey($('.eu').value); } else if (en2 === 'count') rule.count = Math.max(1, Math.min(5000, parseInt($('.ec').value, 10) || 1));
          out.r = rule; if (base.ex && base.ex.length) out.ex = base.ex;
        }
        const rem = $$('.rem:checked').map(c => +c.value); if (rem.length) out.rem = rem;
        const c = $('#kc-c').value; if (c) out.c = c; const l = $('#kc-l').value.trim(); if (l) out.l = l; const d = $('#kc-d').value.trim(); if (d) out.d = d;
        commit(out); closeDlg();
      };
      setTimeout(() => $('#kc-t').focus(), 0);
    }
    function closeDlg() { if (S.dlg) { S.dlg.remove(); S.dlg = null; root_.focus({ preventScroll: true }); } }
    function commit(ev) {
      const i = S.events.findIndex(x => x.id === ev.id); if (i >= 0) S.events[i] = ev; else S.events.push(ev);
      if (typeof opts.onChange === 'function') try { opts.onChange(ev, S.events.slice()); } catch (_) { }
      draw();
    }

    // ---- draw
    function draw() {
      const keep = root_.querySelector('.kc-body'), top = keep ? keep.scrollTop : 0, dlg = S.dlg;
      root_.textContent = ''; root_.appendChild(drawBar());
      const body = document.createElement('div'); body.className = 'kc-body'; root_.appendChild(body);
      if (S.view === 'month') drawMonth(body);
      else if (S.view === 'week') drawGrid(body, Array.from({ length: 7 }, (_, i) => addDays(weekStart(S.cur, S.ws), i)));
      else if (S.view === 'day') drawGrid(body, [sod(S.cur)]);
      else if (S.view === 'year') drawYear(body);
      else drawSchedule(body);
      if (S.view !== 'week' && S.view !== 'day') body.scrollTop = top;
      if (dlg) root_.appendChild(dlg);
    }
    root_.addEventListener('keydown', e => {
      if (e.target.closest && e.target.closest('input,textarea,select')) { if (e.key === 'Escape' && S.dlg) closeDlg(); return; }
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (S.dlg) { if (e.key === 'Escape') closeDlg(); return; }
      const k = e.key.toLowerCase(), map = { d: 'day', w: 'week', m: 'month', y: 'year', a: 'schedule' };
      if (map[k]) { e.preventDefault(); setView(map[k]); } else if (k === 't') { e.preventDefault(); goto(Date.now()); } else if (k === 'j' || e.key === 'ArrowRight' && e.target === root_) { e.preventDefault(); move(1); } else if (k === 'k' || e.key === 'ArrowLeft' && e.target === root_) { e.preventDefault(); move(-1); } else if (k === 'c') { e.preventDefault(); edit(null, at(sod(S.cur), 9, 0)); }
    });
    let minute = setInterval(() => { if (!S.dlg && host.isConnected && (S.view === 'day' || S.view === 'week')) { const t = root_.querySelector('.kc-now'); if (t) draw(); } }, 60000);
    draw();
    return {
      setEvents(list) { S.events = Array.isArray(list) ? list.slice() : []; draw(); },
      getEvents() { return S.events.slice(); },
      refresh() { draw(); }, setView, goto, view: () => S.view,
      setOptions(o) { if (o && 'h12' in o) S.h12 = !!o.h12; if (o && 'ws' in o) S.ws = o.ws === 0 ? 0 : 1; if (o && 'locale' in o) S.loc = o.locale; draw(); },
      newEvent(ms) { edit(null, ms || Date.now()); },
      focus() { root_.focus({ preventScroll: true }); },
      destroy() { clearInterval(minute); root_.remove(); },
    };
  }

  const api = { mount, occurrences, VERSION: '1' };
  root.KemptCalendar = api;
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
})(typeof window !== 'undefined' ? window : globalThis);
