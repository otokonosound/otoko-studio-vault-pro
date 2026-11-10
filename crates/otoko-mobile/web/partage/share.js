'use strict';
/* Page publique d'un lien de partage Otoko Vault. Le jeton est dans l'adresse : /s/<jeton> */
const $ = s => document.querySelector(s);
const audio = $('#audio');
const BASE = location.pathname.replace(/\/+$/, '');
const ICON = {
  play: '<svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M7 4.5v15l13-7.5z"/></svg>',
  pause: '<svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/></svg>',
  dl: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>',
  lock: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/></svg>',
  heart: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z"/></svg>',
  hearton: '<svg viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z"/></svg>',
  talk: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M5 5h14v10H10l-5 4z"/></svg>',
};
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fmt = s => { s = Math.max(0, Math.round(s || 0)); return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0'); };
const fmtLong = s => { const m = Math.round((s || 0) / 60); return m < 60 ? m + ' min' : Math.floor(m / 60) + ' h ' + String(m % 60).padStart(2, '0'); };
const fmtDate = d => d ? new Date(d.length <= 10 ? d + 'T12:00' : d).toLocaleDateString('fr-FR', { day: 'numeric', month: 'long', year: 'numeric' }) : '';
const plural = (n, w) => n + ' ' + w + (n > 1 ? 's' : '');
const bg = (url, color) => `background-color:${/^#[0-9a-f]{3,8}$/i.test(color || '') ? color : '#1a1a1a'};background-image:url('${url}')`;
const opusOk = () => !!audio.canPlayType('audio/ogg; codecs=opus');

let data = null, all = [], queue = [], qi = -1, cur = null;

/* Les réponses de la personne : un identifiant tiré au hasard et gardé dans ce navigateur (rien d'autre), son prénom
   si elle le donne, et les morceaux qu'elle aime. */
const TOKEN = BASE.split('/').pop();
const store = {
  get(k, d) { try { const v = localStorage.getItem(k); return v == null ? d : JSON.parse(v); } catch (e) { return d; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch (e) {} },
};
const visitor = store.get('sv-visitor', null) || (() => {
  const b = new Uint8Array(16); crypto.getRandomValues(b);
  const v = Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
  store.set('sv-visitor', v); return v;
})();
let liked = new Set(store.get('sv-likes-' + TOKEN, []));
const STEPS = [0.1, 0.25, 0.5, 0.75, 0.9];
let heard = 0, lastAt = 0, sent = 0;

async function answer(t, body) {
  const r = await fetch(`${BASE}/t/${t.id}/feedback`, {
    method: 'POST', credentials: 'same-origin', keepalive: true,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(Object.assign({ visitor, name: store.get('sv-name', '') }, body)),
  });
  if (r.status === 403) { data.feedback = false; render(); }
  if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || 'Pas envoyé, réessaie un peu plus tard.');
}

function message(title, text) {
  document.body.classList.add('msg');
  $('#view').innerHTML = `<div><h1>${esc(title)}</h1><p>${esc(text)}</p></div>`;
}

async function load() {
  let res;
  try { res = await fetch(BASE + '/api', { credentials: 'same-origin', cache: 'no-store' }); }
  catch (e) { return message('Hors ligne', 'Le serveur ne répond pas pour le moment. Réessaie un peu plus tard.'); }
  const body = await res.json().catch(() => ({}));
  if (res.status === 401 && body.password) return askPassword(body.label, body.artist);
  if (!res.ok) return message(res.status === 410 ? 'Lien plus disponible' : 'Lien introuvable', body.error || 'Ce lien ne fonctionne pas.');
  data = body;
  render();
}

function askPassword(label, artist) {
  $('#view').innerHTML = `
    <section class="hero">
      <div class="kicker">${artist ? 'Partagé par ' + esc(artist) : 'Partage privé'}</div>
      <h1>${esc(label || 'Écoute privée')}</h1>
      <p class="desc">Ce partage est protégé par un mot de passe.</p>
      <form class="pw" id="pw" autocomplete="off">
        <label for="pwi" class="kicker" style="text-transform:none;letter-spacing:0;font-weight:500">Mot de passe</label>
        <input id="pwi" type="password" required autofocus>
        <button class="btn pri" type="submit">${ICON.lock}Entrer</button>
        <span class="err" id="pwerr" role="alert"></span>
      </form>
    </section>`;
  $('#pw').addEventListener('submit', async e => {
    e.preventDefault();
    const btn = e.target.querySelector('button'); btn.disabled = true;
    try {
      const r = await fetch(BASE + '/auth', { method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password: $('#pwi').value }) });
      if (r.ok) return load();
      $('#pwerr').textContent = (await r.json().catch(() => ({}))).error || 'Mot de passe incorrect.';
    } catch (err) { $('#pwerr').textContent = 'Le serveur ne répond pas.'; }
    btn.disabled = false;
  });
}

function trackRow(t, i, n) {
  const sub = [t.version, t.release && data.kind === 'tracks' ? t.release : ''].filter(Boolean).join(' · ') || t.artist;
  return `<div class="t" data-id="${t.id}">
    <button class="play" data-i="${i}" aria-label="Écouter ${esc(t.title)}">
      <span class="no">${n}</span>
      ${data.kind === 'tracks' ? `<span class="cv" style="${bg(t.cover + '?size=160', t.color)}"></span>` : ''}
      <span class="mid"><span class="ti">${esc(t.title)}</span><span class="su">${esc(sub)}</span></span>
      <span class="dur">${t.duration ? fmt(t.duration) : ''}</span>
    </button>
    ${data.feedback ? likeButton(t) : ''}
    ${data.can_download ? `<a class="dl" href="${t.download}" download aria-label="Télécharger ${esc(t.title)}">${ICON.dl}</a>` : ''}
  </div>`;
}

function likeButton(t) {
  const on = liked.has(t.id);
  return `<button class="like" data-like="${t.id}" aria-pressed="${on}" aria-label="${on ? 'Je n\'aime plus' : 'J\'aime'} ${esc(t.title)}">${on ? ICON.hearton : ICON.heart}</button>`;
}

async function toggleLike(t) {
  const on = !liked.has(t.id);
  on ? liked.add(t.id) : liked.delete(t.id);
  store.set('sv-likes-' + TOKEN, [...liked]);
  paintLikes();
  try { await answer(t, { kind: 'like', on }); }
  catch (e) { on ? liked.delete(t.id) : liked.add(t.id); store.set('sv-likes-' + TOKEN, [...liked]); paintLikes(); }
}

function paintLikes() {
  document.querySelectorAll('[data-like]').forEach(b => { const t = all.find(x => x.id === +b.dataset.like); if (t) b.outerHTML = likeButton(t); });
  const pl = $('#plike'); if (!cur) return;
  const on = liked.has(cur.id);
  pl.setAttribute('aria-pressed', on);
  pl.innerHTML = (on ? ICON.hearton : ICON.heart) + (on ? 'Aimé' : 'J\'aime');
}

function render() {
  all = [];
  const add = t => { t.download = `${BASE}/t/${t.id}/download`; all.push(t); return all.length - 1; };
  const rels = data.releases, single = rels.length === 1 && !data.tracks.length ? rels[0] : null;
  const head = single || rels[0] || data.tracks[0] || {};
  const color = head.color || '#3a3d46';
  document.documentElement.style.setProperty('--accent', color);
  const title = data.label || (single ? single.title : data.kind === 'discography' ? 'Discographie' : 'Sélection');
  document.title = data.artist ? title + ' · ' + data.artist : title;
  let body = '';
  if (single) {
    body = single.tracks.map((t, n) => trackRow(t, add(t), n + 1)).join('');
  } else {
    body = rels.map(r => `<div class="rel"><span class="art" style="${bg(r.cover + '?size=160', r.color)}"></span>
        <span class="mid"><span class="ti">${esc(r.title)}</span><span class="su">${esc([r.type_label, r.date ? r.date.slice(0, 4) : ''].filter(Boolean).join(' · '))}</span></span></div>`
      + r.tracks.map((t, n) => trackRow(t, add(t), n + 1)).join('')).join('')
      + data.tracks.map((t, n) => trackRow(t, add(t), n + 1)).join('');
  }
  const total = all.reduce((s, t) => s + (t.duration || 0), 0);
  const rights = data.can_download ? `<span class="tag">Téléchargement : ${esc(data.download_quality)}</span>` : '<span class="tag">Écoute seule</span>';
  const left = data.can_download && data.downloads_left != null ? ` · ${plural(data.downloads_left, 'téléchargement')} restant${data.downloads_left > 1 ? 's' : ''}` : '';
  $('#view').innerHTML = `
    <section class="hero">
      <div class="in">
        <div class="big" style="${bg(BASE + '/cover?size=600', color)}" role="img" aria-label="Pochette"></div>
        <div>
          <div class="kicker">${esc([single ? single.type_label : '', data.artist].filter(Boolean).join(' · '))}</div>
          <h1>${esc(title)}</h1>
          <div class="meta2">${esc([single && single.date ? (single.upcoming ? 'Sort le ' : 'Sortie le ') + fmtDate(single.date) : '', plural(all.length, 'titre'), total ? fmtLong(total) : ''].filter(Boolean).join(' · '))}</div>
          ${data.message ? `<p class="desc">${esc(data.message)}</p>` : single && single.description ? `<p class="desc">${esc(single.description)}</p>` : ''}
          <div class="meta2" style="margin-top:14px">${rights}${esc(left)}</div>
          ${data.feedback && all.length ? `<p class="invite">Tu peux aimer un morceau ou laisser un avis, à un instant précis si tu veux : ${esc(data.artist || 'l\'artiste')} le verra.</p>` : ''}
          <div class="actions">
            ${all.length ? `<button class="btn pri" id="playall">${ICON.play}Tout écouter</button>` : ''}
            ${data.can_download && all.length > 1 ? `<button class="btn sec" id="dlall">${ICON.dl}Tout télécharger</button>` : ''}
          </div>
        </div>
      </div>
    </section>
    <div class="wrap" id="list">${body || '<p class="desc">Rien à écouter dans ce partage pour le moment.</p>'}</div>
    <p class="foot">${data.expires_at ? 'Lien valable jusqu\'au ' + esc(fmtDate(data.expires_at)) + '. ' : ''}Partage privé : merci de ne pas le diffuser.</p>`;
  $('#list').addEventListener('click', e => {
    const l = e.target.closest('[data-like]');
    if (l) { const t = all.find(x => x.id === +l.dataset.like); if (t) toggleLike(t); return; }
    const b = e.target.closest('button.play'); if (!b) return;
    const i = +b.dataset.i;
    if (cur && all[i].id === cur.id) return toggle();
    queue = all; qi = i; play();
  });
  const pa = $('#playall'); if (pa) pa.onclick = () => { queue = all; qi = 0; play(); };
  const da = $('#dlall'); if (da) da.onclick = downloadAll;
  $('#fbrow').hidden = !data.feedback;
  if (!data.feedback) closeFeedback();
  mark();
}

async function downloadAll() {
  const btn = $('#dlall'); btn.disabled = true;
  for (const t of all) {
    const a = document.createElement('a'); a.href = t.download; a.download = ''; document.body.appendChild(a); a.click(); a.remove();
    await new Promise(r => setTimeout(r, 1500));
  }
  btn.disabled = false;
}

function play() {
  cur = queue[qi]; if (!cur) return;
  heard = 0; lastAt = 0; sent = 0;
  closeFeedback();
  $('#pcom').innerHTML = ICON.talk + 'Laisser un avis';
  audio.src = `${BASE}/t/${cur.id}/stream${opusOk() ? '' : '?f=aac'}`;
  audio.play().catch(() => {});
  $('#player').hidden = false;
  $('#pcv').style.cssText = bg(cur.cover + '?size=160', cur.color);
  $('#pti').textContent = cur.title;
  $('#psu').textContent = 'Préparation…';
  if (cur.color) document.documentElement.style.setProperty('--accent', cur.color);
  paintLikes();
  if ('mediaSession' in navigator) {
    navigator.mediaSession.metadata = new MediaMetadata({ title: cur.title, artist: cur.artist || 'Otoko', album: cur.release || data.label || '', artwork: [{ src: location.origin + cur.cover + '?size=600', sizes: '600x600', type: 'image/jpeg' }] });
  }
  mark();
}
function toggle() { audio.paused ? audio.play().catch(() => {}) : audio.pause(); }
function step(d) { if (!queue.length) return; qi = (qi + d + queue.length) % queue.length; play(); }
function mark() {
  document.querySelectorAll('.t').forEach(r => r.classList.toggle('on', !!cur && +r.dataset.id === cur.id));
  const p = !audio.paused;
  $('#play').innerHTML = p ? ICON.pause : ICON.play;
  $('#play').setAttribute('aria-label', p ? 'Pause' : 'Lecture');
  if ('mediaSession' in navigator) navigator.mediaSession.playbackState = p ? 'playing' : 'paused';
}
audio.addEventListener('playing', () => { $('#psu').textContent = cur.artist || 'Otoko'; mark(); });
audio.addEventListener('pause', mark);
audio.addEventListener('ended', () => { if (qi < queue.length - 1) step(1); else mark(); });
audio.addEventListener('error', () => { if (cur) $('#psu').textContent = 'Lecture impossible pour le moment.'; });
audio.addEventListener('timeupdate', () => {
  const d = audio.duration || (cur && cur.duration) || 0;
  // Ce qui a vraiment été écouté (un saut en avant ne compte pas), envoyé par paliers.
  const step = audio.currentTime - lastAt; lastAt = audio.currentTime;
  if (!audio.paused && step > 0 && step < 2) heard += step;
  if (cur && d && data.feedback) {
    const part = Math.min(1, heard / d);
    while (sent < STEPS.length && part >= STEPS[sent]) { answer(cur, { kind: 'reach', value: STEPS[sent] }).catch(() => {}); sent++; }
  }
  if (cur && $('#fb').hidden) $('#pcom').innerHTML = ICON.talk + 'Avis à ' + fmt(audio.currentTime);
  $('#pos').textContent = fmt(audio.currentTime); $('#dur').textContent = fmt(d);
  const pct = d ? audio.currentTime / d * 100 : 0;
  $('#fill').style.width = pct + '%'; $('#bar').setAttribute('aria-valuenow', Math.round(pct));
});
/* Un avis sur le morceau en cours : l'instant est pris à l'ouverture, la musique continue pendant qu'on écrit. */
let commentAt = 0;
function openFeedback() {
  if (!cur || !data.feedback) return;
  commentAt = audio.currentTime || 0;
  $('#fbt').textContent = 'Ton avis sur « ' + cur.title + ' »';
  $('#fbatt').textContent = 'À ' + fmt(commentAt) + ' dans le morceau';
  $('#fbat').checked = commentAt > 1;
  $('#fbname').value = store.get('sv-name', '');
  $('#fberr').textContent = '';
  $('#fbsend').disabled = false;
  $('#fb').hidden = false;
  $('#fbtext').focus();
}
function closeFeedback() { const f = $('#fb'); if (f.hidden) return; f.hidden = true; $('#fbtext').value = ''; $('#pcom').focus(); }
$('#pcom').onclick = openFeedback;
$('#plike').onclick = () => cur && toggleLike(cur);
$('#fbno').onclick = closeFeedback;
$('#fb').addEventListener('keydown', e => { if (e.key === 'Escape') closeFeedback(); });
$('#fb').addEventListener('submit', async e => {
  e.preventDefault();
  const text = $('#fbtext').value.trim(); if (!text || !cur) return;
  store.set('sv-name', $('#fbname').value.trim().slice(0, 40));
  $('#fbsend').disabled = true;
  try {
    await answer(cur, { kind: 'comment', text, time: $('#fbat').checked ? Math.round(commentAt * 10) / 10 : null });
    $('#fberr').textContent = 'Merci, c\'est envoyé.';
    setTimeout(closeFeedback, 1200);
  } catch (err) { $('#fberr').textContent = err.message; $('#fbsend').disabled = false; }
});
$('#play').onclick = toggle;
$('#prev').onclick = () => audio.currentTime > 3 ? (audio.currentTime = 0) : step(-1);
$('#next').onclick = () => step(1);
$('#bar').addEventListener('click', e => {
  const r = e.currentTarget.getBoundingClientRect(), d = audio.duration || 0;
  if (d) audio.currentTime = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * d;
});
$('#bar').addEventListener('keydown', e => {
  if (e.key === 'ArrowRight') audio.currentTime += 5;
  if (e.key === 'ArrowLeft') audio.currentTime -= 5;
});
if ('mediaSession' in navigator) {
  const set = (a, f) => { try { navigator.mediaSession.setActionHandler(a, f); } catch (e) {} };
  set('play', () => audio.play()); set('pause', () => audio.pause());
  set('previoustrack', () => step(-1)); set('nexttrack', () => step(1));
}
load();
