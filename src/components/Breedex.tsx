import type { Breed } from "../api";
import { Mascot } from "./Mascot";

/** Every breed in the world list; ones you have not met yet stay hidden. */
export function Breedex({ breeds, discovered }: { breeds: Breed[]; discovered: Set<string> }) {
  const found = breeds.filter((b) => discovered.has(b.slug)).length;
  return (
    <div className="breedex">
      <header className="breedex-head">
        <Mascot size={72} mood={found === breeds.length && found > 0 ? "happy" : "idle"} />
        <div>
          <h1>Breedex</h1>
          <p className="muted num">
            {found} of {breeds.length} breeds discovered. Every new workspace meets a breed you haven't met yet.
          </p>
        </div>
      </header>
      <div className="breedex-grid">
        {breeds.map((b) =>
          discovered.has(b.slug) ? (
            <article key={b.slug} className="breed">
              <span className="origin">{b.origin}</span>
              <h3>{b.name}</h3>
              <p>{b.fact}</p>
            </article>
          ) : (
            <article key={b.slug} className="breed locked" aria-label="Not discovered yet">
              <span className="silhouette"><Mascot size={44} mood="sleep" title="Undiscovered breed" /></span>
              <span>Not met yet</span>
            </article>
          ),
        )}
      </div>
    </div>
  );
}
