type Field = Pick<HTMLTextAreaElement, "value" | "focus" | "setSelectionRange" | "scrollTop" | "scrollHeight">;

/** Focus a text field with the caret after its last character, scrolled so the caret is in view. */
export function focusAtEnd(el: Field): void {
  el.focus();
  const end = el.value.length;
  el.setSelectionRange(end, end);
  el.scrollTop = el.scrollHeight;
}
