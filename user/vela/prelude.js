// prelude.js — a API de navegador do Vela, escrita em JS sobre as funcoes
// nativas de __vela (que trabalham com ids inteiros de nodos do DOM em C).
// Roda antes de qualquer script da pagina, num contexto QuickJS novo.
(function () {
'use strict';
const V = globalThis.__vela;
delete globalThis.__vela;
const G = globalThis;
const docId = V.doc();

// ------------------------------------------------------------- utilidades
function reportError(e) {
  let m = e && e.stack ? (e.message || String(e)) + '\n' + e.stack : String(e);
  V.log(2, m);
  try {
    const ev = new ErrorEvent('error', { message: e && e.message ? e.message : String(e), error: e });
    if (typeof G.onerror === 'function') G.onerror(ev.message, '', 0, 0, e);
  } catch (_) {}
}
class DOMException extends Error {
  constructor(message = '', name = 'Error') { super(message); this.name = name; this.code = 0; }
}
class DOMRect {
  constructor(x = 0, y = 0, w = 0, h = 0) { this.x = x; this.y = y; this.width = w; this.height = h; }
  get left() { return this.x; } get top() { return this.y; }
  get right() { return this.x + this.width; } get bottom() { return this.y + this.height; }
  toJSON() { return { x: this.x, y: this.y, width: this.width, height: this.height, top: this.top, left: this.left, right: this.right, bottom: this.bottom }; }
}
function kebab(p) {
  p = String(p);
  if (p.startsWith('--')) return p;
  if (p === 'cssFloat') return 'float';
  return p.replace(/[A-Z]/g, m => '-' + m.toLowerCase()).replace(/^(webkit|moz|ms)-/, '-$1-');
}
function camel(p) { return p.replace(/-([a-z])/g, (_, c) => c.toUpperCase()); }
function list(a) {
  a.item = i => a[i] || null;
  return a;
}

// ------------------------------------------------------------------ eventos
class Event {
  constructor(type, o = {}) {
    this.type = String(type); this.bubbles = !!o.bubbles; this.cancelable = !!o.cancelable;
    this.composed = !!o.composed; this.defaultPrevented = false; this._stop = false; this._stopNow = false;
    this.target = null; this.currentTarget = null; this.eventPhase = 0; this.isTrusted = false;
    this.timeStamp = V.now();
  }
  preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
  stopPropagation() { this._stop = true; }
  stopImmediatePropagation() { this._stop = this._stopNow = true; }
  get cancelBubble() { return this._stop; } set cancelBubble(v) { if (v) this._stop = true; }
  get returnValue() { return !this.defaultPrevented; } set returnValue(v) { if (!v) this.preventDefault(); }
  get srcElement() { return this.target; }
  composedPath() { return this._path || []; }
  initEvent(t, b, c) { this.type = t; this.bubbles = !!b; this.cancelable = !!c; }
}
Event.NONE = 0; Event.CAPTURING_PHASE = 1; Event.AT_TARGET = 2; Event.BUBBLING_PHASE = 3;
class CustomEvent extends Event {
  constructor(t, o = {}) { super(t, o); this.detail = o.detail === undefined ? null : o.detail; }
  initCustomEvent(t, b, c, d) { this.initEvent(t, b, c); this.detail = d; }
}
class UIEvent extends Event { constructor(t, o = {}) { super(t, o); this.view = G; this.detail = o.detail || 0; } }
class MouseEvent extends UIEvent {
  constructor(t, o = {}) {
    super(t, o);
    this.clientX = o.clientX || 0; this.clientY = o.clientY || 0;
    this.screenX = this.clientX; this.screenY = this.clientY;
    this.pageX = this.clientX; this.pageY = this.clientY + V.viewport()[2];
    this.offsetX = 0; this.offsetY = 0; this.x = this.clientX; this.y = this.clientY;
    this.button = o.button || 0; this.buttons = o.buttons || 0; this.which = 1;
    this.ctrlKey = !!o.ctrlKey; this.shiftKey = !!o.shiftKey; this.altKey = !!o.altKey; this.metaKey = !!o.metaKey;
    this.relatedTarget = o.relatedTarget || null;
  }
  getModifierState() { return false; }
}
class PointerEvent extends MouseEvent { constructor(t, o = {}) { super(t, o); this.pointerId = 1; this.pointerType = 'mouse'; this.isPrimary = true; } }
class WheelEvent extends MouseEvent { constructor(t, o = {}) { super(t, o); this.deltaX = o.deltaX || 0; this.deltaY = o.deltaY || 0; this.deltaMode = 0; } }
class KeyboardEvent extends UIEvent {
  constructor(t, o = {}) {
    super(t, o);
    this.key = o.key || ''; this.code = o.code || ''; this.keyCode = o.keyCode || 0; this.which = this.keyCode;
    this.charCode = o.charCode || 0; this.ctrlKey = !!o.ctrlKey; this.shiftKey = !!o.shiftKey;
    this.altKey = !!o.altKey; this.metaKey = !!o.metaKey; this.repeat = false; this.isComposing = false; this.location = 0;
  }
  getModifierState() { return false; }
}
class FocusEvent extends UIEvent { constructor(t, o = {}) { super(t, o); this.relatedTarget = o.relatedTarget || null; } }
class InputEvent extends UIEvent { constructor(t, o = {}) { super(t, o); this.data = o.data || null; this.inputType = o.inputType || ''; } }
class SubmitEvent extends Event { constructor(t, o = {}) { super(t, o); this.submitter = o.submitter || null; } }
class ErrorEvent extends Event { constructor(t, o = {}) { super(t, o); this.message = o.message || ''; this.error = o.error || null; this.filename = ''; this.lineno = 0; this.colno = 0; } }
class PopStateEvent extends Event { constructor(t, o = {}) { super(t, o); this.state = o.state === undefined ? null : o.state; } }
class HashChangeEvent extends Event { constructor(t, o = {}) { super(t, o); this.oldURL = o.oldURL || ''; this.newURL = o.newURL || ''; } }
class MessageEvent extends Event { constructor(t, o = {}) { super(t, o); this.data = o.data; this.origin = o.origin || ''; this.source = o.source || null; this.ports = []; } }
class ProgressEvent extends Event { constructor(t, o = {}) { super(t, o); this.loaded = o.loaded || 0; this.total = o.total || 0; this.lengthComputable = !!o.lengthComputable; } }
class TouchEvent extends UIEvent {}
class AnimationEvent extends Event {}
class TransitionEvent extends Event {}
class StorageEvent extends Event {}
class PageTransitionEvent extends Event { constructor(t, o = {}) { super(t, o); this.persisted = false; } }

function invoke(t, ev, capture, phase) {
  ev.currentTarget = t; ev.eventPhase = phase;
  const l = t._ls && t._ls[ev.type];
  if (l) {
    for (const e of l.slice()) {
      if (e.capture !== capture) continue;
      if (e.once) t.removeEventListener(ev.type, e.fn, e.capture);
      try {
        const r = typeof e.fn === 'function' ? e.fn.call(t, ev) : e.fn.handleEvent(ev);
        if (r === false) ev.preventDefault();
      } catch (err) { reportError(err); }
      if (ev._stopNow) return;
    }
  }
  if (!capture) {
    const h = t['on' + ev.type];
    if (typeof h === 'function') {
      try { if (h.call(t, ev) === false) ev.preventDefault(); } catch (err) { reportError(err); }
    } else if (t instanceof Element && h === undefined) {
      const code = V.attr(t._id, 'on' + ev.type);
      if (code) {
        try { const f = new Function('event', code); if (f.call(t, ev) === false) ev.preventDefault(); }
        catch (err) { reportError(err); }
      }
    }
  }
}

class EventTarget {
  addEventListener(type, fn, opt) {
    if (!fn) return;
    const capture = typeof opt === 'boolean' ? opt : !!(opt && opt.capture);
    const once = !!(opt && opt.once);
    const m = this._ls || (this._ls = Object.create(null));
    const l = m[type] || (m[type] = []);
    if (l.some(e => e.fn === fn && e.capture === capture)) return;
    l.push({ fn, capture, once });
    if (opt && opt.signal) opt.signal.addEventListener('abort', () => this.removeEventListener(type, fn, capture));
  }
  removeEventListener(type, fn, opt) {
    const capture = typeof opt === 'boolean' ? opt : !!(opt && opt.capture);
    const l = this._ls && this._ls[type];
    if (!l) return;
    const i = l.findIndex(e => e.fn === fn && e.capture === capture);
    if (i >= 0) l.splice(i, 1);
  }
  dispatchEvent(ev) {
    ev.target = this;
    const path = [];
    for (let n = this; n; n = n._parentTarget ? n._parentTarget() : null) path.push(n);
    ev._path = path;
    for (let i = path.length - 1; i > 0 && !ev._stop; i--) invoke(path[i], ev, true, 1);
    if (!ev._stop) { invoke(this, ev, true, 2); if (!ev._stopNow) invoke(this, ev, false, 2); }
    if (ev.bubbles) for (let i = 1; i < path.length && !ev._stop; i++) invoke(path[i], ev, false, 3);
    ev.currentTarget = null; ev.eventPhase = 0;
    return !ev.defaultPrevented;
  }
}

// ---------------------------------------------------------------- nodos
const cache = new Map();
const TAGS = Object.create(null);
const customRegistry = new Map(), customNames = new Map(), waiting = new Map();
// promove o nodo `id` a uma instancia da classe `cls` (custom element)
function upgrade(id, cls, existing) {
  const obj = existing || Object.create(cls.prototype);
  if (existing) Object.setPrototypeOf(obj, cls.prototype);
  obj._id = id; obj._ce = true;
  cache.set(id, obj);
  upgrading = obj;
  try { Reflect.construct(cls, [], cls); } catch (e) { reportError(e); }
  upgrading = null;
  const obs = cls.observedAttributes;
  if (obs && typeof obj.attributeChangedCallback === 'function')
    for (const a of obs) { const v = V.attr(id, String(a).toLowerCase()); if (v !== null) try { obj.attributeChangedCallback(a, null, v); } catch (e) { reportError(e); } }
  return obj;
}
function attrChanged(el, n, old, v) {
  const obs = el.constructor.observedAttributes;
  if (obs && obs.includes(n) && typeof el.attributeChangedCallback === 'function')
    try { el.attributeChangedCallback(n, old, v); } catch (e) { reportError(e); }
}
function connectOne(el) {
  if (el._conn) return;
  el._conn = true;
  if (typeof el.connectedCallback === 'function') try { el.connectedCallback(); } catch (e) { reportError(e); }
}
// nodos recem-inseridos: connectedCallback nos custom elements da subarvore
function connected(list) {
  if (!customRegistry.size) return;
  for (const n of list) {
    if (!n || n.nodeType !== 1 || !n.isConnected) continue;
    for (const id of [n._id].concat(V.qsa(n._id, '*', false) || [])) {
      const tag = V.tag(id).toLowerCase();
      if (tag.indexOf('-') > 0 && customRegistry.has(tag)) connectOne(W(id));
    }
  }
}
function W(id) {
  if (!id) return null;
  let w = cache.get(id);
  if (w) return w;
  const t = V.nodeType(id);
  if (t === 1) {
    const tag = V.tag(id);
    const cc = tag.indexOf('-') > 0 && customRegistry.get(tag.toLowerCase());
    if (cc) return upgrade(id, cc);
    const C = TAGS[tag] || HTMLElement; w = new C(id);
  }
  else if (t === 3) w = new Text(id);
  else if (t === 8) w = new Comment(id);
  else if (t === 9) return document;
  else if (t === 11) w = new DocumentFragment(id);
  else return null;
  cache.set(id, w);
  return w;
}
function ID(n) {
  if (n == null) return 0;
  if (typeof n !== 'object' || !n._id) throw new TypeError('o argumento nao e um Node');
  return n._id;
}
function toNode(x) { return x instanceof Node ? x : document.createTextNode(String(x)); }

let upgrading = null;           // elemento sendo promovido a custom element
class Node extends EventTarget {
  constructor(id) {
    super();
    if (id === undefined) {
      // new MeuElemento() ou promocao (customElements)
      if (upgrading) { const u = upgrading; upgrading = null; return u; }
      const tag = customNames.get(new.target);
      if (!tag) throw new TypeError('Illegal constructor');
      id = V.create(tag);
      cache.set(id, this);
      this._ce = true;
    }
    this._id = id;
  }
  _parentTarget() { const p = V.parent(this._id); return p ? W(p) : (this._id === docId ? G : null); }
  get nodeType() { return V.nodeType(this._id); }
  get nodeName() {
    const t = this.nodeType;
    return t === 1 ? V.tag(this._id) : t === 3 ? '#text' : t === 8 ? '#comment' : t === 9 ? '#document' : '#document-fragment';
  }
  get parentNode() { return W(V.parent(this._id)); }
  get parentElement() { const p = this.parentNode; return p && p.nodeType === 1 ? p : null; }
  get firstChild() { return W(V.first(this._id)); }
  get lastChild() { return W(V.last(this._id)); }
  get nextSibling() { return W(V.next(this._id)); }
  get previousSibling() { return W(V.prev(this._id)); }
  get childNodes() { const a = []; for (let c = V.first(this._id); c; c = V.next(c)) a.push(W(c)); return list(a); }
  hasChildNodes() { return !!V.first(this._id); }
  get textContent() { return this._id === docId ? null : V.text(this._id); }
  set textContent(v) { V.setText(this._id, v == null ? '' : String(v)); }
  get nodeValue() { const t = this.nodeType; return t === 3 || t === 8 ? V.text(this._id) : null; }
  set nodeValue(v) { const t = this.nodeType; if (t === 3 || t === 8) V.setText(this._id, String(v)); }
  get ownerDocument() { return this._id === docId ? null : document; }
  get isConnected() { for (let n = this._id; n; n = V.parent(n)) if (n === docId) return true; return false; }
  getRootNode() { let n = this; while (n.parentNode) n = n.parentNode; return n; }
  appendChild(c) { const f = c && c.nodeType === 11 ? c.childNodes : null; V.insert(this._id, ID(c), 0); connected(f || [c]); return c; }
  insertBefore(c, ref) { const f = c && c.nodeType === 11 ? c.childNodes : null; V.insert(this._id, ID(c), ref ? ID(ref) : 0); connected(f || [c]); return c; }
  removeChild(c) { if (V.parent(ID(c)) !== this._id) throw new DOMException('not a child', 'NotFoundError'); V.remove(c._id); return c; }
  replaceChild(n, o) { V.insert(this._id, ID(n), ID(o)); V.remove(o._id); connected([n]); return o; }
  contains(o) { if (!o) return false; for (let n = o._id; n; n = V.parent(n)) if (n === this._id) return true; return false; }
  cloneNode(deep) { return W(V.clone(this._id, !!deep)); }
  isEqualNode(o) { return o === this; }
  isSameNode(o) { return o === this; }
  compareDocumentPosition(o) {
    if (o === this) return 0;
    if (this.contains(o)) return 20;
    if (o.contains(this)) return 10;
    return 4;
  }
  normalize() {}
  get baseURI() { return V.url(); }
  // ChildNode
  remove() { V.remove(this._id); }
  before(...ns) { const p = this.parentNode; if (p) for (const n of ns) p.insertBefore(toNode(n), this); }
  after(...ns) { const p = this.parentNode; if (!p) return; let ref = this.nextSibling; for (const n of ns) p.insertBefore(toNode(n), ref); }
  replaceWith(...ns) { const p = this.parentNode; if (!p) return; this.before(...ns); this.remove(); }
}
const NODE_CONSTS = { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, COMMENT_NODE: 8,
  DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11,
  DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2, DOCUMENT_POSITION_FOLLOWING: 4,
  DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16 };
for (const k in NODE_CONSTS) { Node[k] = NODE_CONSTS[k]; Node.prototype[k] = NODE_CONSTS[k]; }

class CharacterData extends Node {
  get data() { return V.text(this._id); } set data(v) { V.setText(this._id, String(v)); }
  get length() { return this.data.length; }
  appendData(s) { this.data += s; }
  substringData(o, n) { return this.data.substr(o, n); }
}
class Text extends CharacterData {
  get wholeText() { return this.data; }
  splitText(o) { const t = this.data; this.data = t.slice(0, o); const n = document.createTextNode(t.slice(o)); this.after(n); return n; }
}
class Comment extends CharacterData {}

// ParentNode (Element, Document, DocumentFragment)
const ParentNode = {
  get children() { const a = []; for (let c = V.first(this._id); c; c = V.next(c)) if (V.nodeType(c) === 1) a.push(W(c)); a.namedItem = n => a.find(e => e.id === n || e.getAttribute('name') === n) || null; return list(a); },
  get firstElementChild() { for (let c = V.first(this._id); c; c = V.next(c)) if (V.nodeType(c) === 1) return W(c); return null; },
  get lastElementChild() { for (let c = V.last(this._id); c; c = V.prev(c)) if (V.nodeType(c) === 1) return W(c); return null; },
  get childElementCount() { let n = 0; for (let c = V.first(this._id); c; c = V.next(c)) if (V.nodeType(c) === 1) n++; return n; },
  querySelector(s) { const r = V.qsa(this._id, String(s), true); if (r === null) throw new DOMException("'" + s + "' is not a valid selector", 'SyntaxError'); return W(r[0] || 0); },
  querySelectorAll(s) { const r = V.qsa(this._id, String(s), false); if (r === null) throw new DOMException("'" + s + "' is not a valid selector", 'SyntaxError'); return list(r.map(W)); },
  getElementsByTagName(t) { t = String(t); return this.querySelectorAll(t === '*' ? '*' : t.replace(/[^\w-]/g, '')); },
  getElementsByClassName(c) { const p = String(c).trim().split(/\s+/).filter(Boolean); if (!p.length) return list([]); return this.querySelectorAll(p.map(x => '.' + CSS.escape(x)).join('')); },
  append(...ns) { for (const n of ns) this.appendChild(toNode(n)); },
  prepend(...ns) { const f = this.firstChild; for (const n of ns) this.insertBefore(toNode(n), f); },
  replaceChildren(...ns) { while (this.firstChild) this.firstChild.remove(); this.append(...ns); },
};
function mixin(C, obj) { Object.defineProperties(C.prototype, Object.getOwnPropertyDescriptors(obj)); }

class DOMTokenList {
  constructor(el, attr) { this._el = el; this._a = attr; }
  _get() { return (this._el.getAttribute(this._a) || '').split(/\s+/).filter(Boolean); }
  _set(a) { this._el.setAttribute(this._a, a.join(' ')); }
  get length() { return this._get().length; }
  get value() { return this._el.getAttribute(this._a) || ''; } set value(v) { this._el.setAttribute(this._a, v); }
  item(i) { return this._get()[i] || null; }
  contains(t) { return this._get().includes(String(t)); }
  add(...ts) { const a = this._get(); let ch = false; for (const t of ts) if (!a.includes(String(t))) { a.push(String(t)); ch = true; } if (ch || !this._el.hasAttribute(this._a)) this._set(a); }
  remove(...ts) { const a = this._get(); const b = a.filter(x => !ts.map(String).includes(x)); if (b.length !== a.length) this._set(b); }
  toggle(t, force) { t = String(t); const has = this.contains(t); if (force === undefined) force = !has; if (force && !has) this.add(t); else if (!force && has) this.remove(t); return force; }
  replace(a, b) { const l = this._get(); const i = l.indexOf(a); if (i < 0) return false; l[i] = b; this._set(l); return true; }
  supports() { return true; }
  forEach(f, t) { this._get().forEach(f, t); }
  entries() { return this._get().entries(); } keys() { return this._get().keys(); } values() { return this._get().values(); }
  [Symbol.iterator]() { return this._get()[Symbol.iterator](); }
  toString() { return this.value; }
}

function parseStyle(s) {
  const m = new Map();
  if (!s) return m;
  for (const part of s.split(';')) {
    const i = part.indexOf(':');
    if (i < 0) continue;
    let k = part.slice(0, i).trim();
    if (!k.startsWith('--')) k = k.toLowerCase();
    if (k) m.set(k, part.slice(i + 1).trim());
  }
  return m;
}
function makeStyle(el) {
  function write(m) { let s = ''; for (const [k, v] of m) s += k + ': ' + v + '; '; el.setAttribute('style', s.trim()); }
  const api = {
    getPropertyValue(p) { const v = parseStyle(el.getAttribute('style')).get(kebab(p)); return v ? v.replace(/\s*!important$/, '') : ''; },
    getPropertyPriority(p) { const v = parseStyle(el.getAttribute('style')).get(kebab(p)) || ''; return /!important$/.test(v) ? 'important' : ''; },
    setProperty(p, v, pr) {
      const m = parseStyle(el.getAttribute('style')); p = kebab(p);
      if (v == null || v === '') m.delete(p); else m.set(p, String(v) + (pr ? ' !important' : ''));
      write(m);
    },
    removeProperty(p) { const m = parseStyle(el.getAttribute('style')); p = kebab(p); const o = m.get(p) || ''; if (m.delete(p)) write(m); return o; },
    get cssText() { return el.getAttribute('style') || ''; },
    set cssText(v) { el.setAttribute('style', String(v)); },
    get length() { return parseStyle(el.getAttribute('style')).size; },
    item(i) { return [...parseStyle(el.getAttribute('style')).keys()][i] || ''; },
  };
  return new Proxy(api, {
    get(t, p) { if (p in t) return t[p]; if (typeof p !== 'string') return undefined; return t.getPropertyValue(p); },
    set(t, p, v) { if (p === 'cssText') t.cssText = v; else if (typeof p === 'string') t.setProperty(p, v); return true; },
    has(t, p) { return true; },
  });
}
function makeDataset(el) {
  const key = p => 'data-' + String(p).replace(/[A-Z]/g, m => '-' + m.toLowerCase());
  return new Proxy({}, {
    get(t, p) { if (typeof p !== 'string') return undefined; const v = el.getAttribute(key(p)); return v === null ? undefined : v; },
    set(t, p, v) { el.setAttribute(key(p), String(v)); return true; },
    has(t, p) { return el.hasAttribute(key(p)); },
    deleteProperty(t, p) { el.removeAttribute(key(p)); return true; },
    ownKeys() { return el.getAttributeNames().filter(n => n.startsWith('data-')).map(n => camel(n.slice(5))); },
    getOwnPropertyDescriptor(t, p) { const v = el.getAttribute(key(p)); return v === null ? undefined : { value: v, enumerable: true, configurable: true, writable: true }; },
  });
}

class Element extends Node {
  get tagName() { return V.tag(this._id); }
  get localName() { return this.tagName.toLowerCase(); }
  get namespaceURI() { return 'http://www.w3.org/1999/xhtml'; }
  get prefix() { return null; }
  get id() { return V.attr(this._id, 'id') || ''; } set id(v) { V.setAttr(this._id, 'id', String(v)); }
  get className() { return V.attr(this._id, 'class') || ''; } set className(v) { V.setAttr(this._id, 'class', String(v)); }
  get classList() { return this._cl || (this._cl = new DOMTokenList(this, 'class')); }
  get slot() { return ''; }
  getAttribute(n) { return V.attr(this._id, String(n)); }
  setAttribute(n, v) {
    n = String(n).toLowerCase();
    const old = this._ce ? V.attr(this._id, n) : null;
    V.setAttr(this._id, n, String(v));
    if (this._ce) attrChanged(this, n, old, String(v));
  }
  removeAttribute(n) {
    n = String(n).toLowerCase();
    const old = this._ce ? V.attr(this._id, n) : null;
    V.delAttr(this._id, n);
    if (this._ce && old !== null) attrChanged(this, n, old, null);
  }
  hasAttribute(n) { return V.attr(this._id, String(n)) !== null; }
  hasAttributes() { return V.attrs(this._id).length > 0; }
  toggleAttribute(n, f) { const h = this.hasAttribute(n); if (f === undefined) f = !h; if (f) { if (!h) this.setAttribute(n, ''); } else this.removeAttribute(n); return f; }
  getAttributeNames() { const a = V.attrs(this._id), r = []; for (let i = 0; i < a.length; i += 2) r.push(a[i]); return r; }
  get attributes() {
    const a = V.attrs(this._id), r = [];
    for (let i = 0; i < a.length; i += 2) r.push({ name: a[i], value: a[i + 1], nodeName: a[i], nodeValue: a[i + 1], localName: a[i], specified: true });
    r.getNamedItem = n => r.find(x => x.name === String(n).toLowerCase()) || null;
    return list(r);
  }
  getAttributeNode(n) { const v = this.getAttribute(n); return v === null ? null : { name: n, value: v }; }
  setAttributeNS(ns, n, v) { this.setAttribute(n, v); }
  getAttributeNS(ns, n) { return this.getAttribute(n); }
  removeAttributeNS(ns, n) { this.removeAttribute(n); }
  hasAttributeNS(ns, n) { return this.hasAttribute(n); }
  get innerHTML() { return V.html(this._id, false); }
  set innerHTML(v) { V.setHtml(this._id, v == null ? '' : String(v)); connected(this.children); }
  get outerHTML() { return V.html(this._id, true); }
  set outerHTML(v) { if (!this.parentNode) return; V.insertHtml(this._id, 'beforebegin', String(v)); this.remove(); }
  insertAdjacentHTML(pos, h) { V.insertHtml(this._id, String(pos), String(h)); }
  insertAdjacentElement(pos, el) {
    pos = String(pos).toLowerCase();
    if (pos === 'beforebegin') this.before(el); else if (pos === 'afterend') this.after(el);
    else if (pos === 'afterbegin') this.prepend(el); else this.append(el);
    return el;
  }
  insertAdjacentText(pos, t) { this.insertAdjacentElement(pos, document.createTextNode(t)); }
  get innerText() { return this.textContent; } set innerText(v) { this.textContent = v; }
  get outerText() { return this.textContent; }
  closest(sel) { for (let e = this; e && e.nodeType === 1; e = e.parentElement) if (e.matches(sel)) return e; return null; }
  matches(sel) { const r = V.matches(this._id, String(sel)); if (r < 0) throw new DOMException("'" + sel + "' is not a valid selector", 'SyntaxError'); return !!r; }
  webkitMatchesSelector(s) { return this.matches(s); }
  msMatchesSelector(s) { return this.matches(s); }
  getBoundingClientRect() { const r = V.rect(this._id); return new DOMRect(r[0], r[1] - V.viewport()[2], r[2], r[3]); }
  getClientRects() { const r = this.getBoundingClientRect(); return r.width || r.height ? [r] : []; }
  get offsetWidth() { return V.rect(this._id)[2]; }
  get offsetHeight() { return V.rect(this._id)[3]; }
  get offsetTop() { return V.rect(this._id)[1]; }
  get offsetLeft() { return V.rect(this._id)[0]; }
  get offsetParent() { return this.parentElement; }
  get clientWidth() { return this._id === document.documentElement._id ? V.viewport()[0] : V.rect(this._id)[2]; }
  get clientHeight() { return this._id === document.documentElement._id ? V.viewport()[1] : V.rect(this._id)[3]; }
  get clientTop() { return 0; } get clientLeft() { return 0; }
  get scrollWidth() { return this.clientWidth; } get scrollHeight() { return V.rect(this._id)[3]; }
  get scrollTop() { return this === document.documentElement || this === document.body ? V.viewport()[2] : 0; }
  set scrollTop(v) { if (this === document.documentElement || this === document.body) V.scrollTo(+v || 0); }
  get scrollLeft() { return 0; } set scrollLeft(v) {}
  scrollIntoView() { V.scrollTo(V.rect(this._id)[1]); }
  scrollTo() {} scroll() {} scrollBy() {}
  focus() { V.focus(this._id); }
  blur() { if (V.focused() === this._id) V.focus(0); }
  click() {
    const ok = this.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    if (ok) {
      const a = this.closest('a[href]');
      if (a) V.navigate(a.getAttribute('href'));
      else if (this.localName === 'input' && /^(submit|image)$/i.test(this.type) && this.form) this.form.requestSubmit(this);
      else if (this.localName === 'button' && this.type === 'submit' && this.form) this.form.requestSubmit(this);
      else if (this.localName === 'input' && /^(checkbox|radio)$/i.test(this.type)) this.checked = !this.checked;
    }
  }
  get style() { return this._st || (this._st = makeStyle(this)); }
  set style(v) { this.setAttribute('style', String(v)); }
  get dataset() { return this._ds || (this._ds = makeDataset(this)); }
  get hidden() { return this.hasAttribute('hidden'); } set hidden(v) { this.toggleAttribute('hidden', !!v); }
  attachShadow() { this._shadow = true; return this; }
  get shadowRoot() { return this._shadow ? this : null; }
  animate() { const p = Promise.resolve(); return { finished: p, ready: p, cancel() {}, play() {}, pause() {}, finish() {}, reverse() {}, onfinish: null, addEventListener() {}, removeEventListener() {} }; }
  getAnimations() { return []; }
  requestFullscreen() { return Promise.reject(new DOMException('', 'NotSupportedError')); }
  setPointerCapture() {} releasePointerCapture() {} hasPointerCapture() { return false; }
  checkVisibility() { return V.computed(this._id, 'display') !== 'none'; }
}
mixin(Element, ParentNode);

// atributos refletidos como propriedades
function reflect(C, specs) {
  for (const spec of specs) {
    const [prop, attr, kind] = spec;
    Object.defineProperty(C.prototype, prop, {
      configurable: true,
      get() {
        const v = V.attr(this._id, attr);
        if (kind === 'b') return v !== null;
        if (kind === 'n') { const n = parseInt(v, 10); return isNaN(n) ? (spec[3] || 0) : n; }
        if (kind === 'u') return v === null ? '' : (V.resolve(V.url(), v) || v);
        return v === null ? (spec[3] !== undefined ? spec[3] : '') : v;
      },
      set(v) {
        if (kind === 'b') { if (v) V.setAttr(this._id, attr, ''); else V.delAttr(this._id, attr); }
        else V.setAttr(this._id, attr, String(v));
      },
    });
  }
}
class HTMLElement extends Element {}
reflect(HTMLElement, [['title', 'title'], ['lang', 'lang'], ['dir', 'dir'], ['accessKey', 'accesskey'],
  ['tabIndex', 'tabindex', 'n', -1], ['draggable', 'draggable', 'b'], ['spellcheck', 'spellcheck', 'b'],
  ['contentEditable', 'contenteditable', 's', 'inherit'], ['translate', 'translate', 'b'], ['inert', 'inert', 'b'],
  ['nonce', 'nonce'], ['popover', 'popover']]);
Object.defineProperty(HTMLElement.prototype, 'isContentEditable', { get() { return false; } });

const mk = (name, tags, specs, extra) => {
  const C = ({ [name]: class extends HTMLElement {} })[name];
  if (specs) reflect(C, specs);
  if (extra) Object.defineProperties(C.prototype, Object.getOwnPropertyDescriptors(extra));
  for (const t of tags) TAGS[t] = C;
  G[name] = C;
  return C;
};
function merge(...objs) { const o = {}; for (const x of objs) Object.defineProperties(o, Object.getOwnPropertyDescriptors(x)); return o; }
const formOf = { get form() { return this.closest('form'); } };
const valueProps = {
  get value() {
    if (this.localName === 'select') { const o = this.selectedOptions[0]; return o ? o.value : ''; }
    return V.value(this._id);
  },
  set value(v) {
    if (this.localName === 'select') { for (const o of this.options) o.selected = o.value === String(v); return; }
    V.setValue(this._id, v == null ? '' : String(v));
  },
  get defaultValue() { return this.getAttribute('value') || ''; }, set defaultValue(v) { this.setAttribute('value', v); },
  get checked() { return V.checked(this._id); }, set checked(v) {
    if (v && this.type === 'radio' && this.name) {
      const f = this.form || document;
      for (const r of f.querySelectorAll('input[type=radio]')) if (r !== this && r.name === this.name) V.setChecked(r._id, false);
    }
    V.setChecked(this._id, !!v);
  },
  get defaultChecked() { return this.hasAttribute('checked'); }, set defaultChecked(v) { this.toggleAttribute('checked', !!v); },
  get validity() { return { valid: true, valueMissing: false, typeMismatch: false }; },
  get validationMessage() { return ''; }, get willValidate() { return true; },
  checkValidity() { return true; }, reportValidity() { return true; }, setCustomValidity() {},
  select() {}, setSelectionRange() {}, setRangeText() {},
  get selectionStart() { return this.value.length; }, set selectionStart(v) {}, get selectionEnd() { return this.value.length; }, set selectionEnd(v) {},
  get selectionDirection() { return 'none'; }, set selectionDirection(v) {},
  get labels() { return this.id ? document.querySelectorAll('label[for="' + this.id + '"]') : list([]); },
  get type() { const t = (V.attr(this._id, 'type') || '').toLowerCase(); return t || (this.localName === 'button' ? 'submit' : this.localName === 'select' ? 'select-one' : this.localName === 'textarea' ? 'textarea' : 'text'); },
  set type(v) { V.setAttr(this._id, 'type', String(v)); },
};
mk('HTMLAnchorElement', ['A'], [['href', 'href', 'u'], ['target', 'target'], ['rel', 'rel'], ['download', 'download'], ['hreflang', 'hreflang'], ['name', 'name']], {
  get protocol() { return new URL(this.href).protocol; }, get host() { return new URL(this.href).host; },
  get hostname() { return new URL(this.href).hostname; }, get pathname() { return new URL(this.href).pathname; },
  get search() { return new URL(this.href).search; }, get hash() { return new URL(this.href).hash; },
  get origin() { return new URL(this.href).origin; }, get port() { return new URL(this.href).port; },
  get text() { return this.textContent; }, get relList() { return new DOMTokenList(this, 'rel'); }, toString() { return this.href; } });
mk('HTMLAreaElement', ['AREA'], [['href', 'href', 'u'], ['alt', 'alt']]);
mk('HTMLImageElement', ['IMG'], [['src', 'src', 'u'], ['alt', 'alt'], ['srcset', 'srcset'], ['sizes', 'sizes'],
  ['crossOrigin', 'crossorigin'], ['loading', 'loading'], ['decoding', 'decoding'], ['useMap', 'usemap'], ['isMap', 'ismap', 'b'], ['referrerPolicy', 'referrerpolicy']], {
  get width() { return V.rect(this._id)[2] || parseInt(this.getAttribute('width')) || 0; }, set width(v) { this.setAttribute('width', v); },
  get height() { return V.rect(this._id)[3] || parseInt(this.getAttribute('height')) || 0; }, set height(v) { this.setAttribute('height', v); },
  get naturalWidth() { return V.rect(this._id)[2]; }, get naturalHeight() { return V.rect(this._id)[3]; },
  get complete() { return true; }, get currentSrc() { return this.src; }, decode() { return Promise.resolve(); } });
mk('HTMLInputElement', ['INPUT'], [['name', 'name'], ['placeholder', 'placeholder'], ['disabled', 'disabled', 'b'],
  ['readOnly', 'readonly', 'b'], ['required', 'required', 'b'], ['multiple', 'multiple', 'b'], ['autofocus', 'autofocus', 'b'],
  ['autocomplete', 'autocomplete'], ['min', 'min'], ['max', 'max'], ['step', 'step'], ['pattern', 'pattern'],
  ['maxLength', 'maxlength', 'n', -1], ['size', 'size', 'n', 20], ['src', 'src', 'u'], ['alt', 'alt'], ['accept', 'accept'], ['inputMode', 'inputmode']],
  merge(formOf, valueProps, { get files() { return list([]); }, set files(v) {}, get valueAsNumber() { return parseFloat(this.value); }, set valueAsNumber(v) { this.value = String(v); },
    get indeterminate() { return !!this._ind; }, set indeterminate(v) { this._ind = !!v; } }));
mk('HTMLButtonElement', ['BUTTON'], [['name', 'name'], ['disabled', 'disabled', 'b'], ['autofocus', 'autofocus', 'b'], ['formAction', 'formaction', 'u']], merge(formOf, valueProps));
mk('HTMLTextAreaElement', ['TEXTAREA'], [['name', 'name'], ['placeholder', 'placeholder'], ['disabled', 'disabled', 'b'],
  ['readOnly', 'readonly', 'b'], ['required', 'required', 'b'], ['rows', 'rows', 'n', 2], ['cols', 'cols', 'n', 20], ['maxLength', 'maxlength', 'n', -1]],
  merge(formOf, valueProps));
mk('HTMLSelectElement', ['SELECT'], [['name', 'name'], ['disabled', 'disabled', 'b'], ['multiple', 'multiple', 'b'], ['required', 'required', 'b'], ['size', 'size', 'n', 0]],
  merge(formOf, valueProps, {
    get options() { const a = this.querySelectorAll('option'); a.add = o => this.appendChild(o); a.remove = i => { const o = a[i]; if (o) o.remove(); }; return a; },
    get selectedOptions() { const o = this.options; const s = o.filter(x => x.selected); return list(s.length ? s : o.slice(0, 1)); },
    get selectedIndex() { const o = this.options; const i = o.findIndex(x => x.selected); return i < 0 ? (o.length ? 0 : -1) : i; },
    set selectedIndex(i) { this.options.forEach((o, k) => o.selected = k === i); },
    get length() { return this.options.length; }, add(o) { this.appendChild(o); }, item(i) { return this.options[i] || null; } }));
mk('HTMLOptionElement', ['OPTION'], [['label', 'label'], ['disabled', 'disabled', 'b']], {
  get value() { return V.value(this._id); }, set value(v) { this.setAttribute('value', v); },
  get text() { return this.textContent; }, set text(v) { this.textContent = v; },
  get selected() { return V.checked(this._id) || (this.hasAttribute('selected') && V.checked(this._id) !== false); },
  set selected(v) {
    if (v) { const s = this.closest('select'); if (s && !s.multiple) for (const o of s.querySelectorAll('option')) if (o !== this) { V.setChecked(o._id, false); o.removeAttribute('selected'); } this.setAttribute('selected', ''); }
    else this.removeAttribute('selected');
    V.setChecked(this._id, !!v);
  },
  get index() { const s = this.closest('select'); return s ? s.options.indexOf(this) : 0; }, get form() { return this.closest('form'); },
  get defaultSelected() { return this.hasAttribute('selected'); }, set defaultSelected(v) { this.toggleAttribute('selected', !!v); } });
mk('HTMLOptGroupElement', ['OPTGROUP'], [['label', 'label'], ['disabled', 'disabled', 'b']]);
mk('HTMLFormElement', ['FORM'], [['action', 'action', 'u'], ['method', 'method', 's', 'get'], ['target', 'target'], ['name', 'name'],
  ['enctype', 'enctype'], ['noValidate', 'novalidate', 'b'], ['acceptCharset', 'accept-charset'], ['autocomplete', 'autocomplete']], {
  get elements() { const a = this.querySelectorAll('input,select,textarea,button'); for (const e of a) { const n = e.getAttribute('name') || e.id; if (n && !(n in a)) a[n] = e; } a.namedItem = n => a[n] || null; return a; },
  get length() { return this.elements.length; },
  submit() { V.submit(this._id); },
  requestSubmit(sub) { if (this.dispatchEvent(new SubmitEvent('submit', { bubbles: true, cancelable: true, submitter: sub || null }))) V.submit(this._id); },
  reset() { for (const e of this.elements) if ('value' in e && e.localName !== 'button') e.value = e.defaultValue || ''; },
  checkValidity() { return true; }, reportValidity() { return true; } });
mk('HTMLLabelElement', ['LABEL'], [['htmlFor', 'for']], merge(formOf, { get control() { return this.htmlFor ? document.getElementById(this.htmlFor) : this.querySelector('input,select,textarea'); } }));
mk('HTMLFieldSetElement', ['FIELDSET'], [['disabled', 'disabled', 'b'], ['name', 'name']], formOf);
mk('HTMLLegendElement', ['LEGEND'], []);
mk('HTMLScriptElement', ['SCRIPT'], [['src', 'src', 'u'], ['type', 'type'], ['async', 'async', 'b'], ['defer', 'defer', 'b'],
  ['crossOrigin', 'crossorigin'], ['integrity', 'integrity'], ['noModule', 'nomodule', 'b'], ['charset', 'charset'], ['referrerPolicy', 'referrerpolicy']],
  { get text() { return this.textContent; }, set text(v) { this.textContent = v; } });
const HTMLScriptElement = G.HTMLScriptElement;
HTMLScriptElement.supports = t => t === 'classic';
mk('HTMLStyleElement', ['STYLE'], [['media', 'media'], ['type', 'type']], { get sheet() { return { cssRules: [], insertRule() { return 0; }, deleteRule() {} }; } });
mk('HTMLLinkElement', ['LINK'], [['href', 'href', 'u'], ['rel', 'rel'], ['media', 'media'], ['type', 'type'], ['as', 'as'],
  ['crossOrigin', 'crossorigin'], ['integrity', 'integrity'], ['hreflang', 'hreflang'], ['sizes', 'sizes']], { get relList() { return new DOMTokenList(this, 'rel'); }, get sheet() { return null; } });
mk('HTMLMetaElement', ['META'], [['name', 'name'], ['content', 'content'], ['httpEquiv', 'http-equiv'], ['charset', 'charset']]);
mk('HTMLBaseElement', ['BASE'], [['href', 'href', 'u'], ['target', 'target']]);
mk('HTMLTitleElement', ['TITLE'], [], { get text() { return this.textContent; }, set text(v) { this.textContent = v; } });
mk('HTMLIFrameElement', ['IFRAME'], [['src', 'src', 'u'], ['name', 'name'], ['width', 'width'], ['height', 'height'], ['allow', 'allow'], ['loading', 'loading']],
  { get contentWindow() { return null; }, get contentDocument() { return null; } });
mk('HTMLCanvasElement', ['CANVAS'], [['width', 'width', 'n', 300], ['height', 'height', 'n', 150]], {
  getContext(type) { return type === '2d' ? makeCtx2d(this) : null; },
  toDataURL() { return 'data:,'; }, toBlob(cb) { setTimeout(() => cb(null), 0); } });
const mediaProps = { play() { return Promise.resolve(); }, pause() {}, load() {}, canPlayType() { return ''; },
  get paused() { return true; }, get duration() { return NaN; }, get currentTime() { return 0; }, set currentTime(v) {},
  get muted() { return true; }, set muted(v) {}, get volume() { return 0; }, set volume(v) {}, get readyState() { return 0; } };
mk('HTMLMediaElement', [], [['src', 'src', 'u'], ['autoplay', 'autoplay', 'b'], ['loop', 'loop', 'b'], ['controls', 'controls', 'b']], mediaProps);
mk('HTMLVideoElement', ['VIDEO'], [['src', 'src', 'u'], ['poster', 'poster', 'u'], ['autoplay', 'autoplay', 'b'], ['loop', 'loop', 'b'], ['playsInline', 'playsinline', 'b']], mediaProps);
mk('HTMLAudioElement', ['AUDIO'], [['src', 'src', 'u']], mediaProps);
mk('HTMLSourceElement', ['SOURCE'], [['src', 'src', 'u'], ['srcset', 'srcset'], ['type', 'type'], ['media', 'media']]);
mk('HTMLTemplateElement', ['TEMPLATE'], [], { get content() { const f = document.createDocumentFragment(); for (const c of this.childNodes) f.appendChild(c.cloneNode(true)); return f; } });
mk('HTMLTableElement', ['TABLE'], [['border', 'border'], ['width', 'width']], {
  get rows() { return this.querySelectorAll('tr'); }, get tBodies() { return this.querySelectorAll('tbody'); },
  get tHead() { return this.querySelector('thead'); }, get tFoot() { return this.querySelector('tfoot'); },
  insertRow(i) { const r = document.createElement('tr'); const b = this.querySelector('tbody') || this; const rows = b.querySelectorAll('tr'); if (i === undefined || i < 0 || i >= rows.length) b.appendChild(r); else b.insertBefore(r, rows[i]); return r; } });
mk('HTMLTableSectionElement', ['TBODY', 'THEAD', 'TFOOT'], [], { get rows() { return this.querySelectorAll('tr'); }, insertRow() { const r = document.createElement('tr'); this.appendChild(r); return r; } });
mk('HTMLTableRowElement', ['TR'], [], { get cells() { return this.children; }, get rowIndex() { const t = this.closest('table'); return t ? t.rows.indexOf(this) : -1; }, insertCell() { const c = document.createElement('td'); this.appendChild(c); return c; } });
mk('HTMLTableCellElement', ['TD', 'TH'], [['colSpan', 'colspan', 'n', 1], ['rowSpan', 'rowspan', 'n', 1], ['headers', 'headers']], { get cellIndex() { return this.parentElement ? [...this.parentElement.children].indexOf(this) : -1; } });
mk('HTMLDivElement', ['DIV'], [['align', 'align']]);
mk('HTMLSpanElement', ['SPAN'], []);
mk('HTMLParagraphElement', ['P'], []);
mk('HTMLHeadingElement', ['H1', 'H2', 'H3', 'H4', 'H5', 'H6'], []);
mk('HTMLUListElement', ['UL'], []);
mk('HTMLOListElement', ['OL'], [['start', 'start', 'n', 1], ['reversed', 'reversed', 'b'], ['type', 'type']]);
mk('HTMLLIElement', ['LI'], [['value', 'value', 'n', 0]]);
mk('HTMLDListElement', ['DL'], []);
mk('HTMLPreElement', ['PRE'], []);
mk('HTMLBRElement', ['BR'], []);
mk('HTMLHRElement', ['HR'], []);
mk('HTMLBodyElement', ['BODY'], []);
mk('HTMLHeadElement', ['HEAD'], []);
mk('HTMLHtmlElement', ['HTML'], []);
mk('HTMLQuoteElement', ['BLOCKQUOTE', 'Q'], [['cite', 'cite', 'u']]);
mk('HTMLDetailsElement', ['DETAILS'], [['open', 'open', 'b']]);
mk('HTMLDialogElement', ['DIALOG'], [['open', 'open', 'b']], {
  show() { this.setAttribute('open', ''); }, showModal() { this.setAttribute('open', ''); },
  close(r) { this.removeAttribute('open'); this.returnValue = r || ''; this.dispatchEvent(new Event('close')); } });
mk('HTMLPictureElement', ['PICTURE'], []);
mk('HTMLObjectElement', ['OBJECT'], [['data', 'data', 'u'], ['type', 'type']]);
mk('HTMLEmbedElement', ['EMBED'], [['src', 'src', 'u'], ['type', 'type']]);
mk('HTMLSlotElement', ['SLOT'], [['name', 'name']], { assignedNodes() { return []; }, assignedElements() { return []; } });
mk('HTMLDataElement', ['DATA'], [['value', 'value']]);
mk('HTMLTimeElement', ['TIME'], [['dateTime', 'datetime']]);
mk('HTMLProgressElement', ['PROGRESS'], [['value', 'value', 'n', 0], ['max', 'max', 'n', 1]]);
mk('HTMLOutputElement', ['OUTPUT'], [['name', 'name']], formOf);
mk('HTMLMapElement', ['MAP'], [['name', 'name']]);
mk('HTMLModElement', ['INS', 'DEL'], [['cite', 'cite', 'u']]);
mk('HTMLUnknownElement', [], []);
class SVGElement extends Element {
  get className() { return { baseVal: this.getAttribute('class') || '', animVal: this.getAttribute('class') || '' }; }
  set className(v) { this.setAttribute('class', v); }
  get ownerSVGElement() { return this.closest('svg'); }
  getBBox() { const r = this.getBoundingClientRect(); return { x: 0, y: 0, width: r.width, height: r.height }; }
}
for (const t of ['SVG', 'PATH', 'G', 'CIRCLE', 'RECT', 'LINE', 'POLYLINE', 'POLYGON', 'ELLIPSE', 'TEXT', 'TSPAN', 'USE', 'DEFS', 'SYMBOL', 'LINEARGRADIENT', 'STOP', 'CLIPPATH', 'MASK', 'TITLE'])
  if (!TAGS[t]) TAGS[t] = SVGElement;
TAGS['SVG'] = SVGElement;
const SVGSVGElement = SVGElement;

class DocumentFragment extends Node {
  getElementById(id) { return this.querySelector('#' + CSS.escape(id)); }
}
mixin(DocumentFragment, ParentNode);
class ShadowRoot extends DocumentFragment {}

// canvas 2d: aceita tudo e nao desenha
function makeCtx2d(canvas) {
  const target = { canvas, fillStyle: '#000', strokeStyle: '#000', lineWidth: 1, font: '10px sans-serif', globalAlpha: 1, textAlign: 'start', textBaseline: 'alphabetic',
    measureText(t) { return { width: String(t).length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2 }; },
    getImageData(x, y, w, h) { return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) }; },
    createImageData(w, h) { return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) }; },
    createLinearGradient() { return { addColorStop() {} }; }, createRadialGradient() { return { addColorStop() {} }; },
    createPattern() { return {}; }, isPointInPath() { return false; }, getTransform() { return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }; },
    getContextAttributes() { return {}; } };
  return new Proxy(target, { get(t, p) { if (p in t) return t[p]; return () => {}; }, set(t, p, v) { t[p] = v; return true; } });
}

// ---------------------------------------------------------------- document
let readyState = 'loading';
class Document extends Node {
  get documentElement() { for (let c = V.first(docId); c; c = V.next(c)) if (V.nodeType(c) === 1) return W(c); return null; }
  get head() { return this.querySelector('head'); }
  get body() { return this.querySelector('body'); }
  set body(b) { const o = this.body; if (o && b) o.replaceWith(b); }
  get title() { return V.title(); } set title(v) { V.setTitle(String(v)); }
  getElementById(id) { return W(V.byId(String(id))); }
  getElementsByName(n) { return this.querySelectorAll('[name="' + String(n).replace(/"/g, '\\"') + '"]'); }
  createElement(t) { return W(V.create(String(t))); }
  createElementNS(ns, t) { return W(V.create(String(t).replace(/^.*:/, ''))); }
  createTextNode(s) { return W(V.createText(String(s))); }
  createComment(s) { return W(V.createComment(String(s))); }
  createDocumentFragment() { return W(V.createFrag()); }
  createAttribute(n) { return { name: n, value: '' }; }
  createEvent(type) {
    const t = String(type).toLowerCase();
    const e = t.startsWith('mouse') ? new MouseEvent('') : t.startsWith('keyboard') ? new KeyboardEvent('') : t.startsWith('custom') ? new CustomEvent('') : new Event('');
    e.initMouseEvent = function (ty, b, c) { this.initEvent(ty, b, c); };
    e.initKeyboardEvent = e.initMouseEvent; e.initUIEvent = e.initMouseEvent;
    return e;
  }
  createRange() {
    return { selectNodeContents() {}, selectNode() {}, setStart() {}, setEnd() {}, setStartBefore() {}, setEndAfter() {}, collapse() {}, detach() {},
      createContextualFragment(h) { const f = document.createDocumentFragment(); const d = document.createElement('div'); d.innerHTML = h; while (d.firstChild) f.appendChild(d.firstChild); return f; },
      getBoundingClientRect() { return new DOMRect(); }, getClientRects() { return []; }, cloneRange() { return this; }, toString() { return ''; },
      deleteContents() {}, insertNode() {}, surroundContents() {}, commonAncestorContainer: document.body };
  }
  createTreeWalker(root, what = 0xFFFFFFFF, filter = null) {
    const ok = n => {
      const t = n.nodeType, bit = 1 << (t - 1);
      if (!(what & bit)) return false;
      if (!filter) return true;
      const f = typeof filter === 'function' ? filter : filter.acceptNode.bind(filter);
      return f(n) === 1;
    };
    const next = n => { if (n.firstChild) return n.firstChild; while (n && n !== root) { if (n.nextSibling) return n.nextSibling; n = n.parentNode; } return null; };
    return { root, currentNode: root, whatToShow: what, filter,
      nextNode() { let n = this.currentNode; while ((n = next(n))) if (ok(n)) return (this.currentNode = n); return null; },
      parentNode() { const p = this.currentNode.parentNode; if (p && p !== root.parentNode) return (this.currentNode = p); return null; },
      firstChild() { const c = this.currentNode.firstChild; return c ? (this.currentNode = c) : null; },
      nextSibling() { const c = this.currentNode.nextSibling; return c ? (this.currentNode = c) : null; } };
  }
  createNodeIterator(root, what, filter) { const w = this.createTreeWalker(root, what, filter); w.currentNode = root; let first = true; return { nextNode() { if (first) { first = false; return root; } return w.nextNode(); }, detach() {} }; }
  get readyState() { return readyState; }
  get cookie() { return V.cookie(); } set cookie(v) { V.setCookie(String(v)); }
  get location() { return location; } set location(v) { location.href = v; }
  get URL() { return V.url(); } get documentURI() { return V.url(); }
  get domain() { return location.hostname; } set domain(v) {}
  get referrer() { return ''; }
  get defaultView() { return G; }
  get activeElement() { return W(V.focused()) || this.body; }
  hasFocus() { return true; }
  get hidden() { return false; } get visibilityState() { return 'visible'; }
  get currentScript() { return W(V.currentScript()); }
  get characterSet() { return 'UTF-8'; } get charset() { return 'UTF-8'; } get inputEncoding() { return 'UTF-8'; }
  get compatMode() { return 'CSS1Compat'; } get contentType() { return 'text/html'; }
  get doctype() { return { name: 'html', publicId: '', systemId: '', nodeType: 10 }; }
  get forms() { return this.querySelectorAll('form'); } get images() { return this.querySelectorAll('img'); }
  get links() { return this.querySelectorAll('a[href],area[href]'); } get scripts() { return this.querySelectorAll('script'); }
  get styleSheets() { return list([]); }
  get scrollingElement() { return this.documentElement; }
  get fullscreenElement() { return null; } get fullscreenEnabled() { return false; } get pointerLockElement() { return null; }
  get implementation() {
    return { hasFeature() { return true; },
      createHTMLDocument(t) {
        const html = document.createElement('html'), head = document.createElement('head'), body = document.createElement('body');
        html.append(head, body);
        return { documentElement: html, head, body, title: t || '', nodeType: 9,
          createElement: t2 => document.createElement(t2), createTextNode: s => document.createTextNode(s),
          createDocumentFragment: () => document.createDocumentFragment(), createComment: s => document.createComment(s),
          querySelector: s => html.querySelector(s), querySelectorAll: s => html.querySelectorAll(s),
          getElementById: id => html.querySelector('#' + CSS.escape(id)), implementation: document.implementation, addEventListener() {} };
      },
      createDocumentType() { return {}; } };
  }
  write(...a) { V.write(a.join('')); }
  writeln(...a) { V.write(a.join('') + '\n'); }
  open() { return this; } close() {}
  elementFromPoint(x, y) { return W(V.hit(x | 0, y | 0)); }
  elementsFromPoint(x, y) { const e = this.elementFromPoint(x, y); const a = []; for (let n = e; n; n = n.parentElement) a.push(n); return a; }
  getSelection() { return G.getSelection(); }
  execCommand() { return false; } queryCommandSupported() { return false; } queryCommandEnabled() { return false; }
  get fonts() { return { ready: Promise.resolve(), status: 'loaded', load() { return Promise.resolve([]); }, add() {}, check() { return true; }, addEventListener() {}, forEach() {} }; }
  adoptNode(n) { return n; } importNode(n, deep) { return n.cloneNode(deep); }
  exitFullscreen() { return Promise.resolve(); }
  startViewTransition(cb) { if (cb) cb(); const p = Promise.resolve(); return { finished: p, ready: p, updateCallbackDone: p, skipTransition() {} }; }
}
mixin(Document, ParentNode);
const document = new Document(docId);
cache.set(docId, document);
class HTMLDocument extends Document {}

// --------------------------------------------------------------- URL
const URL_RE = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(?:\/\/(?:([^\/?#@]*)@)?(\[[^\]]*\]|[^\/?#:]*)(?::(\d*))?)?([^?#]*)(\?[^#]*)?(#.*)?$/;
class URLSearchParams {
  constructor(init = '') {
    this._l = [];
    if (init instanceof URLSearchParams) this._l = init._l.map(x => x.slice());
    else if (Array.isArray(init)) for (const [k, v] of init) this._l.push([String(k), String(v)]);
    else if (typeof init === 'object' && init !== null) for (const k of Object.keys(init)) this._l.push([k, String(init[k])]);
    else {
      let s = String(init); if (s[0] === '?') s = s.slice(1);
      for (const p of s.split('&')) {
        if (!p) continue;
        const i = p.indexOf('=');
        const dec = x => { try { return decodeURIComponent(x.replace(/\+/g, ' ')); } catch (e) { return x; } };
        this._l.push(i < 0 ? [dec(p), ''] : [dec(p.slice(0, i)), dec(p.slice(i + 1))]);
      }
    }
  }
  _upd() { if (this._u) this._u._search = this._l.length ? '?' + this.toString() : ''; }
  get(k) { const e = this._l.find(x => x[0] === k); return e ? e[1] : null; }
  getAll(k) { return this._l.filter(x => x[0] === k).map(x => x[1]); }
  has(k) { return this._l.some(x => x[0] === k); }
  set(k, v) { const i = this._l.findIndex(x => x[0] === k); if (i < 0) this._l.push([k, String(v)]); else { this._l[i][1] = String(v); this._l = this._l.filter((x, j) => x[0] !== k || j === i); } this._upd(); }
  append(k, v) { this._l.push([String(k), String(v)]); this._upd(); }
  delete(k) { this._l = this._l.filter(x => x[0] !== k); this._upd(); }
  sort() { this._l.sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0); this._upd(); }
  forEach(f, t) { for (const [k, v] of this._l) f.call(t, v, k, this); }
  keys() { return this._l.map(x => x[0])[Symbol.iterator](); }
  values() { return this._l.map(x => x[1])[Symbol.iterator](); }
  entries() { return this._l.map(x => [x[0], x[1]])[Symbol.iterator](); }
  [Symbol.iterator]() { return this.entries(); }
  get size() { return this._l.length; }
  toString() { const e = s => encodeURIComponent(s).replace(/%20/g, '+'); return this._l.map(([k, v]) => e(k) + '=' + e(v)).join('&'); }
}
class URL {
  constructor(u, base) {
    u = String(u).trim();
    let abs;
    if (base !== undefined) abs = V.resolve(String(base instanceof URL ? base.href : base), u);
    else if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(u)) abs = u;
    if (!abs || !URL_RE.test(abs)) throw new TypeError('Invalid URL: ' + u);
    this._set(abs);
  }
  _set(abs) {
    const m = URL_RE.exec(abs);
    this._protocol = m[1].toLowerCase(); this._auth = m[2] || ''; this._hostname = (m[3] || '').toLowerCase();
    this._port = m[4] || ''; this._pathname = m[5] || (m[3] !== undefined ? '/' : '');
    this._search = m[6] && m[6] !== '?' ? m[6] : ''; this._hash = m[7] && m[7] !== '#' ? m[7] : '';
    if ((this._protocol === 'https:' && this._port === '443') || (this._protocol === 'http:' && this._port === '80')) this._port = '';
    this._sp = null;
  }
  get href() { const auth = this._auth ? this._auth + '@' : ''; const slash = this._hostname || this._protocol === 'file:' || /^https?:$/.test(this._protocol) ? '//' : ''; return this._protocol + slash + auth + this.host + this._pathname + this._search + this._hash; }
  set href(v) { this._set(String(v)); }
  get protocol() { return this._protocol; } set protocol(v) { this._protocol = String(v).replace(/:?$/, ':'); }
  get username() { return this._auth.split(':')[0] || ''; } get password() { return this._auth.split(':')[1] || ''; }
  get host() { return this._hostname + (this._port ? ':' + this._port : ''); }
  set host(v) { const [h, p] = String(v).split(':'); this._hostname = h; this._port = p || ''; }
  get hostname() { return this._hostname; } set hostname(v) { this._hostname = String(v); }
  get port() { return this._port; } set port(v) { this._port = String(v); }
  get pathname() { return this._pathname; } set pathname(v) { v = String(v); this._pathname = v[0] === '/' ? v : '/' + v; }
  get search() { return this._search; } set search(v) { v = String(v); this._search = v && v !== '?' ? (v[0] === '?' ? v : '?' + v) : ''; this._sp = null; }
  get hash() { return this._hash; } set hash(v) { v = String(v); this._hash = v && v !== '#' ? (v[0] === '#' ? v : '#' + v) : ''; }
  get origin() { return /^https?:$/.test(this._protocol) ? this._protocol + '//' + this.host : 'null'; }
  get searchParams() { if (!this._sp) { this._sp = new URLSearchParams(this._search); this._sp._u = this; } return this._sp; }
  toString() { return this.href; } toJSON() { return this.href; }
  static createObjectURL() { return 'blob:null/0'; } static revokeObjectURL() {}
  static canParse(u, b) { try { new URL(u, b); return true; } catch (e) { return false; } }
}

// ----------------------------------------------------------- location etc.
function navigateTo(v) { V.navigate(String(v)); }
const location = {
  get href() { return V.url(); }, set href(v) { navigateTo(v); },
  assign(v) { navigateTo(v); }, replace(v) { navigateTo(v); }, reload() { navigateTo(V.url()); },
  toString() { return V.url(); }, valueOf() { return V.url(); },
  get protocol() { return new URL(V.url()).protocol; }, get host() { return new URL(V.url()).host; },
  get hostname() { return new URL(V.url()).hostname; }, get port() { return new URL(V.url()).port; },
  get pathname() { return new URL(V.url()).pathname; }, set pathname(v) { const u = new URL(V.url()); u.pathname = v; navigateTo(u.href); },
  get search() { return new URL(V.url()).search; }, set search(v) { const u = new URL(V.url()); u.search = v; navigateTo(u.href); },
  get hash() { return new URL(V.url()).hash; },
  set hash(v) { const u = new URL(V.url()); const old = u.href; u.hash = v; V.setUrl(u.href); const el = document.getElementById(String(v).replace(/^#/, '')); if (el) el.scrollIntoView(); G.dispatchEvent(new HashChangeEvent('hashchange', { oldURL: old, newURL: u.href })); },
  get origin() { return new URL(V.url()).origin; },
  get ancestorOrigins() { return list([]); },
};
const histStates = [null];
const history = {
  get length() { return histStates.length; }, get state() { return histStates[histStates.length - 1]; }, scrollRestoration: 'auto',
  pushState(s, t, u) { histStates.push(s); if (u != null) { const r = V.resolve(V.url(), String(u)); if (r) V.setUrl(r); } },
  replaceState(s, t, u) { histStates[histStates.length - 1] = s; if (u != null) { const r = V.resolve(V.url(), String(u)); if (r) V.setUrl(r); } },
  back() { V.back(); }, forward() {}, go(n) { if (n < 0) V.back(); },
};
const navigator = {
  userAgent: 'Mozilla/5.0 (Pulsar OS) Vela/1.0', appName: 'Netscape', appCodeName: 'Mozilla', appVersion: '5.0 (Pulsar OS)',
  platform: 'Pulsar', product: 'Gecko', productSub: '20030107', vendor: '', vendorSub: '', language: 'pt-BR', languages: ['pt-BR', 'pt', 'en'],
  cookieEnabled: true, onLine: true, doNotTrack: null, hardwareConcurrency: 4, maxTouchPoints: 0, webdriver: false, pdfViewerEnabled: false,
  deviceMemory: 1, plugins: list([]), mimeTypes: list([]),
  javaEnabled() { return false; }, sendBeacon() { return true; }, vibrate() { return false; },
  clipboard: { writeText() { return Promise.resolve(); }, readText() { return Promise.resolve(''); } },
  permissions: { query() { return Promise.resolve({ state: 'denied', addEventListener() {} }); } },
  connection: { effectiveType: '4g', downlink: 10, rtt: 50, saveData: false, addEventListener() {} },
  storage: { estimate() { return Promise.resolve({ quota: 0, usage: 0 }); }, persist() { return Promise.resolve(false); } },
  locks: { request(n, o, cb) { return Promise.resolve((typeof o === 'function' ? o : cb)({ name: n })); } },
};
const screen = { width: 1280, height: 800, availWidth: 1280, availHeight: 800, colorDepth: 24, pixelDepth: 24,
  orientation: { type: 'landscape-primary', angle: 0, addEventListener() {}, removeEventListener() {} } };

// --------------------------------------------------------------- timers
let timerSeq = 1;
const timers = new Map();
function setTimeout(fn, ms, ...args) { const id = timerSeq++; timers.set(id, { fn, args, due: V.now() + Math.max(0, +ms || 0), every: 0 }); return id; }
function setInterval(fn, ms, ...args) { const id = timerSeq++; const e = Math.max(10, +ms || 0); timers.set(id, { fn, args, due: V.now() + e, every: e }); return id; }
function clearTimeout(id) { timers.delete(id); }
function requestAnimationFrame(cb) { return setTimeout(() => cb(V.now()), 16); }
function requestIdleCallback(cb) { return setTimeout(() => cb({ didTimeout: false, timeRemaining: () => 10 }), 1); }
function queueMicrotask(cb) { Promise.resolve().then(cb); }

// ------------------------------------------------------------ armazenamento
class Storage {
  constructor() { Object.defineProperty(this, '_m', { value: new Map() }); }
  get length() { return this._m.size; }
  key(i) { return [...this._m.keys()][i] ?? null; }
  getItem(k) { return this._m.has(String(k)) ? this._m.get(String(k)) : null; }
  setItem(k, v) { this._m.set(String(k), String(v)); }
  removeItem(k) { this._m.delete(String(k)); }
  clear() { this._m.clear(); }
}
const localStorage = new Storage(), sessionStorage = new Storage();

// ---------------------------------------------------------- texto/base64
const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function btoa(s) {
  s = String(s); let o = '';
  for (let i = 0; i < s.length; i += 3) {
    const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
    if (a > 255 || b > 255 || c > 255) throw new DOMException('Invalid character', 'InvalidCharacterError');
    const n = (a << 16) | ((b || 0) << 8) | (c || 0);
    o += B64[n >> 18] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : '=') + (i + 2 < s.length ? B64[n & 63] : '=');
  }
  return o;
}
function atob(s) {
  s = String(s).replace(/[\s=]/g, ''); let o = '', buf = 0, bits = 0;
  for (const ch of s) { const v = B64.indexOf(ch === '-' ? '+' : ch === '_' ? '/' : ch); if (v < 0) throw new DOMException('Invalid character', 'InvalidCharacterError'); buf = (buf << 6) | v; bits += 6; if (bits >= 8) { bits -= 8; o += String.fromCharCode((buf >> bits) & 255); } }
  return o;
}
class TextEncoder {
  get encoding() { return 'utf-8'; }
  encode(s = '') { const u = unescape(encodeURIComponent(String(s))); const a = new Uint8Array(u.length); for (let i = 0; i < u.length; i++) a[i] = u.charCodeAt(i); return a; }
}
class TextDecoder {
  constructor(enc = 'utf-8') { this.encoding = enc; }
  decode(b) { if (!b) return ''; const a = b instanceof ArrayBuffer ? new Uint8Array(b) : new Uint8Array(b.buffer || b, b.byteOffset || 0, b.byteLength); let s = ''; for (let i = 0; i < a.length; i++) s += String.fromCharCode(a[i]); try { return decodeURIComponent(escape(s)); } catch (e) { return s; } }
}

// ------------------------------------------------------------- rede (JS)
class Headers {
  constructor(init) { this._m = new Map(); if (init) { if (init instanceof Headers) init.forEach((v, k) => this.append(k, v)); else if (Array.isArray(init)) for (const [k, v] of init) this.append(k, v); else for (const k of Object.keys(init)) this.append(k, init[k]); } }
  append(k, v) { k = String(k).toLowerCase(); this._m.set(k, this._m.has(k) ? this._m.get(k) + ', ' + v : String(v)); }
  set(k, v) { this._m.set(String(k).toLowerCase(), String(v)); }
  get(k) { const v = this._m.get(String(k).toLowerCase()); return v === undefined ? null : v; }
  has(k) { return this._m.has(String(k).toLowerCase()); }
  delete(k) { this._m.delete(String(k).toLowerCase()); }
  forEach(f, t) { for (const [k, v] of this._m) f.call(t, v, k, this); }
  entries() { return this._m.entries(); } keys() { return this._m.keys(); } values() { return this._m.values(); }
  [Symbol.iterator]() { return this._m.entries(); }
}
class Response {
  constructor(body = '', o = {}) {
    this._body = body == null ? '' : String(body); this.status = o.status === undefined ? 200 : o.status;
    this.statusText = o.statusText || ''; this.headers = new Headers(o.headers); this.url = o.url || '';
    this.ok = this.status >= 200 && this.status < 300; this.redirected = false; this.type = 'basic'; this.bodyUsed = false;
  }
  text() { this.bodyUsed = true; return Promise.resolve(this._body); }
  json() { this.bodyUsed = true; return new Promise((r, j) => { try { r(JSON.parse(this._body)); } catch (e) { j(e); } }); }
  arrayBuffer() { return Promise.resolve(new TextEncoder().encode(this._body).buffer); }
  blob() { return Promise.resolve(new Blob([this._body], { type: this.headers.get('content-type') || '' })); }
  formData() { return Promise.resolve(new FormData()); }
  clone() { return new Response(this._body, { status: this.status, headers: this.headers, url: this.url }); }
  get body() { return null; }
  static json(d, o) { return new Response(JSON.stringify(d), o); }
  static error() { return new Response('', { status: 0 }); }
}
class Request {
  constructor(input, o = {}) { this.url = input instanceof Request ? input.url : new URL(String(input), V.url()).href; this.method = (o.method || 'GET').toUpperCase(); this.headers = new Headers(o.headers); this.body = o.body || null; this.signal = o.signal || null; this.credentials = o.credentials || 'same-origin'; this.mode = o.mode || 'cors'; }
  clone() { return new Request(this, this); }
}
class AbortSignal extends EventTarget {
  constructor() { super(); this.aborted = false; this.reason = undefined; this.onabort = null; }
  throwIfAborted() { if (this.aborted) throw this.reason; }
  static timeout(ms) { const c = new AbortController(); setTimeout(() => c.abort(new DOMException('timeout', 'TimeoutError')), ms); return c.signal; }
  static abort(r) { const c = new AbortController(); c.abort(r); return c.signal; }
  static any(sigs) { const c = new AbortController(); for (const s of sigs) s.addEventListener('abort', () => c.abort(s.reason)); return c.signal; }
}
class AbortController {
  constructor() { this.signal = new AbortSignal(); }
  abort(r) { if (this.signal.aborted) return; this.signal.aborted = true; this.signal.reason = r === undefined ? new DOMException('aborted', 'AbortError') : r; this.signal.dispatchEvent(new Event('abort')); }
}
function bodyToString(b) {
  if (b == null) return null;
  if (typeof b === 'string') return b;
  if (b instanceof URLSearchParams) return b.toString();
  if (b instanceof FormData) return b._urlencoded();
  if (b instanceof Blob) return b._t;
  return String(b);
}
function doFetch(url, method, body, ctype) {
  const r = V.fetch(url, method, body, ctype);
  if (!r) throw new TypeError('Failed to fetch');
  return r;
}
function fetch(input, init = {}) {
  return new Promise((resolve, reject) => {
    setTimeout(() => {
      try {
        const req = input instanceof Request ? input : null;
        const url = new URL(req ? req.url : String(input), V.url()).href;
        const method = String(init.method || (req && req.method) || 'GET').toUpperCase();
        const signal = init.signal || (req && req.signal);
        if (signal && signal.aborted) { reject(signal.reason); return; }
        const h = new Headers(init.headers || (req && req.headers));
        const raw = init.body !== undefined ? init.body : req && req.body;
        let ctype = h.get('content-type');
        if (!ctype) ctype = raw instanceof URLSearchParams || raw instanceof FormData ? 'application/x-www-form-urlencoded' : 'text/plain;charset=UTF-8';
        const r = doFetch(url, method, bodyToString(raw), ctype);
        resolve(new Response(r[1], { status: r[0], url: r[2], headers: { 'content-type': r[3] } }));
      } catch (e) { reject(e); }
    }, 0);
  });
}
class XMLHttpRequest extends EventTarget {
  constructor() { super(); this.readyState = 0; this.status = 0; this.statusText = ''; this.responseText = ''; this.response = ''; this.responseType = ''; this.responseURL = ''; this.timeout = 0; this.withCredentials = false; this._h = {}; this._rh = ''; this.upload = new EventTarget(); }
  open(m, u, async = true) { this._m = String(m).toUpperCase(); this._u = new URL(String(u), V.url()).href; this._async = async !== false; this.readyState = 1; this._fire('readystatechange'); }
  setRequestHeader(k, v) { this._h[String(k).toLowerCase()] = String(v); }
  getResponseHeader(k) { return String(k).toLowerCase() === 'content-type' ? this._rh : null; }
  getAllResponseHeaders() { return this._rh ? 'content-type: ' + this._rh + '\r\n' : ''; }
  overrideMimeType() {}
  abort() { this._aborted = true; }
  _fire(t) { this.dispatchEvent(new ProgressEvent(t)); }
  send(body) {
    const run = () => {
      if (this._aborted) return;
      try {
        const r = doFetch(this._u, this._m, bodyToString(body === undefined ? null : body), this._h['content-type'] || 'text/plain;charset=UTF-8');
        this.status = r[0]; this.statusText = r[0] === 200 ? 'OK' : ''; this.responseText = r[1]; this.responseURL = r[2]; this._rh = r[3];
        if (this.responseType === 'json') { try { this.response = JSON.parse(r[1]); } catch (e) { this.response = null; } }
        else if (this.responseType === 'document') { this.response = new DOMParser().parseFromString(r[1], 'text/html'); }
        else this.response = r[1];
        this.readyState = 4;
        this._fire('readystatechange'); this._fire('load'); this._fire('loadend');
      } catch (e) {
        this.readyState = 4; this.status = 0;
        this._fire('readystatechange'); this._fire('error'); this._fire('loadend');
      }
    };
    if (this._async) setTimeout(run, 0); else run();
  }
}
XMLHttpRequest.UNSENT = 0; XMLHttpRequest.OPENED = 1; XMLHttpRequest.HEADERS_RECEIVED = 2; XMLHttpRequest.LOADING = 3; XMLHttpRequest.DONE = 4;
class Blob {
  constructor(parts = [], o = {}) { this._t = parts.map(p => p instanceof Blob ? p._t : String(p)).join(''); this.type = o.type || ''; }
  get size() { return this._t.length; }
  text() { return Promise.resolve(this._t); }
  slice(a, b, t) { return new Blob([this._t.slice(a, b)], { type: t }); }
  arrayBuffer() { return Promise.resolve(new TextEncoder().encode(this._t).buffer); }
}
class File extends Blob { constructor(parts, name, o) { super(parts, o); this.name = name; this.lastModified = Date.now(); } }
class FileReader extends EventTarget {
  readAsText(b) { setTimeout(() => { this.result = b._t; this.dispatchEvent(new Event('load')); if (this.onload) this.onload({ target: this }); }, 0); }
  readAsDataURL(b) { setTimeout(() => { this.result = 'data:' + b.type + ';base64,' + btoa(unescape(encodeURIComponent(b._t))); this.dispatchEvent(new Event('load')); if (this.onload) this.onload({ target: this }); }, 0); }
  abort() {}
}
class FormData {
  constructor(form) {
    this._l = [];
    if (form) for (const e of form.elements) {
      const n = e.getAttribute('name');
      if (!n || e.disabled) continue;
      const t = e.type;
      if ((t === 'checkbox' || t === 'radio') && !e.checked) continue;
      if (t === 'submit' || t === 'button' || t === 'reset' || t === 'file') continue;
      this._l.push([n, e.value]);
    }
  }
  append(k, v) { this._l.push([String(k), v instanceof Blob ? v : String(v)]); }
  set(k, v) { this.delete(k); this.append(k, v); }
  get(k) { const e = this._l.find(x => x[0] === k); return e ? e[1] : null; }
  getAll(k) { return this._l.filter(x => x[0] === k).map(x => x[1]); }
  has(k) { return this._l.some(x => x[0] === k); }
  delete(k) { this._l = this._l.filter(x => x[0] !== k); }
  forEach(f, t) { for (const [k, v] of this._l) f.call(t, v, k, this); }
  entries() { return this._l.map(x => x.slice())[Symbol.iterator](); }
  keys() { return this._l.map(x => x[0])[Symbol.iterator](); } values() { return this._l.map(x => x[1])[Symbol.iterator](); }
  [Symbol.iterator]() { return this.entries(); }
  _urlencoded() { return new URLSearchParams(this._l.map(([k, v]) => [k, typeof v === 'string' ? v : ''])).toString(); }
}
class DOMParser {
  parseFromString(s, type) {
    const div = document.createElement('div');
    div.innerHTML = String(s);
    const html = document.createElement('html'), head = document.createElement('head'), body = document.createElement('body');
    html.append(head, body);
    while (div.firstChild) body.appendChild(div.firstChild);
    return { documentElement: html, head, body, nodeType: 9, title: '',
      querySelector: q => html.querySelector(q), querySelectorAll: q => html.querySelectorAll(q),
      getElementById: id => html.querySelector('#' + CSS.escape(id)), getElementsByTagName: t => html.getElementsByTagName(t),
      createElement: t => document.createElement(t) };
  }
}
class XMLSerializer { serializeToString(n) { return n.outerHTML || n.textContent || ''; } }

// ------------------------------------------------------------ observadores
class MutationObserver { constructor(cb) { this._cb = cb; } observe() {} disconnect() {} takeRecords() { return []; } }
class IntersectionObserver {
  constructor(cb, o = {}) { this._cb = cb; this._els = new Set(); this.root = o.root || null; this.rootMargin = o.rootMargin || '0px'; this.thresholds = [].concat(o.threshold || 0); }
  observe(el) {
    if (this._els.has(el)) return;
    this._els.add(el);
    // sem rolagem real de viewport: todo elemento observado conta como visivel
    setTimeout(() => {
      if (!this._els.has(el)) return;
      const r = el.getBoundingClientRect();
      try { this._cb([{ target: el, isIntersecting: true, intersectionRatio: 1, boundingClientRect: r, intersectionRect: r, rootBounds: new DOMRect(0, 0, G.innerWidth, G.innerHeight), time: V.now() }], this); }
      catch (e) { reportError(e); }
    }, 0);
  }
  unobserve(el) { this._els.delete(el); }
  disconnect() { this._els.clear(); }
  takeRecords() { return []; }
}
class ResizeObserver {
  constructor(cb) { this._cb = cb; }
  observe(el) { setTimeout(() => { const r = el.getBoundingClientRect(); try { this._cb([{ target: el, contentRect: r, borderBoxSize: [{ inlineSize: r.width, blockSize: r.height }], contentBoxSize: [{ inlineSize: r.width, blockSize: r.height }] }], this); } catch (e) { reportError(e); } }, 0); }
  unobserve() {} disconnect() {}
}
class PerformanceObserver { constructor() {} observe() {} disconnect() {} static get supportedEntryTypes() { return []; } }
const customElements = {
  define(n, c) {
    n = String(n).toLowerCase();
    if (customRegistry.has(n)) throw new DOMException("'" + n + "' ja foi definido", 'NotSupportedError');
    customRegistry.set(n, c); customNames.set(c, n);
    for (const id of V.qsa(docId, n, false) || []) {
      const old = cache.get(id);
      const el = upgrade(id, c, old);
      if (el.isConnected) connectOne(el);
    }
    const w = waiting.get(n); if (w) { waiting.delete(n); w.forEach(r => r(c)); }
  },
  get(n) { return customRegistry.get(String(n).toLowerCase()); },
  getName(c) { return customNames.get(c) || null; },
  whenDefined(n) { n = String(n).toLowerCase(); const c = customRegistry.get(n); if (c) return Promise.resolve(c); return new Promise(r => { if (!waiting.has(n)) waiting.set(n, []); waiting.get(n).push(r); }); },
  upgrade(root) { if (root && root.querySelectorAll) for (const e of root.querySelectorAll('*')) void e; },
};

// ------------------------------------------------------------- window
const console = {};
for (const [k, lvl] of [['log', 0], ['info', 0], ['debug', 0], ['trace', 0], ['dir', 0], ['table', 0], ['warn', 1], ['error', 2]])
  console[k] = (...a) => V.log(lvl, a.map(x => { try { return typeof x === 'string' ? x : x instanceof Error ? x.message : JSON.stringify(x); } catch (e) { return String(x); } }).join(' '));
for (const k of ['group', 'groupCollapsed', 'groupEnd', 'time', 'timeEnd', 'timeLog', 'count', 'countReset', 'clear', 'profile', 'profileEnd', 'timeStamp'])
  console[k] = () => {};
console.assert = (c, ...a) => { if (!c) console.error('Assertion failed:', ...a); };

const timeOrigin = Date.now() - V.now();
const performance = {
  now() { return V.now(); }, timeOrigin,
  mark() {}, measure() {}, clearMarks() {}, clearMeasures() {}, clearResourceTimings() {}, setResourceTimingBufferSize() {},
  getEntries() { return []; }, getEntriesByType() { return []; }, getEntriesByName() { return []; },
  timing: { navigationStart: timeOrigin, domContentLoadedEventEnd: 0, loadEventEnd: 0, responseStart: timeOrigin },
  navigation: { type: 0, redirectCount: 0 }, toJSON() { return {}; }, addEventListener() {},
};
const crypto = {
  getRandomValues(a) {
    const bytes = V.random(a.byteLength || a.length);
    const u8 = new Uint8Array(a.buffer, a.byteOffset || 0, a.byteLength);
    for (let i = 0; i < u8.length; i++) u8[i] = bytes[i] || 0;
    return a;
  },
  randomUUID() { const b = crypto.getRandomValues(new Uint8Array(16)); b[6] = (b[6] & 15) | 64; b[8] = (b[8] & 63) | 128; const h = [...b].map(x => x.toString(16).padStart(2, '0')).join(''); return h.slice(0, 8) + '-' + h.slice(8, 12) + '-' + h.slice(12, 16) + '-' + h.slice(16, 20) + '-' + h.slice(20); },
  subtle: {},
};
const CSS = {
  supports() { return false; },
  escape(s) { return String(s).replace(/(^-?\d)|[^\w-]/g, (m, d) => d ? '\\3' + d.slice(-1) + ' ' + (d.length > 1 ? '' : '') : '\\' + m); },
};
function getComputedStyle(el) {
  const id = el && el._id;
  const api = { getPropertyValue(p) { return id ? V.computed(id, kebab(p)) : ''; }, getPropertyPriority() { return ''; }, get length() { return 0; }, item() { return ''; } };
  return new Proxy(api, { get(t, p) { if (p in t) return t[p]; if (typeof p !== 'string') return undefined; return t.getPropertyValue(p); } });
}
function matchMedia(q) {
  const ml = { matches: V.media(String(q)), media: String(q), onchange: null,
    addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent() { return true; } };
  return ml;
}
function structuredClone(v) { return v === undefined ? undefined : JSON.parse(JSON.stringify(v)); }

class Window extends EventTarget {}
const winProps = {
  window: G, self: G, top: G, parent: G, frames: G, globalThis: G, document, location, history, navigator, screen,
  console, performance, crypto, CSS, localStorage, sessionStorage, customElements,
  Node, Element, HTMLElement, CharacterData, Text, Comment, Document, HTMLDocument, DocumentFragment, ShadowRoot, SVGElement, SVGSVGElement,
  EventTarget, Event, CustomEvent, UIEvent, MouseEvent, PointerEvent, WheelEvent, KeyboardEvent, FocusEvent, InputEvent, SubmitEvent,
  ErrorEvent, PopStateEvent, HashChangeEvent, MessageEvent, ProgressEvent, TouchEvent, AnimationEvent, TransitionEvent, StorageEvent, PageTransitionEvent,
  DOMException, DOMRect, DOMRectReadOnly: DOMRect, DOMTokenList, URL, URLSearchParams, Headers, Request, Response, AbortController, AbortSignal,
  XMLHttpRequest, Blob, File, FileReader, FormData, DOMParser, XMLSerializer, TextEncoder, TextDecoder, Storage,
  MutationObserver, WebKitMutationObserver: MutationObserver, IntersectionObserver, ResizeObserver, PerformanceObserver, Window,
  setTimeout, setInterval, clearTimeout, clearInterval: clearTimeout, requestAnimationFrame, cancelAnimationFrame: clearTimeout,
  requestIdleCallback, cancelIdleCallback: clearTimeout, queueMicrotask, structuredClone, btoa, atob, fetch, getComputedStyle, matchMedia,
  reportError,
  Image: function Image(w, h) { const e = document.createElement('img'); if (w !== undefined) e.setAttribute('width', w); if (h !== undefined) e.setAttribute('height', h); return e; },
  Audio: function Audio(src) { const e = document.createElement('audio'); if (src) e.setAttribute('src', src); return e; },
  Option: function Option(t, v, d, s) { const e = document.createElement('option'); if (t !== undefined) e.textContent = t; if (v !== undefined) e.setAttribute('value', v); if (s) e.setAttribute('selected', ''); return e; },
  alert(m) { V.alert(m === undefined ? '' : String(m)); },
  confirm(m) { V.alert(String(m)); return true; },
  prompt() { return null; },
  open(u) { if (u) navigateTo(u); return null; },
  close() {}, focus() {}, blur() {}, print() {}, stop() {}, moveTo() {}, resizeTo() {},
  postMessage(data, origin) { setTimeout(() => G.dispatchEvent(new MessageEvent('message', { data, origin: location.origin, source: G })), 0); },
  scrollTo(x, y) { if (typeof x === 'object' && x) y = x.top; V.scrollTo(+y || 0); },
  scroll(x, y) { G.scrollTo(x, y); },
  scrollBy(x, y) { if (typeof x === 'object' && x) y = x.top; V.scrollTo(V.viewport()[2] + (+y || 0)); },
  getSelection() { return { rangeCount: 0, isCollapsed: true, type: 'None', removeAllRanges() {}, addRange() {}, getRangeAt() { return document.createRange(); }, collapse() {}, toString() { return ''; } }; },
  name: '', status: '', closed: false, opener: null, devicePixelRatio: 1, isSecureContext: V.url().startsWith('https:'),
  origin: (() => { try { return new URL(V.url()).origin; } catch (e) { return 'null'; } })(),
  visualViewport: undefined, speechSynthesis: undefined,
  onerror: null, onload: null, onunload: null, onbeforeunload: null, onresize: null, onscroll: null, onmessage: null,
  onpopstate: null, onhashchange: null, onfocus: null, onblur: null, onpageshow: null, onDOMContentLoaded: null,
};
for (const k of Object.keys(winProps)) {
  Object.defineProperty(G, k, { value: winProps[k], writable: true, configurable: true, enumerable: false });
}
for (const [k, get] of [['innerWidth', () => V.viewport()[0]], ['innerHeight', () => V.viewport()[1]],
  ['outerWidth', () => V.viewport()[0]], ['outerHeight', () => V.viewport()[1] + 90],
  ['scrollX', () => 0], ['scrollY', () => V.viewport()[2]], ['pageXOffset', () => 0], ['pageYOffset', () => V.viewport()[2]],
  ['screenX', () => 0], ['screenY', () => 0], ['screenLeft', () => 0], ['screenTop', () => 0]])
  Object.defineProperty(G, k, { get, configurable: true });
Object.defineProperty(G, 'event', { value: undefined, writable: true, configurable: true });
// window e um EventTarget
Object.setPrototypeOf(G, Window.prototype);
// chamadas sem receptor (addEventListener('load', f)) usam o window
for (const k of ['addEventListener', 'removeEventListener', 'dispatchEvent'])
  Object.defineProperty(G, k, { value: EventTarget.prototype[k].bind(G), writable: true, configurable: true });

// ------------------------------------------------- pontos de entrada (C)
G.__vela_tick = function (now) {
  let ran = 0;
  for (const [id, t] of [...timers]) {
    if (t.due > now || !timers.has(id)) continue;
    if (t.every) t.due = now + t.every; else timers.delete(id);
    try { if (typeof t.fn === 'function') t.fn.apply(G, t.args); else (0, eval)(String(t.fn)); }
    catch (e) { reportError(e); }
    if (++ran > 500) break;
  }
  let next = -1;
  for (const t of timers.values()) if (next < 0 || t.due < next) next = t.due;
  return next < 0 ? -1 : Math.max(0, Math.min(60000, next - V.now()));
};
G.__vela_dispatch = function (id, type, x, y) {
  const t = W(id);
  if (!t) return false;
  const mouse = type === 'click' || type === 'dblclick' || type === 'contextmenu' || type.startsWith('mouse') || type.startsWith('pointer');
  const ev = mouse ? (type.startsWith('pointer') ? new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y })
                                                 : new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y }))
    : type === 'submit' ? new SubmitEvent('submit', { bubbles: true, cancelable: true })
    : new Event(type, { bubbles: type !== 'focus' && type !== 'blur', cancelable: true });
  ev.isTrusted = true;
  t.dispatchEvent(ev);
  return ev.defaultPrevented;
};
G.__vela_fire = function (id, type) { const t = W(id); if (t) t.dispatchEvent(new Event(type)); };
G.__vela_key = function (id, key, code) {
  const t = W(id) || document.body || document;
  const o = { bubbles: true, cancelable: true, key, keyCode: code, code: key.length === 1 ? 'Key' + key.toUpperCase() : key };
  const ev = new KeyboardEvent('keydown', o); ev.isTrusted = true;
  t.dispatchEvent(ev);
  if (!ev.defaultPrevented && key.length === 1) t.dispatchEvent(new KeyboardEvent('keypress', o));
  t.dispatchEvent(new KeyboardEvent('keyup', o));
  return ev.defaultPrevented;
};
G.__vela_loaded = function () {
  readyState = 'interactive';
  document.dispatchEvent(new Event('readystatechange'));
  document.dispatchEvent(new Event('DOMContentLoaded', { bubbles: true }));
  readyState = 'complete';
  document.dispatchEvent(new Event('readystatechange'));
  G.dispatchEvent(new Event('load'));
  G.dispatchEvent(new PageTransitionEvent('pageshow'));
};
})();
