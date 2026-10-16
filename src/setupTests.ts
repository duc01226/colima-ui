import '@testing-library/jest-dom/vitest';

// jsdom has no native dialog methods. These DOM-only seams expose open state;
// focus, keyboard and native modal behavior require installed qualification.
Object.defineProperties(HTMLDialogElement.prototype, {
  showModal: { configurable: true, writable: true, value(this: HTMLDialogElement) { this.setAttribute('open', ''); } },
  close: { configurable: true, writable: true, value(this: HTMLDialogElement) { this.removeAttribute('open'); } },
});
