// Scroll reveals. Three kinds, set in markup:
//   .reveal                text blocks rise and fade in
//   [data-reveal="media"]  clips and screenshots open from a clip-path window
//   [data-stagger]         the children of a list arrive one after another (capped)
// Content is visible by default; the hidden start state only applies under html.js with motion
// allowed (global.css), so a failed script or reduced motion shows everything at once.
export function reveals() {
  const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const items = document.querySelectorAll<HTMLElement>(".reveal, [data-reveal], [data-stagger]");
  document.querySelectorAll<HTMLElement>("[data-stagger]").forEach((list) => {
    Array.from(list.children).forEach((child, i) => (child as HTMLElement).style.setProperty("--i", String(Math.min(i, 6))));
  });
  if (reduce || !("IntersectionObserver" in window)) {
    items.forEach((el) => el.classList.add("in"));
    return;
  }
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (!e.isIntersecting) continue;
      e.target.classList.add("in");
      io.unobserve(e.target);
    }
  }, { threshold: 0.12, rootMargin: "0px 0px -6% 0px" });
  items.forEach((el) => io.observe(el));
}
