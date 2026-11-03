// SJ Utility UI 1.0 (2026-11-02): select, tooltip, dialog. Pairs with sj-ui.css.
// Netlume needs no menu component, so none is included.

const chevron = `<svg class="icon" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path fill="currentColor" d="M2.14645 4.64645C2.34171 4.45118 2.65829 4.45118 2.85355 4.64645L6 7.79289L9.14645 4.64645C9.34171 4.45118 9.65829 4.45118 9.85355 4.64645C10.0488 4.84171 10.0488 5.15829 9.85355 5.35355L6.35355 8.85355C6.15829 9.04882 5.84171 9.04882 5.64645 8.85355L2.14645 5.35355C1.95118 5.15829 1.95118 4.84171 2.14645 4.64645Z"/></svg>`; // Fluent UI System Icons, MIT

/** ComboBox: a button whose listbox flyout opens with the chosen option over the field.
 * @param {{ options: {value: string, label: string}[], value: string, label: string, onChange: (value: string) => void }} config
 * @returns {HTMLButtonElement & { value: string }} */
export function select({ options, value, label, onChange }) {
  const button = /** @type {any} */ (document.createElement("button"));
  button.type = "button"; button.className = "btn select";
  button.setAttribute("aria-haspopup", "listbox"); button.setAttribute("aria-label", label);
  const render = () => {
    button.innerHTML = `<span></span>${chevron}`;
    button.firstElementChild.textContent = options.find((o) => o.value === button.value)?.label ?? "";
  };
  button.value = value; render();
  button.addEventListener("click", () => openList());
  button.addEventListener("keydown", (/** @type {KeyboardEvent} */ e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); openList(); }
  });

  function openList() {
    const list = document.createElement("div");
    list.className = "flyout"; list.setAttribute("role", "listbox"); list.tabIndex = -1;
    list.setAttribute("aria-label", label);
    let active = Math.max(0, options.findIndex((o) => o.value === button.value));
    const items = options.map((o, i) => {
      const item = document.createElement("div");
      item.setAttribute("role", "option"); item.id = `sj-opt-${i}`; item.textContent = o.label;
      item.setAttribute("aria-selected", String(o.value === button.value));
      item.addEventListener("click", () => choose(i));
      item.addEventListener("pointermove", () => mark(i));
      return item;
    });
    list.append(...items); document.body.append(list);
    const box = button.getBoundingClientRect();
    list.style.minWidth = `${box.width}px`;
    list.style.left = `${Math.min(box.left, innerWidth - list.offsetWidth - 8)}px`;
    const top = box.top - items[active].offsetTop - 1 + (box.height - items[active].offsetHeight) / 2;
    list.style.top = `${Math.max(8, Math.min(top, innerHeight - list.offsetHeight - 8))}px`;
    mark(active); list.focus();

    function mark(i) {
      active = i; items.forEach((item, n) => item.classList.toggle("is-active", n === i));
      list.setAttribute("aria-activedescendant", items[i].id);
    }
    function close() { list.remove(); removeEventListener("pointerdown", outside, true); button.focus(); }
    function choose(i) {
      close();
      if (options[i].value !== button.value) { button.value = options[i].value; render(); onChange(button.value); }
    }
    /** @param {PointerEvent} e */
    function outside(e) { if (!list.contains(/** @type {Node} */ (e.target))) close(); }
    addEventListener("pointerdown", outside, true);
    list.addEventListener("focusout", (e) => { if (!list.contains(/** @type {Node} */ (e.relatedTarget))) close(); });
    list.addEventListener("keydown", (e) => {
      if (e.key === "ArrowDown") mark(Math.min(items.length - 1, active + 1));
      else if (e.key === "ArrowUp") mark(Math.max(0, active - 1));
      else if (e.key === "Home") mark(0);
      else if (e.key === "End") mark(items.length - 1);
      else if (e.key === "Enter" || e.key === " ") choose(active);
      else if (e.key === "Escape" || e.key === "Tab") close();
      else return;
      e.preventDefault(); e.stopPropagation();
    });
  }
  return button;
}

/** Tooltips for every [data-tip] element: ~700 ms on hover, at once on keyboard focus. */
export function tooltips() {
  /** @type {HTMLDivElement | null} */ let tip = null; let timer = 0;
  const hide = () => { clearTimeout(timer); tip?.remove(); tip = null; };
  /** @param {HTMLElement} el */
  const show = (el) => {
    hide(); tip = document.createElement("div"); tip.className = "tip"; tip.setAttribute("role", "tooltip");
    tip.textContent = el.dataset.tip ?? ""; document.body.append(tip);
    const box = el.getBoundingClientRect();
    const below = box.bottom + 4 + tip.offsetHeight < innerHeight;
    tip.style.top = `${below ? box.bottom + 4 : box.top - tip.offsetHeight - 4}px`;
    tip.style.left = `${Math.max(8, Math.min(box.left + (box.width - tip.offsetWidth) / 2, innerWidth - tip.offsetWidth - 8))}px`;
  };
  /** @param {Event} e */
  const target = (e) => /** @type {HTMLElement | null} */ (/** @type {Element} */ (e.target).closest?.("[data-tip]"));
  document.addEventListener("pointerover", (e) => {
    const el = target(e); if (!el) return;
    clearTimeout(timer); timer = window.setTimeout(() => show(el), 700);
  });
  document.addEventListener("pointerout", (e) => { if (target(e)) hide(); });
  document.addEventListener("focusin", (e) => {
    const el = target(e); if (el?.matches(":focus-visible")) show(el); else hide();
  });
  document.addEventListener("focusout", hide);
  document.addEventListener("pointerdown", hide);
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") hide(); });
}

/** Modal dialog: title (Subtitle), body, footer buttons (accent first). Esc closes; the dialog
 * is removed on close. An action returning false keeps it open.
 * @param {{ title: string, body: HTMLElement, actions: {label: string, accent?: boolean, run?: () => (boolean | void | Promise<boolean | void>)}[] }} config */
export function dialog({ title, body, actions }) {
  const d = document.createElement("dialog");
  d.className = "dialog"; d.setAttribute("aria-label", title);
  d.innerHTML = `<div class="dialog-body"><h2 class="t-subtitle"></h2></div><div class="dialog-foot"></div>`;
  d.querySelector("h2").textContent = title;
  d.firstElementChild.append(body);
  for (const action of actions) {
    const b = document.createElement("button");
    b.type = "button"; b.className = action.accent ? "btn accent" : "btn"; b.textContent = action.label;
    b.addEventListener("click", async () => { if ((await action.run?.()) !== false) d.close(); });
    d.lastElementChild.append(b);
  }
  d.addEventListener("close", () => d.remove());
  document.body.append(d); d.showModal();
  return d;
}
