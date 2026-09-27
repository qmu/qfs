/**
 * The browser entrypoint: the column view (plgg-view
 * Html), the poll effect over the host's loopback JSON
 * API, and the mount. The only impure module (fetch, DOM,
 * clock); the walk itself is `domain/usecase/walk.ts`.
 */
import { match } from "plgg";
import {
  type Html,
  div,
  span,
  button,
  nav,
  section,
  h2,
  text,
  class_,
  onClick,
} from "plgg-view";
import {
  type Url,
  application,
  cmdEffect,
} from "plgg-view/client";
import {
  asMembers,
  asAccounts,
  asSessions,
} from "../domain/model/Cluster.ts";
import {
  type Column,
  type Item,
  type Model,
  type Msg,
  areas,
  areaCount,
  columns,
  initialModel,
  makeUpdate,
  open,
  polled,
  pollFailed,
  subscriptions,
  list$,
  detail$,
} from "../domain/usecase/walk.ts";

const getJson = (
  path: string,
): Promise<unknown> =>
  fetch(path, { cache: "no-store" }).then((r) =>
    r.ok
      ? r.json()
      : Promise.reject(
          new Error(`${path}: HTTP ${r.status}`),
        ),
  );

const poll = (): Promise<Msg> =>
  Promise.all([
    getJson("/api/cluster/members"),
    getJson("/api/cluster/accounts"),
    getJson("/api/cluster/sessions"),
  ]).then(
    ([m, a, s]) =>
      polled(
        {
          members: asMembers(m),
          accounts: asAccounts(a),
          sessions: asSessions(s),
        },
        Date.now(),
      ),
    (e: unknown) =>
      pollFailed(
        e instanceof Error
          ? e.message
          : String(e),
      ),
  );

// A two-scheme palette through custom properties (the
// color-scheme idea borrowed from plggmatic): one set of
// role variables, re-inked under prefers-color-scheme.
const STYLE = `
:root { --surface:#ffffff; --surface-2:#f4f4f5; --text:#18181b; --muted:#71717a; --border:#e4e4e7; --accent:#18181b; --ok:#15803d; --warn:#b45309; --off:#a1a1aa; --danger:#b91c1c; }
@media (prefers-color-scheme: dark) { :root { --surface:#18181b; --surface-2:#27272a; --text:#f4f4f5; --muted:#a1a1aa; --border:#3f3f46; --accent:#f4f4f5; --ok:#4ade80; --warn:#fbbf24; --off:#71717a; --danger:#f87171; } }
html, body { margin:0; height:100%; }
body { font:14px/1.45 system-ui, sans-serif; background:var(--surface); color:var(--text); }
.console { display:flex; flex-direction:column; height:100vh; }
.rail { display:flex; gap:1rem; align-items:center; padding:.4rem .9rem; border-bottom:1px solid var(--border); background:var(--surface-2); font-size:12px; color:var(--muted); }
.rail .brand { color:var(--text); font-weight:600; }
.rail .error { color:var(--danger); }
.strip { flex:1; display:flex; overflow-x:auto; min-height:0; }
.col { flex:0 0 300px; display:flex; flex-direction:column; border-right:1px solid var(--border); min-height:0; }
.col.areas { flex-basis:200px; background:var(--surface-2); }
.col h2 { margin:0; padding:.6rem .9rem; font-size:12px; letter-spacing:.04em; text-transform:uppercase; color:var(--muted); border-bottom:1px solid var(--border); white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
.col .body { overflow-y:auto; flex:1; }
.item { display:block; width:100%; text-align:left; border:0; border-bottom:1px solid var(--border); background:transparent; color:inherit; font:inherit; padding:.55rem .9rem; cursor:pointer; }
.item:hover { background:var(--surface-2); }
.item.active { box-shadow: inset 3px 0 0 var(--accent); background:var(--surface-2); }
.item .label { display:block; font-weight:500; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.item .detail { display:block; font-size:12px; color:var(--muted); }
.item .count { float:right; color:var(--muted); font-size:12px; }
.tone { display:inline-block; width:.55rem; height:.55rem; border-radius:50%; margin-right:.45rem; background:var(--off); vertical-align:middle; }
.tone.online, .tone.available, .tone.idle { background:var(--ok); }
.tone.busy, .tone.assigned { background:var(--warn); }
.empty { padding:.8rem .9rem; color:var(--muted); font-size:12px; }
.field { padding:.45rem .9rem; border-bottom:1px solid var(--border); }
.field .k { display:block; font-size:11px; color:var(--muted); text-transform:uppercase; letter-spacing:.04em; }
.field .v { display:block; overflow-wrap:anywhere; }
`;

const itemView = (
  it: Item,
): Html<Msg, "button"> =>
  button(
    [
      class_(it.active ? "item active" : "item"),
      onClick(it.msg),
    ],
    [
      span(
        [class_("label")],
        [
          span([class_(`tone ${it.tone}`)], []),
          text(it.label),
        ],
      ),
      span([class_("detail")], [text(it.detail)]),
    ],
  );

const columnView = (
  c: Column,
): Html<Msg, "section"> =>
  match(c)(
    [
      list$(),
      ({ content }): Html<Msg, "section"> =>
        section(
          [class_(`col list ${content.key}`)],
          [
            h2([], [text(content.title)]),
            div(
              [class_("body")],
              content.items.length === 0
                ? [
                    div(
                      [class_("empty")],
                      [text(content.empty)],
                    ),
                  ]
                : content.items.map(itemView),
            ),
          ],
        ),
    ],
    [
      detail$(),
      ({ content }): Html<Msg, "section"> =>
        section(
          [class_(`col detail ${content.key}`)],
          [
            h2([], [text(content.title)]),
            div(
              [class_("body")],
              content.fields.map(([k, v]) =>
                div(
                  [class_("field")],
                  [
                    span(
                      [class_("k")],
                      [text(k)],
                    ),
                    span(
                      [class_("v")],
                      [text(v)],
                    ),
                  ],
                ),
              ),
            ),
          ],
        ),
    ],
  );

const areaView = (
  model: Model,
): Html<Msg, "nav"> =>
  nav(
    [class_("col areas")],
    [
      h2([], [text("qfs cluster")]),
      div(
        [class_("body")],
        areas.map(([area, title]) =>
          button(
            [
              class_(
                model.selection.area === area
                  ? "item active"
                  : "item",
              ),
              onClick(open(area)),
            ],
            [
              span(
                [class_("count")],
                [
                  text(
                    String(
                      areaCount(
                        model.snapshot,
                        area,
                      ),
                    ),
                  ),
                ],
              ),
              span(
                [class_("label")],
                [text(title)],
              ),
            ],
          ),
        ),
      ),
    ],
  );

const status = (model: Model): string =>
  model.polledAt === 0
    ? "connecting…"
    : `${model.snapshot.members.length} members · ${model.snapshot.accounts.length} accounts · ${model.snapshot.sessions.length} sessions · updated ${new Date(model.polledAt).toLocaleTimeString()} (every 5 s)`;

const view = (model: Model): Html<Msg> =>
  div(
    [class_("console")],
    [
      div(
        [class_("rail")],
        [
          span(
            [class_("brand")],
            [text("qfs cluster console")],
          ),
          span([], [text(status(model))]),
          ...(model.error === ""
            ? []
            : [
                span(
                  [class_("error")],
                  [text(model.error)],
                ),
              ]),
        ],
      ),
      div(
        [class_("strip")],
        [
          areaView(model),
          ...columns(model).map(columnView),
        ],
      ),
    ],
  );

const style = document.createElement("style");
style.textContent = STYLE;
document.head.appendChild(style);

const root = document.getElementById("root");
if (root !== null) {
  application<Model, Msg>({
    init: () => [initialModel, cmdEffect(poll)],
    update: makeUpdate(poll),
    view,
    subscriptions,
    // The walk is session state, not an address: a URL
    // change re-polls rather than navigating.
    onUrlChange: (_url: Url): Msg =>
      open("servers"),
  })(root);
}
