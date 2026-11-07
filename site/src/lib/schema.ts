// JSON-LD for search engines and answer engines. Every page passes what it needs to Base.astro
// through the `jsonld` prop. Text in a FAQPage must match the FAQ shown on the page word for word.
import { SITE, AGENTS, absolute, appVersion } from "./site";

type Schema = Record<string, unknown>;

const author = { "@type": "Person", name: SITE.author, url: "https://github.com/Harshil-Jani" };

export function softwareApp(): Schema {
  return {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    "@id": absolute("#app"),
    name: "NekoTrail",
    url: absolute(),
    description:
      "Free, open-source desktop app that runs Claude Code, Codex, Gemini CLI and 13 more coding agents in parallel, each in its own git worktree and branch, on the user's own agent logins.",
    applicationCategory: "DeveloperApplication",
    applicationSubCategory: "Coding agent manager",
    operatingSystem: "macOS 13 or later; Linux x86_64 (experimental)",
    softwareVersion: appVersion(),
    downloadUrl: SITE.releases,
    installUrl: absolute("download/"),
    releaseNotes: absolute("changelog/"),
    license: "https://opensource.org/license/mit",
    isAccessibleForFree: true,
    offers: { "@type": "Offer", price: "0", priceCurrency: "USD" },
    screenshot: absolute("shots/workspace.webp"),
    featureList: [
      "One git worktree and branch per task",
      "Runs " + AGENTS.join(", "),
      "Compare two agents on the same task",
      "Hand off a task from one agent to another in the same branch",
      "Import repos, workspaces and chats from Conductor",
      "10 ports per workspace",
    ],
    author,
    sameAs: [SITE.repo],
  };
}

export function website(): Schema {
  return {
    "@context": "https://schema.org",
    "@type": "WebSite",
    name: "NekoTrail",
    url: absolute(),
    publisher: author,
  };
}

export type Faq = [question: string, answer: string][];

export function faq(items: Faq): Schema {
  return {
    "@context": "https://schema.org",
    "@type": "FAQPage",
    mainEntity: items.map(([q, a]) => ({
      "@type": "Question",
      name: q,
      acceptedAnswer: { "@type": "Answer", text: a },
    })),
  };
}

/** Trail of [name, path under the base]; "" is the home page. */
export function breadcrumbs(trail: [name: string, path: string][]): Schema {
  return {
    "@context": "https://schema.org",
    "@type": "BreadcrumbList",
    itemListElement: trail.map(([name, p], i) => ({
      "@type": "ListItem", position: i + 1, name, item: absolute(p),
    })),
  };
}

export function blogPosting(
  post: { id: string; data: { title: string; description: string; date: Date; author: string } },
  image: string,
): Schema {
  return {
    "@context": "https://schema.org",
    "@type": "BlogPosting",
    headline: post.data.title,
    description: post.data.description,
    datePublished: post.data.date.toISOString(),
    author: { ...author, name: post.data.author },
    mainEntityOfPage: absolute(`blog/${post.id}/`),
    image: absolute(image),
    about: { "@id": absolute("#app") },
  };
}
