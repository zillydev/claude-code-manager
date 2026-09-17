import { describe, it, expect } from "vitest";
import { backdropDismiss } from "./backdropDismiss";

const backdrop = { id: "backdrop" };
const inside = { id: "textarea" };
const on = (target: object) => ({ target, currentTarget: backdrop });

/** A real mouse gesture: down and up where they happened, then click on the nearest common ancestor. */
function gesture(h: ReturnType<typeof backdropDismiss>, down: object, up: object) {
  h.onMouseDown(on(down));
  h.onMouseUp(on(up));
  h.onClick(on(down === up ? down : backdrop));
}

describe("backdropDismiss", () => {
  it("closes on a click on the backdrop", () => {
    let closed = 0;
    gesture(backdropDismiss(() => closed++), backdrop, backdrop);
    expect(closed).toBe(1);
  });

  it("stays open when a selection drag starts inside and ends on the backdrop", () => {
    let closed = 0;
    gesture(backdropDismiss(() => closed++), inside, backdrop);
    expect(closed).toBe(0);
  });

  it("stays open when a press on the backdrop is released inside", () => {
    let closed = 0;
    gesture(backdropDismiss(() => closed++), backdrop, inside);
    expect(closed).toBe(0);
  });

  it("stays open on a click inside the dialog", () => {
    let closed = 0;
    gesture(backdropDismiss(() => closed++), inside, inside);
    expect(closed).toBe(0);
  });

  it("judges each gesture on its own", () => {
    let closed = 0;
    const h = backdropDismiss(() => closed++);
    gesture(h, inside, backdrop);
    gesture(h, backdrop, backdrop);
    expect(closed).toBe(1);
  });
});
