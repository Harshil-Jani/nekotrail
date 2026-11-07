// Every cat on the page ([data-cat]) watches the pointer: its eyes glide toward it, clamped inside
// the face, and now and then it blinks. Touch screens and reduced motion keep the eyes centred.
// One rAF loop for all cats, running only while some eye is still moving.

type Cat = { el: SVGSVGElement; balls: SVGElement[]; glints: SVGElement[]; rx: number; ry: number; x: number; y: number; tx: number; ty: number };

export function catEyes() {
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)");
  const fine = window.matchMedia("(hover: hover) and (pointer: fine)");
  const cats: Cat[] = Array.from(document.querySelectorAll<SVGSVGElement>("svg[data-cat]")).map((el) => {
    const [rx, ry] = (el.dataset.reach ?? "2,1.6").split(",").map(Number);
    return {
      el, rx, ry, x: 0, y: 0, tx: 0, ty: 0,
      balls: Array.from(el.querySelectorAll<SVGElement>(".eye-ball")),
      glints: Array.from(el.querySelectorAll<SVGElement>(".eye-glint")),
    };
  }).filter((c) => c.balls.length);
  if (!cats.length) return;

  let pointer: { x: number; y: number } | null = null;
  let frame = 0;

  const aim = () => {
    for (const c of cats) {
      if (!pointer || reduce.matches || !fine.matches) { c.tx = 0; c.ty = 0; continue; }
      const r = c.el.getBoundingClientRect();
      if (!r.width) continue;
      // Aim from the middle of the face (upper half of the drawing).
      const dx = pointer.x - (r.left + r.width / 2);
      const dy = pointer.y - (r.top + r.height * 0.45);
      const d = Math.hypot(dx, dy) || 1;
      const reach = Math.min(1, d / 260); // nearer pointer, smaller glance
      c.tx = (dx / d) * c.rx * reach;
      c.ty = (dy / d) * c.ry * reach;
    }
  };

  const step = () => {
    let moving = false;
    for (const c of cats) {
      c.x += (c.tx - c.x) * 0.18;
      c.y += (c.ty - c.y) * 0.18;
      if (Math.abs(c.tx - c.x) > 0.01 || Math.abs(c.ty - c.y) > 0.01) moving = true;
      for (const b of c.balls) b.style.translate = `${c.x.toFixed(2)}px ${c.y.toFixed(2)}px`;
      // The glint rides a little further, which reads as the eye turning.
      for (const g of c.glints) g.style.translate = `${(c.x * 1.3).toFixed(2)}px ${(c.y * 1.3).toFixed(2)}px`;
    }
    frame = moving ? requestAnimationFrame(step) : 0;
  };
  const kick = () => { aim(); if (!frame) frame = requestAnimationFrame(step); };

  window.addEventListener("pointermove", (e) => {
    if (e.pointerType !== "mouse") return;
    pointer = { x: e.clientX, y: e.clientY };
    kick();
  }, { passive: true });
  document.documentElement.addEventListener("pointerleave", () => { pointer = null; kick(); });
  window.addEventListener("scroll", () => { if (pointer) kick(); }, { passive: true });
  reduce.addEventListener("change", kick);
  fine.addEventListener("change", kick);

  // Blink: one cat at a time, every 3 to 7 seconds, only when motion is welcome and the tab is visible.
  const blink = () => {
    if (!reduce.matches && document.visibilityState === "visible") {
      const visible = cats.filter((c) => {
        const r = c.el.getBoundingClientRect();
        return r.bottom > 0 && r.top < window.innerHeight && r.width > 0;
      });
      const c = visible[Math.floor(Math.random() * visible.length)];
      if (c) {
        c.el.classList.remove("blink");
        void c.el.getBoundingClientRect();
        c.el.classList.add("blink");
        setTimeout(() => c.el.classList.remove("blink"), 260);
      }
    }
    setTimeout(blink, 3000 + Math.random() * 4000);
  };
  setTimeout(blink, 2400);
}
