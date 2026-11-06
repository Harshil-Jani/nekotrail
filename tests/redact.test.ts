// Run with `bun test tests/`.
import { describe, expect, test } from "bun:test";
import { hint, isPlaceholder, maskText, scanSecrets, shortenHome } from "../src/redact";

const kinds = (text: string, env = false) => scanSecrets(text, { env }).map((s) => [s.kind, s.value]);

// Made-up values in the shape of real ones. None of them is a live credential.
const GH = "ghp_" + "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8";
const GH_PAT = "github_pat_" + "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz";
const OPENAI = "sk-proj-" + "Ab12Cd34Ef56Gh78Ij90Kl12";
const ANTHROPIC = "sk-ant-api03-" + "Zx9Yw8Vu7Ts6Rq5Po4Nm3";
const AWS = "AKIA" + "IOSFODNN7EXAMPL1";
const STRIPE = "sk_live_" + "51HbQ2eLkdjf83Kdl2";
const SLACK = "xoxb-" + "123456789012-abcdefABCDEF";
const GOOGLE = "AIza" + "SyA1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q";
const JWT = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";

describe("secret masking: common token formats", () => {
  test("names each format", () => {
    expect(kinds(`export GH=${GH}`)[0]).toEqual(["GitHub token", GH]);
    expect(kinds(`token ${GH_PAT} here`)).toEqual([["GitHub token", GH_PAT]]);
    expect(kinds(`key ${OPENAI}`)).toEqual([["API key", OPENAI]]);
    expect(kinds(`key ${ANTHROPIC}`)).toEqual([["Anthropic key", ANTHROPIC]]);
    expect(kinds(`aws configure set aws_access_key_id ${AWS}`)).toEqual([["AWS access key", AWS]]);
    expect(kinds(`stripe ${STRIPE}`)).toEqual([["Stripe key", STRIPE]]);
    expect(kinds(`slack ${SLACK}`)).toEqual([["Slack token", SLACK]]);
    expect(kinds(`maps ${GOOGLE}`)).toEqual([["Google API key", GOOGLE]]);
    expect(kinds(`cookie ${JWT}`)).toEqual([["JWT", JWT]]);
  });

  test("bearer tokens, passwords in URLs and private keys", () => {
    expect(kinds('curl -H "Authorization: Bearer abcdEFGH1234ijklMNOP5678" https://api.x.io')).toEqual([["Bearer token", "abcdEFGH1234ijklMNOP5678"]]);
    expect(kinds("DATABASE=postgres://app:s3cretPassw0rd@db.internal:5432/app")).toEqual([["Password in a URL", "s3cretPassw0rd"]]);
    const pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nIBAAK\n-----END RSA PRIVATE KEY-----";
    expect(kinds(`key:\n${pem}\nafter`)).toEqual([["Private key", pem]]);
  });

  test("secret-sounding KEY=value, inline in a command too, and quoted config values", () => {
    expect(kinds("OPENAI_API_KEY=abc123def456 npm run dev")).toEqual([["Secret value", "abc123def456"]]);
    expect(kinds('DB_PASSWORD="hunter2hunter2"')).toEqual([["Secret value", "hunter2hunter2"]]);
    expect(kinds('{"apiKey": "k-29fj39fj2", "name": "x"}')).toEqual([["Secret value", "k-29fj39fj2"]]);
    expect(kinds("client_secret: 'zz98aa77bb66'")).toEqual([["Secret value", "zz98aa77bb66"]]);
  });

  test("every value in a .env file, only when the text came from one", () => {
    const env = "PORT=8080\nSENTRY_ORG=pawprint\n# comment\nEMPTY=\nREF=${OTHER}";
    expect(kinds(env, true)).toEqual([[".env value", "pawprint"]]);
    expect(kinds(env, false)).toEqual([]);
  });

  test("emails", () => {
    expect(kinds("Author: Sam Lee <sam.lee@pawprint.io>")).toEqual([["Email address", "sam.lee@pawprint.io"]]);
  });
});

describe("secret masking: false-positive guard", () => {
  test("leaves ordinary engineering text alone", () => {
    const text = [
      "commit 9fceb02d0ae598e95dc970b74767f19372d61af8", // a git SHA
      "id 123e4567-e89b-12d3-a456-426614174000", // a UUID
      "pip install scikit-learn; see risk-assessment-for-the-quarter",
      "GITHUB_TOKEN=$GITHUB_TOKEN gh pr create",
      "API_KEY=${{ secrets.API_KEY }}",
      "TOKEN_TTL=3600 SESSION_TIMEOUT=30s",
      "AUTHOR=sam",
      'password: "changeme"',
      "const token = req.headers.authorization;",
      "git@github.com:pawprint/api.git",
      "icon@2x.png and logo@3x.webp",
      "noreply@github.com, 123+sam@users.noreply.github.com, you@example.com",
      "npm i @types/node",
      "Bearer token",
    ].join("\n");
    expect(scanSecrets(text)).toEqual([]);
  });

  test("placeholders", () => {
    for (const v of ["$TOKEN", "<your-key>", "xxxx", "****", "your_api_key", "true", "3600", "{{key}}", "process.env.KEY"]) expect(isPlaceholder(v)).toBe(true);
    expect(isPlaceholder("a8f3k2m9")).toBe(false);
  });
});

describe("maskText", () => {
  test("hides each secret with a label, keeps the rest", () => {
    expect(maskText(`push with ${GH} to sam@pawprint.io`)).toBe("push with [hidden: GitHub token] to [hidden: Email address]");
  });

  test("a value the user chose to show stays", () => {
    expect(maskText(`push with ${GH} to sam@pawprint.io`, {}, new Set(["sam@pawprint.io"]))).toBe("push with [hidden: GitHub token] to sam@pawprint.io");
  });

  test("a long value found elsewhere is hidden here too; a short one is not", () => {
    const found = [{ value: "pawprint-prod-8842", kind: ".env value" }, { value: "8080", kind: ".env value" }];
    expect(maskText("echo pawprint-prod-8842 on 8080", {}, new Set(), found)).toBe("echo [hidden: .env value] on 8080");
  });

  test("quotes stay around a hidden value", () => {
    expect(maskText('DB_PASSWORD="hunter2hunter2"')).toBe('DB_PASSWORD="[hidden: Secret value]"');
  });

  test("the hint never shows a whole secret", () => {
    expect(hint(GH)).toBe("ghp_•••••••• (40 characters)");
    expect(hint("abcdefgh")).toBe("ab•••••• (8 characters)");
    expect(hint("abcd")).not.toContain("abcd");
  });
});

describe("shortenHome", () => {
  test("home becomes ~ only as a whole folder", () => {
    expect(shortenHome("cd /Users/sam/code && ls /Users/sam", "/Users/sam")).toBe("cd ~/code && ls ~");
    expect(shortenHome("/Users/samantha/code", "/Users/sam")).toBe("/Users/samantha/code");
    expect(shortenHome("/home/sam/x", "/home/sam/")).toBe("~/x");
    expect(shortenHome("/x", null)).toBe("/x");
  });
});
