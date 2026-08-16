// Minimal DOM helpers. Panels are generated from schemas rather than written
// out as markup, so adding a parameter only means adding a schema entry.

export function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === 'class') node.className = v;
    else if (k === 'text') node.textContent = v;
    else if (k === 'html') node.innerHTML = v;
    else if (k.startsWith('on') && typeof v === 'function') node.addEventListener(k.slice(2), v);
    else if (k === 'style' && typeof v === 'object') Object.assign(node.style, v);
    else if (v !== null && v !== undefined) node.setAttribute(k, v);
  }
  for (const child of [].concat(children)) {
    if (child == null) continue;
    node.appendChild(typeof child === 'string' ? document.createTextNode(child) : child);
  }
  return node;
}

export function clear(node) {
  while (node.firstChild) node.removeChild(node.firstChild);
  return node;
}

export function row(label, control, hint) {
  return el('label', { class: 'field' }, [
    el('span', { class: 'field-label', text: label }),
    control,
    hint ? el('span', { class: 'field-hint', text: hint }) : null,
  ]);
}

export function numberField(label, { value, min, max, step, onInput, hint }) {
  const slider = el('input', { type: 'range', min, max, step, value });
  const box = el('input', { type: 'number', min, max, step, value, class: 'num' });
  const sync = (v, from) => {
    const n = Number(v);
    if (Number.isNaN(n)) return;
    if (from !== 'slider') slider.value = n;
    if (from !== 'box') box.value = n;
    onInput(n);
  };
  slider.addEventListener('input', () => sync(slider.value, 'slider'));
  box.addEventListener('input', () => sync(box.value, 'box'));
  return row(label, el('span', { class: 'num-pair' }, [slider, box]), hint);
}

export function rangeField(label, opts) {
  const { minValue, maxValue, min, max, step, onInput, hint } = opts;
  const a = el('input', { type: 'number', min, max, step, value: minValue, class: 'num' });
  const b = el('input', { type: 'number', min, max, step, value: maxValue, class: 'num' });
  const emit = () => {
    let lo = Number(a.value);
    let hi = Number(b.value);
    if (Number.isNaN(lo) || Number.isNaN(hi)) return;
    if (hi < lo) hi = lo;
    b.value = hi;
    onInput(lo, hi);
  };
  a.addEventListener('input', emit);
  b.addEventListener('input', emit);
  return row(label, el('span', { class: 'range-pair' }, [a, el('span', { text: 'to' }), b]), hint);
}

export function selectField(label, { value, options, onInput, hint }) {
  const sel = el('select', {});
  for (const opt of options) {
    const o = typeof opt === 'string' ? { value: opt, label: opt } : opt;
    const node = el('option', { value: o.value, text: o.label });
    if (o.value === value) node.selected = true;
    sel.appendChild(node);
  }
  sel.addEventListener('change', () => onInput(sel.value));
  return row(label, sel, hint);
}

export function boolField(label, { value, onInput, hint }) {
  const box = el('input', { type: 'checkbox' });
  box.checked = !!value;
  box.addEventListener('change', () => onInput(box.checked));
  return row(label, box, hint);
}

export function textField(label, { value, onInput, hint }) {
  const input = el('input', { type: 'text', value });
  input.addEventListener('input', () => onInput(input.value));
  return row(label, input, hint);
}

export function colorField(label, { value, onInput, hint }) {
  const input = el('input', { type: 'color', value });
  input.addEventListener('input', () => onInput(input.value));
  return row(label, input, hint);
}

export function button(text, onClick, cls = '') {
  return el('button', { class: `btn ${cls}`.trim(), type: 'button', onclick: onClick, text });
}

export function section(title, children, actions) {
  return el('section', { class: 'group' }, [
    el('header', { class: 'group-head' }, [
      el('h3', { text: title }),
      actions ? el('span', { class: 'group-actions' }, actions) : null,
    ]),
    el('div', { class: 'group-body' }, children),
  ]);
}
