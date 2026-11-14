// haus 3D house view (REQ-UI-006, DD-017). Draws the house model from
// api/house with vendored three.js and colours each room by its current air
// temperature; the values refresh every minute.
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { CSS2DRenderer, CSS2DObject } from 'three/addons/renderers/CSS2DRenderer.js';

const $ = id => document.getElementById(id);
const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const dark = matchMedia('(prefers-color-scheme: dark)').matches;
// The status page's icons for a sensor to check.
const CHECK_ICON = { sensor_stuck: '⏸', sensor_silent: '📡', battery_low: '🔋', source_down: '📡' };

async function load() {
  const r = await fetch('api/house', { headers: { accept: 'application/json' } });
  if (!r.ok) throw new Error('api/house: ' + r.status);
  return r.json();
}

let data;
try { data = await load(); } catch (e) {
  $('view').innerHTML = '<p class="empty">The house model could not be loaded (' + esc(e.message) + ').</p>';
  throw e;
}
const M = data.model;
const el = $('view');
const size = () => [el.clientWidth || 300, el.clientHeight || 300];
const small = () => matchMedia('(max-width: 700px), (max-height: 600px)').matches;

const scene = new THREE.Scene();
scene.background = new THREE.Color(dark ? 0x15130f : 0xf6f3ee);
let [W, H] = size();
const camera = new THREE.PerspectiveCamera(40, W / H, 0.1, 2000);
// Portrait (a phone): keep the horizontal angle of a square view, so the
// house is not cut off at the sides; landscape keeps the 40° vertical angle.
// cover: pixels hidden by the info box, at the right (wide screens) or at
// the bottom (small ones). The picture is then the part of a frame that
// much larger, so the camera's target sits in the middle of the free area.
let cover = { x: 0, y: 0 };
function fitCamera() {
  const fw = W + cover.x, fh = H + cover.y;
  camera.aspect = fw / fh;
  camera.fov = camera.aspect < 1
    ? 2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(20)) / camera.aspect) * 180 / Math.PI
    : 40;
  if (cover.x || cover.y) camera.setViewOffset(fw, fh, cover.x, cover.y, W, H); else camera.clearViewOffset();
  camera.updateProjectionMatrix();
}
fitCamera();
const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setPixelRatio(devicePixelRatio); renderer.setSize(W, H);
el.appendChild(renderer.domElement);
const labels = new CSS2DRenderer(); labels.setSize(W, H);
Object.assign(labels.domElement.style, { position: 'absolute', top: 0, left: 0, pointerEvents: 'none' });
el.appendChild(labels.domElement);
const hemi = new THREE.HemisphereLight(0xffffff, 0x8a7f72, 1.1); scene.add(hemi);
const sun = new THREE.DirectionalLight(0xffffff, 1.2); sun.position.set(20, 40, 25); scene.add(sun);
const baseBg = scene.background.clone();

// Plan metres (x, y) -> three (x, z = -y), centred on the buildings (or rooms).
const bbox = ps => { const xs = ps.map(p => p[0]), ys = ps.map(p => p[1]); return [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)]; };
const centrePts = (M.buildings.length ? M.buildings.flatMap(b => b.outline) : M.rooms.flatMap(r => r.poly));
const [bx0, bx1, by0, by1] = bbox(centrePts), cx = (bx0 + bx1) / 2, cy = (by0 + by1) / 2;
const P = ([x, y]) => new THREE.Vector2(x - cx, -(y - cy));
// ExtrudeGeometry + rotateX(-90 deg) maps shape (sx, sy) to world (sx, 0, -sy):
// feed it plan y, not P's negated one, or everything lands mirrored.
const shapeOf = poly => new THREE.Shape(poly.map(([x, y]) => new THREE.Vector2(x - cx, y - cy)));
const mid = poly => { const [x0, x1, y0, y1] = bbox(poly); return [(x0 + x1) / 2, (y0 + y1) / 2]; };
function slab(poly, h, color, opacity, edges = true) {
  const geo = new THREE.ExtrudeGeometry(shapeOf(poly), { depth: h, bevelEnabled: false });
  geo.rotateX(-Math.PI / 2);
  const mesh = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color, transparent: opacity < 1, opacity, roughness: 0.85 }));
  const g = new THREE.Group(); g.add(mesh);
  if (edges) g.add(new THREE.LineSegments(new THREE.EdgesGeometry(geo),
    new THREE.LineBasicMaterial({ color: dark ? 0x0c0a08 : 0x78716c, transparent: true, opacity: 0.6 })));
  return { g, mesh };
}
function line(poly, y, color, close = true) {
  const pts = (close ? poly.concat([poly[0]]) : poly).map(p => { const v = P(p); return new THREE.Vector3(v.x, y, v.y); });
  return new THREE.Line(new THREE.BufferGeometry().setFromPoints(pts), new THREE.LineBasicMaterial({ color }));
}
function tempColor(t) {
  if (t == null) return new THREE.Color(dark ? 0x57534e : 0xc7c0b6);
  const stops = [[16, 0x2563eb], [20, 0x16a34a], [23, 0xeab308], [26, 0xdc2626]];
  if (t <= stops[0][0]) return new THREE.Color(stops[0][1]);
  for (let i = 1; i < stops.length; i++) if (t <= stops[i][0]) {
    const [t0, c0] = stops[i - 1], [t1, c1] = stops[i];
    return new THREE.Color(c0).lerp(new THREE.Color(c1), (t - t0) / (t1 - t0));
  }
  return new THREE.Color(stops[stops.length - 1][1]);
}
const levelById = Object.fromEntries(M.levels.map(l => [l.id, l]));
const groups = {}, base = {}, otherGroups = {}, tagGroups = {};
for (const l of M.levels) {
  const g = new THREE.Group(); groups[l.id] = g; base[l.id] = l.z; scene.add(g);
  otherGroups[l.id] = new THREE.Group(); g.add(otherGroups[l.id]);
  tagGroups[l.id] = new THREE.Group(); g.add(tagGroups[l.id]);
}
const pickables = [], roomMesh = {}, roomLabel = {};

// Our rooms, coloured by temperature.
for (const r of M.rooms) {
  const g = groups[r.level];
  const h = r.outdoor ? (r.deck ? 0.25 : 0.05) : 0.35;
  const color = r.deck ? 0xb45309 : r.outdoor ? (dark ? 0x3f3a33 : 0xe7e1d8) : tempColor(null);
  const { g: s, mesh } = slab(r.poly, h, color, r.deck ? 1 : r.outdoor ? 0.5 : 0.92);
  mesh.userData.room = r; g.add(s); pickables.push(mesh);
  if (!r.outdoor) roomMesh[r.id] = mesh;
  if (r.deck) {
    // Railing 1 m high; deck and railing stay visible through the roof.
    const rail = line(r.poly, 1.0, 0x78350f); const posts = new THREE.Group();
    for (const p of r.poly) { const v = P(p); posts.add(new THREE.Line(new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(v.x, h, v.y), new THREE.Vector3(v.x, 1.0, v.y)]), new THREE.LineBasicMaterial({ color: 0x78350f }))); }
    for (const o of [mesh, rail, ...posts.children, ...s.children]) { o.renderOrder = 10; o.material.transparent = true; o.material.opacity = 1; o.material.depthTest = false; }
    g.add(rail); g.add(posts);
  }
  if (r.haus_rooms.length || r.deck || r.note) {
    const d = document.createElement('div'); d.className = 'lbl compact';
    d.dataset.prio = r.deck ? -1 : (levelById[r.level].z < 0 ? 2 : 0) + (r.outdoor ? 1 : 0);
    const o = new CSS2DObject(d); const v = P(r.label || mid(r.poly)); o.position.set(v.x, h + 0.4, v.y); g.add(o);
    roomLabel[r.id] = d;
  }
}
// Other units and shared rooms: light slabs, one tag per unit and level.
const tagged = {};
for (const r of M.others) {
  const og = otherGroups[r.level];
  const { g: s, mesh } = slab(r.poly, r.outdoor ? 0.04 : 0.12, r.outdoor ? (dark ? 0x34302a : 0xece7df) : (dark ? 0x4a453e : 0xddd6cc), r.assumed ? 0.25 : 0.45);
  mesh.userData.other = r; og.add(s); pickables.push(mesh);
  const key = r.unit + '/' + r.level;
  if (r.unit && !r.outdoor && !tagged[key]) {
    tagged[key] = true;
    const d = document.createElement('div'); d.className = 'tag'; d.textContent = r.unit;
    const o = new CSS2DObject(d); const v = P(mid(r.poly)); o.position.set(v.x, 0.3, v.y); tagGroups[r.level].add(o);
  }
}
// Building outlines on each floor.
for (const g of Object.values(groups))
  for (const b of M.buildings.filter(b => !b.glass)) g.add(line(b.outline, 0.01, dark ? 0x6b645c : 0xa8a29e));

// Walls and roofs. Pitched roofs and vaults run along the longer side of
// the outline's box.
const roof = new THREE.Group(); scene.add(roof); roof.visible = false;
const roofMat = new THREE.MeshStandardMaterial({ color: dark ? 0x57534e : 0x6b5e55, transparent: true, opacity: 0.55, side: THREE.DoubleSide });
const wallMat = new THREE.MeshStandardMaterial({ color: 0xf5f5f4, transparent: true, opacity: 0.18, side: THREE.DoubleSide, depthWrite: false });
const glassMat = new THREE.MeshStandardMaterial({ color: 0x93c5fd, transparent: true, opacity: 0.25, side: THREE.DoubleSide, depthWrite: false });
const metalMat = new THREE.MeshStandardMaterial({ color: 0xa8a29e, metalness: 0.4, roughness: 0.4, transparent: true, opacity: 0.65, side: THREE.DoubleSide });
function addTris(tris, mat) {
  const geo = new THREE.BufferGeometry(); geo.setAttribute('position', new THREE.Float32BufferAttribute(tris, 3)); geo.computeVertexNormals();
  roof.add(new THREE.Mesh(geo, mat));
  roof.add(new THREE.LineSegments(new THREE.EdgesGeometry(geo, 20), new THREE.LineBasicMaterial({ color: dark ? 0x0c0a08 : 0x57534e, transparent: true, opacity: 0.7 })));
}
const W3 = (x, y, h) => { const v = P([x, y]); return [v.x, h, v.y]; };
function walls(O, h, mat) {
  const t = [];
  for (let i = 0; i < O.length; i++) {
    const a = O[i], b = O[(i + 1) % O.length];
    t.push(...W3(a[0], a[1], 0), ...W3(b[0], b[1], 0), ...W3(b[0], b[1], h), ...W3(a[0], a[1], 0), ...W3(b[0], b[1], h), ...W3(a[0], a[1], h));
  }
  addTris(t, mat);
}
// A frame across (u) and along (v) the ridge: Q(u, v, h) is a world point.
function frame(O) {
  const [x0, x1, y0, y1] = bbox(O), alongY = (y1 - y0) >= (x1 - x0);
  return alongY
    ? { u0: x0, u1: x1, v0: y0, v1: y1, Q: (u, v, h) => W3(u, v, h) }
    : { u0: y0, u1: y1, v0: x0, v1: x1, Q: (u, v, h) => W3(v, u, h) };
}
function gable(O, eave, ridge) {
  const { u0, u1, v0, v1, Q } = frame(O), um = (u0 + u1) / 2;
  addTris([...Q(u0, v0, eave), ...Q(um, v0, ridge), ...Q(um, v1, ridge), ...Q(u0, v0, eave), ...Q(um, v1, ridge), ...Q(u0, v1, eave),
    ...Q(u1, v0, eave), ...Q(um, v0, ridge), ...Q(um, v1, ridge), ...Q(u1, v0, eave), ...Q(um, v1, ridge), ...Q(u1, v1, eave)], roofMat);
  const g = []; for (const v of [v0, v1]) g.push(...Q(u0, v, eave), ...Q(u1, v, eave), ...Q(um, v, ridge));
  addTris(g, wallMat);
}
function hip(O, eave, ridge) {
  const { u0, u1, v0, v1, Q } = frame(O), um = (u0 + u1) / 2, inset = Math.min(u1 - u0, v1 - v0) / 2;
  const ra = [um, v0 + inset], rb = [um, v1 - inset], A = Q(...ra, ridge), B = Q(...rb, ridge);
  const c = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
  const near = p => (Math.hypot(p[0] - ra[0], p[1] - ra[1]) <= Math.hypot(p[0] - rb[0], p[1] - rb[1]) ? A : B);
  const t = [];
  for (let i = 0; i < 4; i++) {
    const p = c[i], q = c[(i + 1) % 4], np = near(p), nq = near(q);
    t.push(...Q(...p, eave), ...Q(...q, eave), ...nq); if (np !== nq) t.push(...Q(...p, eave), ...nq, ...np);
  }
  addTris(t, roofMat);
}
function dormers(O, eave, ridge, list) {     // flat box dormers on the low-u slope
  const { u0, u1, Q } = frame(O), run = (u1 - u0) / 2, top = eave + (ridge - eave) * 0.55, ui = u0 + run * 0.55, uo = u0 + 0.3, t = [];
  for (const [a, b] of list) {
    t.push(...Q(uo, a, eave), ...Q(uo, b, eave), ...Q(uo, b, top), ...Q(uo, a, eave), ...Q(uo, b, top), ...Q(uo, a, top));
    t.push(...Q(uo, a, top), ...Q(uo, b, top), ...Q(ui, b, top), ...Q(uo, a, top), ...Q(ui, b, top), ...Q(ui, a, top));
    for (const v of [a, b]) t.push(...Q(uo, v, eave), ...Q(uo, v, top), ...Q(ui, v, top));
  }
  addTris(t, metalMat);
}
function barrel(O, wall, crown) {
  const { u0, u1, v0, v1, Q } = frame(O), um = (u0 + u1) / 2, r = (u1 - u0) / 2, n = 12, t = [];
  const pt = (i, v) => { const a = Math.PI * i / n; return Q(um - r * Math.cos(a), v, wall + (crown - wall) * Math.sin(a)); };
  for (let i = 0; i < n; i++) t.push(...pt(i, v0), ...pt(i + 1, v0), ...pt(i + 1, v1), ...pt(i, v0), ...pt(i + 1, v1), ...pt(i, v1));
  addTris(t, metalMat);
}
for (const b of M.buildings) {
  walls(b.outline, b.wall_m, b.glass ? glassMat : wallMat);
  if (b.roof === 'gable') gable(b.outline, b.wall_m, b.ridge_m);
  else if (b.roof === 'hip') hip(b.outline, b.wall_m, b.ridge_m);
  else if (b.roof === 'barrel') barrel(b.outline, b.wall_m, b.ridge_m);
  else { const top = slab(b.outline, 0.05, 0xe7e5e4, 0.5); top.g.position.y = b.wall_m - 0.05; roof.add(top.g); }
  if (b.dormers.length && b.ridge_m) dormers(b.outline, b.wall_m, b.ridge_m, b.dormers);
}

// Surroundings at ground level (the level at z = 0).
const site = new THREE.Group(); scene.add(site);
const S = M.site;
// True north: north_deg clockwise from plan +y.
const bearing = deg => { const a = (M.north_deg + deg) * Math.PI / 180; return [Math.sin(a), Math.cos(a)]; };
if (S) {
  if (S.parcel) {
    const p = slab(S.parcel, 0.02, dark ? 0x23301f : 0xdfe8d2, 0.55, false); p.g.position.y = -0.06; site.add(p.g);
    site.add(line(S.parcel, 0.0, dark ? 0x84cc16 : 0x65a30d));
  }
  for (const s of S.streets) {
    const { g } = slab(s.poly, 0.02, s.kind === 'road' ? (dark ? 0x3f3f46 : 0xa8a29e) : (dark ? 0x52525b : 0xd6d3d1), 0.8, false);
    g.position.y = -0.07; site.add(g);
    if (s.kind === 'road') {
      const d = document.createElement('div'); d.className = 'tag'; d.textContent = s.name;
      const o = new CSS2DObject(d); const v = P(mid(s.poly)); o.position.set(v.x, 0.2, v.y); site.add(o);
    }
  }
  for (const n of S.neighbours) {
    const { g, mesh } = slab(n.poly, n.height_m, dark ? 0x57534e : 0xd6d3d1, 0.5);
    mesh.userData.other = { name: n.name }; pickables.push(mesh); site.add(g);
  }
  for (const p of S.parking) {
    const { g } = slab(p.poly, 0.04, dark ? 0x52525b : 0xe7e5e4, 1); g.position.y = -0.05; site.add(g);
    const d = document.createElement('div'); d.className = 'tag'; d.textContent = p.name;
    const o = new CSS2DObject(d); const v = P(mid(p.poly)); o.position.set(v.x, 0.1, v.y); site.add(o);
  }
  for (const t of S.trees) {
    const v = P(t);
    const trunk = new THREE.Mesh(new THREE.CylinderGeometry(0.25, 0.35, 4), new THREE.MeshStandardMaterial({ color: 0x78350f }));
    trunk.position.set(v.x, 2, v.y); site.add(trunk);
    const crown = new THREE.Mesh(new THREE.SphereGeometry(3.2, 16, 12), new THREE.MeshStandardMaterial({ color: 0x4d7c0f, transparent: true, opacity: 0.55 }));
    crown.position.set(v.x, 6.5, v.y); site.add(crown);
  }
  // North arrow beside the parcel (or the buildings).
  const [nx, ny] = bearing(0), dir = new THREE.Vector3(nx, 0, -ny);
  const [px0, , , py1] = bbox(S.parcel || centrePts);
  const o = P([px0 - 6, py1 - 4]), at = new THREE.Vector3(o.x, 0.2, o.y);
  site.add(new THREE.ArrowHelper(dir, at, 6, 0xc2410c, 1.6, 1.0));
  const d = document.createElement('div'); d.className = 'north'; d.textContent = 'N';
  const lab = new CSS2DObject(d); lab.position.copy(at.clone().add(dir.clone().multiplyScalar(7.2))); site.add(lab);
}

// Panel collapse (starts collapsed on small screens).
{
  const b = $('collapse'), body = $('pbody');
  const set = c => { body.style.display = c ? 'none' : ''; b.textContent = c ? '+' : '–'; };
  let c = innerWidth < 700; set(c); b.addEventListener('click', () => set(c = !c));
}
// Floors, toggles and explode.
const lv = $('levels');
for (const l of [...M.levels].sort((a, b) => b.z - a.z)) {
  const lab = document.createElement('label');
  lab.innerHTML = `<input type="checkbox" checked data-l="${esc(l.id)}"> ${esc(l.name)}`;
  lv.appendChild(lab);
}
lv.addEventListener('change', e => { groups[e.target.dataset.l].visible = e.target.checked; });
const tog = (id, f) => { const c = $(id); c.addEventListener('change', () => f(c.checked)); f(c.checked); };
tog('t-others', v => Object.values(otherGroups).forEach(g => g.visible = v));
tog('t-site', v => site.visible = v);
tog('t-roof', v => roof.visible = v);
tog('t-tags', v => Object.values(tagGroups).forEach(g => g.visible = v));
const ex = $('explode');
function place() {
  const k = parseFloat(ex.value), order = Object.keys(groups).sort((a, b) => base[a] - base[b]);
  const g0 = order.findIndex(id => base[id] >= 0);
  order.forEach((id, i) => groups[id].position.y = base[id] + (i - Math.max(g0, 0)) * k);
}
const roofBox = $('t-roof');
const setRoof = on => { if (roofBox.checked !== on) { roofBox.checked = on; roofBox.dispatchEvent(new Event('change')); } };
ex.addEventListener('input', () => { place(); if (parseFloat(ex.value) > 0) setRoof(false); });
roofBox.addEventListener('change', () => { if (roofBox.checked && parseFloat(ex.value) > 0) { ex.value = 0; place(); } });
place();
// Camera, controls and the model's viewpoints.
const target = new THREE.Vector3(0, 4, 0);
const span = Math.max(bx1 - bx0, by1 - by0, 10);
camera.position.set(span * 0.9, span * 1.2, span * 1.6);
const controls = new OrbitControls(camera, renderer.domElement);
controls.target.copy(target); controls.enableDamping = true; controls.update();
function view(v) {
  if (v.distance_m > 0) {
    const [dx, dy] = bearing(v.from_deg);
    camera.position.set(dx * v.distance_m, v.height_m, -dy * v.distance_m);
  } else {
    camera.position.set(0.01, v.height_m, 0.01);
  }
  if (v.shell) { ex.value = 0; place(); setRoof(true); }
  controls.target.copy(target); controls.update();
}
for (const v of M.views) {
  const b = document.createElement('button'); b.type = 'button'; b.textContent = v.name;
  b.addEventListener('click', () => view(v)); $('views').appendChild(b);
}

// Values: colours, labels and the info box; refreshed every minute.
let shownRoom = null;
const fmt = (x, d) => (x == null ? null : x.toFixed(d));
function applyValues() {
  for (const r of M.rooms) {
    const v = data.rooms[r.id], mesh = roomMesh[r.id];
    if (mesh) mesh.material.color.copy(tempColor(v ? v.air_c : null));
    const d = roomLabel[r.id]; if (!d) continue;
    const parts = [];
    if (v && v.air_c != null) parts.push(fmt(v.air_c, 1) + ' °C');
    if (v && v.rh != null) parts.push(fmt(v.rh, 0) + ' %');
    // An alarm about the room is red with ⚠; a sensor to check is a calm note.
    const alarm = v && v.warnings.some(w => !w.check), check = !alarm && v && v.warnings.length;
    const warned = alarm ? ' ⚠' : check ? ' ' + CHECK_ICON[v.warnings[0].kind] : '';
    d.dataset.full = esc(r.name) + warned + (parts.length ? ' <span>' + parts.join(' · ') + '</span>' : '');
    d.dataset.compact = esc(r.short || r.name) + warned + (v && v.air_c != null ? ' <span>' + fmt(v.air_c, 0) + '°</span>' : '');
    d.innerHTML = labelMode === 'full' ? d.dataset.full : d.dataset.compact;
    d.classList.toggle('warned', !!alarm); d.classList.toggle('checked', !!check);
  }
  renderAlerts();
  $('asof').textContent = 'Values as of ' + new Date(data.now_ms).toLocaleTimeString() + '.';
  if (shownRoom) showRoom(shownRoom);
}
const info = $('info');
function showRoom(r) {
  shownRoom = r;
  const v = data.rooms[r.id];
  let h = `<b>${esc(r.name)}</b> <span class="dim">${esc(levelById[r.level].name)}</span>`;
  if (v) {
    for (const w of v.warnings.filter(w => !w.check)) h += `<p class="warn">${esc(w.message)}</p>`;
    for (const w of v.warnings.filter(w => w.check)) h += `<p class="check">${esc(CHECK_ICON[w.kind] + ' ' + w.short)}</p>`;
    const sum = [v.air_c != null ? fmt(v.air_c, 1) + ' °C' : null, v.rh != null ? fmt(v.rh, 0) + ' %' : null].filter(Boolean);
    if (sum.length) h += `<p class="sum">${esc(sum.join(' · '))}</p>`;
    // The sensor list folds away during the tour and on small screens, so
    // the box does not cover the house; a tap opens it.
    const open = !touring && !small() ? ' open' : '';
    if (v.series.length) h += `<details${open}><summary>Sensors (${v.series.length})</summary>` + '<table>' + v.series.map(s =>
      `<tr${s.stale ? ' class="stale"' : ''}><td><a href="sensor/${s.id}" title="${esc(s.name)}">${esc(s.label)}</a>${s.used || !['Temperature', 'Humidity'].includes(s.quantity) ? '' : ' <span class="dim">(not counted)</span>'}</td>` +
      `<td class="v">${s.value == null ? '–' : esc(fmt(s.value, 1) + (s.unit ? ' ' + s.unit : ''))}</td></tr>` +
      `<tr><td class="dim" colspan="2">${esc(s.age)}</td></tr>`).join('') + '</table></details>';
    else h += '<p class="dim">No live sensor in this room.</p>';
  } else if (!r.note) h += '<p class="dim">No sensor mapped to this room.</p>';
  if (r.note) h += `<p class="dim">${esc(r.note)}</p>`;
  info.innerHTML = h; info.style.display = 'block';
}
const ray = new THREE.Raycaster(), mouse = new THREE.Vector2();
const visible = o => { for (let p = o; p; p = p.parent) if (!p.visible) return false; return true; };
renderer.domElement.addEventListener('click', ev => {
  const rect = renderer.domElement.getBoundingClientRect();
  mouse.set((ev.clientX - rect.left) / rect.width * 2 - 1, -((ev.clientY - rect.top) / rect.height) * 2 + 1);
  ray.setFromCamera(mouse, camera);
  const hit = ray.intersectObjects(pickables.filter(visible))[0];
  if (!hit) { info.style.display = 'none'; shownRoom = null; return; }
  const r = hit.object.userData.room, o = hit.object.userData.other;
  if (r) { showRoom(r); return; }
  shownRoom = null;
  info.innerHTML = `<b>${esc(o.unit ? o.unit + ' · ' : '')}${esc(o.name)}</b> <span class="dim">${esc(o.level ? levelById[o.level].name : '')}</span>` +
    (o.assumed ? '<p class="dim">Layout assumed.</p>' : '');
  info.style.display = 'block';
});
new ResizeObserver(() => {
  [W, H] = size(); fitCamera();
  renderer.setSize(W, H); labels.setSize(W, H);
}).observe(el);

// CSS2D labels follow only their own visible flag: hide them with their groups.
const css = []; scene.traverse(o => { if (o.isCSS2DObject) css.push(o); });
const shown = o => { for (let p = o.parent; p; p = p.parent) if (!p.visible) return false; return true; };
// Labels: mode switch, and drop a label that would overlap one already shown
// (living floors first, then decks and outdoor areas, then below ground).
let labelMode = 'compact';
const roomLabels = css.filter(o => o.element.classList.contains('lbl'));
$('t-labels').addEventListener('change', e => {
  labelMode = e.target.value;
  for (const o of roomLabels) {
    const d = o.element; d.innerHTML = labelMode === 'full' ? d.dataset.full : d.dataset.compact;
    d.classList.toggle('compact', labelMode === 'compact');
  }
});
// Tour (REQ-UI-010): visit the rooms one after another. Rooms with a
// warning come up every other step, a new warning is shown at once, and
// warned rooms pulse red. Touching the view pauses the tour for a minute.
const tourRooms = M.rooms.filter(r => r.haus_rooms.length && !r.deck)
  .sort((a, b) => levelById[b.level].z - levelById[a.level].z || a.name.localeCompare(b.name));
// Only alarms about a room come first in the tour and pulse; a sensor to
// check is listed, calmly, and waits for its turn.
const warnedRooms = () => tourRooms.filter(r => data.rooms[r.id]?.warnings.some(w => !w.check));
const checkRooms = () => tourRooms.filter(r => { const w = data.rooms[r.id]?.warnings || []; return w.length && w.every(x => x.check); });
const levelBox = id => lv.querySelector(`input[data-l="${CSS.escape(id)}"]`);
const tourBtn = $('tour'), every = $('every'), box = el.parentElement;
let touring = false, tourTimer = null, step = 0, iAll = 0, iWarn = 0, pausedUntil = 0, fly = null, current = null;
function centreOf(r) { const v = P(r.label || mid(r.poly)); return new THREE.Vector3(v.x, groups[r.level].position.y + 0.4, v.y); }
function focusOn(r) {
  // Floors above the room are hidden, so it is seen from above.
  const z = levelById[r.level].z;
  for (const l of M.levels) groups[l.id].visible = l.z <= z && levelBox(l.id).checked;
  setRoof(false);
  const t = centreOf(r), [x0, x1, y0, y1] = bbox(r.poly);
  const dir = camera.position.clone().sub(controls.target);
  dir.y = Math.max(dir.y, 0.8 * Math.hypot(dir.x, dir.z)); dir.normalize();
  fly = { t, p: t.clone().add(dir.multiplyScalar(Math.max(2.6 * Math.max(x1 - x0, y1 - y0), 14))) };
  current = r; showRoom(r); renderAlerts();
  for (const [id, d] of Object.entries(roomLabel)) d.classList.toggle('current', id === r.id);
}
function next() {
  if (Date.now() < pausedUntil || !tourRooms.length) return;
  const w = warnedRooms();
  focusOn(w.length && step++ % 2 === 0 ? w[iWarn++ % w.length] : tourRooms[iAll++ % tourRooms.length]);
}
function restartTimer() { clearInterval(tourTimer); if (touring) tourTimer = setInterval(next, every.value * 1000); }
function setTour(on) {
  touring = on; tourBtn.textContent = on ? '■ Stop tour' : '▶ Tour'; tourBtn.setAttribute('aria-pressed', on);
  box.classList.toggle('touring', on);
  if (on) { pausedUntil = 0; next(); }
  else {
    fly = null; current = null;
    for (const l of M.levels) groups[l.id].visible = levelBox(l.id).checked;
    for (const d of Object.values(roomLabel)) d.classList.remove('current');
    renderAlerts();
  }
  restartTimer();
}
tourBtn.addEventListener('click', () => setTour(!touring));
every.addEventListener('change', restartTimer);
const pause = () => { if (touring) { pausedUntil = Date.now() + 60_000; fly = null; } };
renderer.domElement.addEventListener('pointerdown', pause);
renderer.domElement.addEventListener('wheel', pause, { passive: true });
// The warned rooms, always on top of the view; a click flies there.
function renderAlerts() {
  const a = $('alerts'); a.textContent = '';
  for (const r of [...warnedRooms(), ...checkRooms()]) {
    const ws = data.rooms[r.id].warnings, alarm = ws.some(w => !w.check);
    const b = document.createElement('button'); b.type = 'button';
    b.textContent = (alarm ? '⚠ ' : CHECK_ICON[ws[0].kind] + ' ') + (r.short || r.name);
    b.title = ws.map(w => w.check ? w.short : w.message).join('\n');
    b.classList.toggle('check', !alarm);
    b.classList.toggle('on', current === r);
    b.addEventListener('click', () => { if (touring) pausedUntil = Date.now() + 60_000; focusOn(r); });
    a.appendChild(b);
  }
}
// Full screen: the button or the f key. Where the browser refuses (iPad
// Safari for anything but video, in-app browsers), the view fills the
// browser window instead; the button or Esc brings the page back.
const fsBtn = $('fullscreen');
const fsEl = () => document.fullscreenElement || document.webkitFullscreenElement;
const fsLabel = () => {
  const on = !!(fsEl() || box.classList.contains('max'));
  fsBtn.textContent = on ? '✕' : '⛶'; fsBtn.title = on ? 'Leave full screen (f, Esc)' : 'Full screen (f)';
  fsBtn.setAttribute('aria-label', on ? 'Leave full screen' : 'Full screen');
};
const setMax = on => { box.classList.toggle('max', on); document.documentElement.classList.toggle('house-max', on); fsLabel(); };
function toggleFs() {
  if (box.classList.contains('max')) return setMax(false);
  if (fsEl()) { (document.exitFullscreen || document.webkitExitFullscreen).call(document); return; }
  const req = box.requestFullscreen || box.webkitRequestFullscreen;
  let p;
  try { p = req && req.call(box); } catch (e) { p = null; }
  Promise.resolve(p).catch(() => {}).then(() => setTimeout(() => { if (!fsEl()) setMax(true); }, 400));
}
fsBtn.addEventListener('click', toggleFs);
document.addEventListener('keydown', e => {
  if (e.key === 'Escape' && box.classList.contains('max')) setMax(false);
  else if (e.key === 'f' && !e.target.closest('input, select, textarea')) toggleFs();
});
document.addEventListener('fullscreenchange', fsLabel);
document.addEventListener('webkitfullscreenchange', fsLabel);
// Weather and sky in the scene (REQ-UI-014): the sun lights the house from
// where it stands (time, place, the model's north), dusk and night darken
// the scene, overcast dims the sun, rain, snow or fog fall around the house
// slanted by the wind. The weather comes from haus (api/weather: the
// Miniserver's weather service, else Open-Meteo); none, and only the sky
// changes. The checkbox in the panel turns it off.
let wx = null;
const wxBox = $('t-weather'), wxLine = $('wx');
function geoPlace() {
  const w = window.hausIdle && window.hausIdle.where ? window.hausIdle.where() : null;
  return w ? [w.lat, w.lon] : [51, 10];
}
// The sun's azimuth (degrees from north, clockwise) and elevation.
function sunPos(t, lat, lon) {
  const r = Math.PI / 180, d = t / 86400000 - 10957.5;
  const g = (357.529 + 0.98560028 * d) * r, q = 280.459 + 0.98564736 * d;
  const L = (q + 1.915 * Math.sin(g) + 0.020 * Math.sin(2 * g)) * r, e = (23.439 - 0.00000036 * d) * r;
  const ra = Math.atan2(Math.cos(e) * Math.sin(L), Math.cos(L)), dec = Math.asin(Math.sin(e) * Math.sin(L));
  const H = ((280.46061837 + 360.98564736629 * d + lon) % 360) * r - ra, la = lat * r;
  const el = Math.asin(Math.sin(la) * Math.sin(dec) + Math.cos(la) * Math.cos(dec) * Math.cos(H));
  const az = Math.atan2(-Math.sin(H), Math.tan(dec) * Math.cos(la) - Math.sin(la) * Math.cos(H));
  return { az: (az / r + 360) % 360, el: el / r };
}
const night = new THREE.Color(dark ? 0x0b1020 : 0x1e2a44), grey = new THREE.Color(dark ? 0x2a2a2e : 0xc9cdd2);
const warm = new THREE.Color(0xffb36b), white = new THREE.Color(0xffffff);
const kind = c => c == null ? null : c >= 95 ? 'storm' : (c >= 71 && c <= 77) || c === 85 || c === 86 ? 'snow'
  : c >= 51 ? 'rain' : c === 45 || c === 48 ? 'fog' : c >= 3 ? 'overcast' : c >= 2 ? 'cloudy' : 'clear';
// Particles in a box around the buildings.
const fx = new THREE.Group(); scene.add(fx);
const boxW = span * 1.6, boxH = 30;
let drops = null, flakes = null, flash = 0;
function particles(n, rain) {
  fx.clear();
  drops = flakes = null;
  if (!n) return;
  const pos = new Float32Array(n * (rain ? 6 : 3));
  for (let i = 0; i < n; i++) {
    const x = (Math.random() - 0.5) * boxW, y = Math.random() * boxH, z = (Math.random() - 0.5) * boxW;
    if (rain) pos.set([x, y, z, x, y + 0.6, z], i * 6); else pos.set([x, y, z], i * 3);
  }
  const geo = new THREE.BufferGeometry(); geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  if (rain) { drops = new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color: dark ? 0x93c5fd : 0x5b7fa6, transparent: true, opacity: 0.55 })); fx.add(drops); }
  else { flakes = new THREE.Points(geo, new THREE.PointsMaterial({ color: 0xffffff, size: 0.22, transparent: true, opacity: 0.9 })); fx.add(flakes); }
}
function applyWeather() {
  const on = wxBox.checked, k = on && wx ? kind(wx.weather_code) : null;
  const heavy = wx && [65, 67, 75, 82, 86, 95, 96, 99].includes(wx.weather_code), light = wx && [51, 53, 61, 71, 80, 85].includes(wx.weather_code);
  const n = k === 'rain' || k === 'storm' ? (heavy ? 2600 : light ? 900 : 1600) : k === 'snow' ? (heavy ? 2200 : light ? 700 : 1400) : 0;
  particles(n, k === 'rain' || k === 'storm');
  scene.fog = k === 'fog' ? new THREE.Fog(grey, span * 0.6, span * 3) : null;
  wxLine.textContent = wx ? 'Outside: ' + Math.round(wx.temperature_2m) + ' °C, ' + (wx.text || k) +
    (wx.wind_speed_10m >= 20 ? ', wind ' + Math.round(wx.wind_speed_10m) + ' km/h' : '') + '.' : '';
}
async function loadWeather() {
  try {
    const r = await fetch('api/weather', { headers: { accept: 'application/json' } });
    wx = r.ok ? (await r.json()).current : null;
  } catch (e) { wx = null; }
  applyWeather();
}
wxBox.addEventListener('change', applyWeather);
loadWeather(); setInterval(loadWeather, 5 * 60_000);
// Light and sky for the moment; called every frame (cheap).
function sky(now, dt) {
  const [lat, lon] = geoPlace(), s = sunPos(now, lat, lon), k = wxBox.checked && wx ? kind(wx.weather_code) : null;
  const [dx, dy] = bearing(s.az), c = Math.cos(s.el * Math.PI / 180), R = span * 2;
  sun.position.set(dx * c * R, Math.max(Math.sin(s.el * Math.PI / 180), 0.05) * R, -dy * c * R);
  const day = THREE.MathUtils.smoothstep(s.el, -6, 6);          // civil twilight
  const dim = k === 'overcast' || k === 'fog' ? 0.45 : k === 'rain' || k === 'snow' || k === 'storm' ? 0.35 : k === 'cloudy' ? 0.75 : 1;
  sun.intensity = (s.el > 0 ? 0.4 + Math.min(1, s.el / 30) : 0) * dim * 1.2;
  sun.color.copy(white).lerp(warm, 1 - THREE.MathUtils.smoothstep(s.el, 2, 18));
  hemi.intensity = 0.35 + 0.75 * day;
  const bg = night.clone().lerp(dim < 1 ? baseBg.clone().lerp(grey, 1 - dim) : baseBg, day);
  if (flash > 0) { flash -= dt; hemi.intensity += 2; bg.lerp(white, 0.5); }
  else if (k === 'storm' && Math.random() < 0.002) flash = 0.12;
  scene.background.copy(bg);
  if (scene.fog) scene.fog.color.copy(bg);
  // Particles fall, drift with the wind (it blows towards from + 180°).
  const wind = wx && wx.wind_direction_10m != null ? bearing(wx.wind_direction_10m + 180) : [0, 0];
  const ws = wx ? Math.min(wx.wind_speed_10m || 0, 60) / 10 : 0;
  const p = (drops || flakes);
  if (p) {
    const a = p.geometry.attributes.position.array, rain = !!drops, step = rain ? 6 : 3, v = rain ? 22 : 1.4;
    for (let i = 0; i < a.length; i += step) {
      const sway = rain ? 0 : Math.sin(now / 700 + i) * 0.3;
      for (let j = 0; j < step; j += 3) {
        a[i + j] += (wind[0] * ws + sway) * dt; a[i + j + 1] -= v * dt; a[i + j + 2] += -wind[1] * ws * dt;
      }
      if (a[i + 1] < 0) {
        const x = (Math.random() - 0.5) * boxW, z = (Math.random() - 0.5) * boxW;
        a[i] = x; a[i + 1] = boxH; a[i + 2] = z;
        if (rain) { a[i + 3] = x + wind[0] * ws * 0.03; a[i + 4] = boxH + 0.6; a[i + 5] = z - wind[1] * ws * 0.03; }
      }
    }
    p.geometry.attributes.position.needsUpdate = true;
  }
}
applyValues();
// ?tour or ?tour=<seconds> starts the tour (a wall screen).
{
  const q = new URLSearchParams(location.search);
  if (q.has('tour')) {
    const sec = parseInt(q.get('tour'), 10);
    if ([...every.options].some(o => +o.value === sec)) every.value = String(sec);
    // A wall screen: the view fills the screen (✕ or Esc brings the page
    // back) and the panel is collapsed.
    setMax(true);
    if ($('pbody').style.display !== 'none') $('collapse').click();
    setTour(true);
  }
}
setInterval(async () => {
  try {
    const before = new Set(warnedRooms().map(r => r.id));
    data = { ...data, ...(await load()) }; applyValues();
    const fresh = warnedRooms().find(r => !before.has(r.id));
    if (fresh && touring) { focusOn(fresh); restartTimer(); }
  } catch (e) { $('asof').textContent = 'Update failed: ' + e.message; }
}, 60_000);
let frameNo = 0;
function declutter() {
  const placed = [];
  const order = roomLabels.filter(o => o.visible).sort((a, b) => a.element.dataset.prio - b.element.dataset.prio);
  for (const o of order) {
    const d = o.element;
    if (labelMode === 'off') { d.style.visibility = 'hidden'; continue; }
    d.style.visibility = '';
    const r = d.getBoundingClientRect();
    if (placed.some(q => r.left < q.right && r.right > q.left && r.top < q.bottom && r.bottom > q.top)) d.style.visibility = 'hidden';
    else placed.push(r);
  }
}
const pulseOff = new THREE.Color(0x000000);
let lastFrame = 0;
(function loop(now) {
  const dt = Math.min(0.05, lastFrame ? ((now || 0) - lastFrame) / 1000 : 0.016); lastFrame = now || 0;
  sky(Date.now(), dt);
  if (fly) {
    controls.target.lerp(fly.t, 0.08); camera.position.lerp(fly.p, 0.08);
    if (camera.position.distanceTo(fly.p) < 0.05) fly = null;
  }
  const k = 0.35 + 0.35 * Math.sin((now || 0) / 300);
  for (const r of tourRooms) {
    const m = roomMesh[r.id]; if (!m) continue;
    // A warned room glows in its own colour (a red tint would read as
    // another temperature); its label and the alert strip are red.
    if (data.rooms[r.id]?.warnings.some(w => !w.check)) m.material.emissive.copy(m.material.color).multiplyScalar(k); else m.material.emissive.copy(pulseOff);
  }
  // While the info box is open, centre the picture in the part of the view
  // it does not cover.
  const infoOpen = info.style.display === 'block';
  const c = !infoOpen ? { x: 0, y: 0 } : small()
    ? { x: 0, y: Math.min(info.offsetHeight + 20, H * 0.6) }
    : { x: Math.min(info.offsetWidth + 20, W * 0.5), y: 0 };
  if (Math.abs(c.x - cover.x) > 4 || Math.abs(c.y - cover.y) > 4) { cover = c; fitCamera(); }
  controls.update(); for (const o of css) o.visible = shown(o); renderer.render(scene, camera); labels.render(scene, camera); if (frameNo++ % 6 === 0) declutter(); requestAnimationFrame(loop); })();
