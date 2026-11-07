// The hero's one authored moment: the NekoTrail wordmark forms out of the night sky.
//   0 to 700 ms     stars drift in from across the sky and lock onto points along each letter
//   520 to 950 ms   hairline constellation lines draw through them, letter by letter
//   900 to 1350 ms  the lines swell into the solid wordmark, the stars fade, the i gets its star
//   1150 to 1500 ms the cat hops onto the T
// Runs once. The markup is the finished wordmark; reduced motion or no script shows it as is.

const EASE = "cubic-bezier(0.16, 1, 0.3, 1)";
const SVGNS = "http://www.w3.org/2000/svg";

export function wordmarkReveal() {
  const svg = document.querySelector<SVGSVGElement>("svg[data-wordmark]");
  if (!svg) return;
  const done = () => svg.classList.add("wm-done");
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches || !svg.animate) return done();

  const host = svg.closest<HTMLElement>(".sky") ?? document.body;
  const paths = Array.from(svg.querySelectorAll<SVGPathElement>(".wm-strokes path"));
  const star = svg.querySelector<SVGPathElement>(".wm-star");
  const cat = svg.querySelector<SVGGElement>(".wm-cat-g");
  const ctm = svg.getScreenCTM();
  if (!ctm || !paths.length) return done();
  const toSvg = (x: number, y: number) => new DOMPoint(x, y).matrixTransform(ctm.inverse());
  const box = host.getBoundingClientRect();
  const stroke = Number(getComputedStyle(paths[0]).strokeWidth.replace("px", "")) || 15;

  // Seeded, so every visit draws the same sky.
  let seed = 7;
  const rand = () => (seed = (seed * 16807) % 2147483647) / 2147483647;

  const dots = document.createElementNS(SVGNS, "g");
  dots.setAttribute("class", "wm-dots");
  svg.appendChild(dots);
  const anims: Animation[] = [];

  paths.forEach((p, li) => {
    const len = p.getTotalLength();
    const n = Math.max(2, Math.round(len / 26));
    for (let i = 0; i <= n; i++) {
      const pt = p.getPointAtLength((len * i) / n);
      const from = toSvg(box.left + rand() * box.width, box.top + rand() * Math.min(box.height, window.innerHeight));
      const c = document.createElementNS(SVGNS, "circle");
      c.setAttribute("cx", pt.x.toFixed(1));
      c.setAttribute("cy", pt.y.toFixed(1));
      c.setAttribute("r", (rand() > 0.8 ? 3.2 : 2.2).toFixed(1));
      dots.appendChild(c);
      const delay = rand() * 180 + li * 12;
      anims.push(c.animate(
        [
          { transform: `translate(${(from.x - pt.x).toFixed(1)}px, ${(from.y - pt.y).toFixed(1)}px)`, opacity: 0 },
          { opacity: 0.9, offset: 0.35 },
          { transform: "translate(0, 0)", opacity: 1 },
        ],
        { duration: 620, delay, easing: EASE, fill: "both" },
      ));
      anims.push(c.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 360, delay: 980, easing: "ease-out", fill: "forwards" }));
    }
    // The constellation line through this letter, then the swell into the solid stroke.
    p.style.strokeDasharray = `${len} ${len}`;
    anims.push(p.animate(
      [
        { strokeDashoffset: len, strokeWidth: "1.4px", opacity: 0.75 },
        { strokeDashoffset: 0, strokeWidth: "1.4px", opacity: 0.75, offset: 0.48 },
        { strokeDashoffset: 0, strokeWidth: `${stroke}px`, opacity: 1 },
      ],
      { duration: 840, delay: 520 + li * 28, easing: EASE, fill: "both" },
    ));
  });

  if (star) anims.push(star.animate(
    [{ transform: "scale(0) rotate(-45deg)", opacity: 0 }, { transform: "scale(1) rotate(0deg)", opacity: 1 }],
    { duration: 420, delay: 1080, easing: EASE, fill: "both" },
  ));
  if (cat) anims.push(cat.animate(
    [{ transform: "translateY(-14px)", opacity: 0 }, { transform: "translateY(0)", opacity: 1 }],
    { duration: 360, delay: 1150, easing: EASE, fill: "both" },
  ));

  svg.classList.add("wm-playing");
  Promise.all(anims.map((a) => a.finished)).catch(() => {}).finally(() => {
    dots.remove();
    for (const p of paths) p.style.strokeDasharray = "";
    for (const a of anims) a.cancel();
    svg.classList.remove("wm-playing");
    done();
  });
}
