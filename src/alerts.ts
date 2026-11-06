/**
 * Away alerts (Settings, Notifications): short messages to a phone or team chat. The sending,
 * gathering, quiet hours and keychain live in src-tauri/src/alerts.rs; this file holds the names
 * and fields the Settings pane shows, and the status line under each destination.
 */
import { ago } from "./search";

export type AlertEvent =
  | "needs_you" | "turn_finished" | "checks_failed" | "checks_green" | "pr_merged"
  | "queue_drained" | "cap_reached" | "runaway" | "morning_summary";

export type AlertService = "ntfy" | "slack" | "discord" | "webhook" | "pushover";

export type Destination = {
  id: string;
  service: AlertService;
  name: string;
  on: boolean;
  events: AlertEvent[];
  last_line: boolean;
  /** Set by the app: a secret is in the keychain, and the host it goes to. */
  saved: boolean;
  hint: string;
};

export type AlertsConfig = {
  destinations: Destination[];
  quiet: { on: boolean; from: string; to: string };
  morning: string;
};

export type AlertStatus = { last_ok?: number | null; last_error?: string | null; last_error_at?: number | null };

export type AlertsView = {
  config: AlertsConfig;
  status: Record<string, AlertStatus>;
  /** Alerts waiting per destination (gathering, quiet hours, the rate limit). */
  held: Record<string, number>;
  quiet_now: boolean;
};

/** What the window sends to save a destination's address or tokens. Never stored by the window. */
export type AlertSecret = { url: string; token: string; user: string };

/** In the order the pane lists them. */
export const EVENTS: { id: AlertEvent; label: string }[] = [
  { id: "needs_you", label: "Needs you: a plan, a question, an OK" },
  { id: "turn_finished", label: "A turn finished" },
  { id: "checks_failed", label: "PR checks failed" },
  { id: "checks_green", label: "PR checks are green" },
  { id: "pr_merged", label: "PR merged" },
  { id: "queue_drained", label: "The queue is done" },
  { id: "cap_reached", label: "A spend cap was reached" },
  { id: "runaway", label: "A runaway agent was stopped" },
  { id: "morning_summary", label: "Morning summary of the queue" },
];

type Field = { key: keyof AlertSecret; label: string; placeholder: string; optional?: boolean };

export const SERVICES: { id: AlertService; name: string; fields: Field[]; note: string }[] = [
  { id: "ntfy", name: "ntfy", note: "Push to the ntfy app on your phone. Pick a topic name nobody would guess.",
    fields: [{ key: "url", label: "Topic URL", placeholder: "https://ntfy.sh/your-topic" }, { key: "token", label: "Access token", placeholder: "Only for a protected topic", optional: true }] },
  { id: "slack", name: "Slack", note: "An incoming webhook posts to one channel.",
    fields: [{ key: "url", label: "Webhook URL", placeholder: "https://hooks.slack.com/services/…" }] },
  { id: "discord", name: "Discord", note: "A channel webhook, from the channel's Integrations settings.",
    fields: [{ key: "url", label: "Webhook URL", placeholder: "https://discord.com/api/webhooks/…" }] },
  { id: "webhook", name: "Webhook", note: "Gets a JSON POST with the event, title, message, link and each item.",
    fields: [{ key: "url", label: "URL", placeholder: "https://…" }, { key: "token", label: "Bearer token", placeholder: "Sent as Authorization", optional: true }] },
  { id: "pushover", name: "Pushover", note: "Push to the Pushover app. Make an app at pushover.net for its token.",
    fields: [{ key: "token", label: "App token", placeholder: "30 letters and digits" }, { key: "user", label: "User key", placeholder: "30 letters and digits" }] },
];

export const serviceInfo = (s: AlertService) => SERVICES.find((x) => x.id === s) ?? SERVICES[0];

/** Every required field has something in it. */
export function secretReady(service: AlertService, s: AlertSecret): boolean {
  return serviceInfo(service).fields.every((f) => f.optional || s[f.key].trim() !== "");
}

/**
 * The line under a destination: its last error, what is waiting, or when it last sent.
 * `now` is epoch milliseconds; the status times are epoch seconds.
 */
export function statusLine(d: Destination, st: AlertStatus | undefined, held: number, quietNow: boolean, now = Date.now()): { text: string; bad?: boolean } {
  const when = (s: number) => { const w = ago(new Date(s * 1000).toISOString(), now); return w === "now" ? "just now" : w; };
  if (!d.saved) return { text: "Not set up yet. Nothing is sent until you save its details." };
  if (st?.last_error) return { text: `Last try failed ${when(st.last_error_at ?? 0)}: ${st.last_error}`, bad: true };
  if (!d.on) return { text: "Off. Nothing is sent." };
  if (held > 0) return { text: quietNow ? `${held} waiting for quiet hours to end` : `${held} waiting, sent together in a moment` };
  if (st?.last_ok) return { text: `Last sent ${when(st.last_ok)}` };
  if (d.events.length === 0) return { text: "Pick at least one event." };
  return { text: "Nothing sent yet" };
}
