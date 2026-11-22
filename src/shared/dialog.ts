// Opening a native <dialog>: showModal() focuses its first focusable thing, and WebKit then draws that thing's focus
// ring (:focus-visible) as if it had been reached with Tab — the first button of every dialog (an icon picker, 取消, a
// segmented choice) opened with a box drawn around it. Instead the focus goes where typing is wanted or nowhere:
// - the first focusable is a text box: it keeps the focus (as before);
// - `field`: the first text box anywhere in the dialog (a form whose first control is an icon picker or a choice);
// - else the dialog itself (no ring: index.css; Tab starts at its first control, Esc still closes it).

const TEXT = 'input:not([type=hidden],[type=checkbox],[type=radio],[type=button],[type=submit],[type=range],[type=file],[type=color]), textarea, [contenteditable=""], [contenteditable=true]';

const typing = (el: Element | null): el is HTMLElement => !!el && el.matches(TEXT) && !(el as HTMLInputElement).disabled && !(el as HTMLInputElement).readOnly;

export function showDialog(d: HTMLDialogElement | null, opts: { field?: boolean } = {}) {
  if (!d || d.open) return; // StrictMode runs effects twice
  d.showModal();
  if (typing(document.activeElement) && d.contains(document.activeElement)) return;
  if (opts.field) {
    const f = [...d.querySelectorAll(TEXT)].find((x) => typing(x) && (x as HTMLElement).offsetParent !== null) as HTMLElement | undefined;
    if (f) {
      f.focus({ preventScroll: true });
      return;
    }
  }
  if (!d.hasAttribute("tabindex")) d.tabIndex = -1;
  d.focus({ preventScroll: true });
}
