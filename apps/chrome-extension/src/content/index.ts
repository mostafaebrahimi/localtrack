/**
 * Optional detailed interaction capture (spec §48–§51).
 *
 * Injected only when the user turns detailed tracking on and grants host
 * permission. It records that an interaction happened and, for buttons and
 * links, a short accessible label.
 *
 * It never reads input values, textareas, passwords, the clipboard, selected
 * text, keystrokes, cookies or request bodies.
 */

const SCROLL_THROTTLE_MS = 15_000;
const LABEL_MAX = 120;

type Interaction = 'button_click' | 'link_click' | 'form_submit' | 'scroll_activity';

let lastScrollSent = 0;

function send(interaction: Interaction, element?: string, label?: string): void {
  chrome.runtime.sendMessage({
    type: 'localtrack:interaction',
    payload: {
      interaction,
      capturedAt: Date.now(),
      element,
      label,
      url: window.location.href,
    },
  });
}

/**
 * A safe label for a control: its accessible name only, never a value.
 * Anything that could contain typed text is refused.
 */
function safeLabel(element: Element): string | undefined {
  const tag = element.tagName.toLowerCase();
  if (tag === 'input' || tag === 'textarea' || tag === 'select') {
    const type = (element as HTMLInputElement).type;
    // Buttons rendered as inputs are fine; anything that holds typed text is not.
    if (type !== 'button' && type !== 'submit' && type !== 'reset') return undefined;
  }

  const aria = element.getAttribute('aria-label');
  const text = aria ?? (element as HTMLElement).innerText ?? element.textContent ?? '';
  const normalized = text.replace(/\s+/g, ' ').trim();
  if (!normalized) return undefined;
  return normalized.slice(0, LABEL_MAX);
}

function closestControl(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  return target.closest('button, a, [role="button"], input[type="submit"], input[type="button"]');
}

document.addEventListener(
  'click',
  (event) => {
    const control = closestControl(event.target);
    if (!control) return;
    const tag = control.tagName.toLowerCase();
    const interaction: Interaction = tag === 'a' ? 'link_click' : 'button_click';
    send(interaction, tag, safeLabel(control));
  },
  { capture: true, passive: true },
);

document.addEventListener(
  'submit',
  () => {
    // Only the fact that a form was submitted is recorded (spec §50).
    send('form_submit', 'form');
  },
  { capture: true, passive: true },
);

document.addEventListener(
  'scroll',
  () => {
    const now = Date.now();
    if (now - lastScrollSent < SCROLL_THROTTLE_MS) return;
    lastScrollSent = now;
    // Coarse activity only: no scroll positions are stored (spec §51).
    send('scroll_activity');
  },
  { capture: true, passive: true },
);
