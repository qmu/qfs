---
created_at: 2026-07-17T10:20:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission:
---

# qfs gdrive ドライバの REMOVE が WHERE セレクタを無視し、対象フォルダ自体をゴミ箱に入れる

## Overview

qfs **0.0.71**（aarch64-linux-musl、commit f9387de）で、Google Drive の共有ドライブ内フォルダから
1 ファイルだけをゴミ箱に入れる意図で cookbook どおりの文を発行したところ、**WHERE セレクタが適用されず、
ターゲットパスのフォルダノード自体（配下約 30 ファイルごと）がゴミ箱に入った**。共有ドライブのゴミ箱
（30 日保持）から復元できたため実害は回避したが、不可逆ゲート（`--commit --commit-irreversible`）を
正しく通過した上での予期しない大量削除であり、REMOVE の契約違反として優先度高で修正したい。

## 再現手順

1. `/drive` を gdrive ドライバでマウントし、共有ドライブ内のフォルダ（複数ファイルを含む）を用意する。
2. 次の形の文を発行する（gdrive cookbook の「Trash a file」例と同型）:

   ```
   remove /drive/shared/<Drive名>/<フォルダ> where name == '<ファイル名>.xlsx'
   ```

3. プレビューは次を表示する — `target` がフォルダパス、`selector: ["name"]`、`affected: "unknown"`:

   ```json
   {"preview":{"rows":[{"id":0,"verb":"REMOVE","target":{"driver":"drive","path":"/drive/shared/<Drive名>/<フォルダ>"},"affected":"unknown","irreversible":true,"selector":["name"]}],"irreversible":[0],"total_affected":"unknown","is_pure":false},"committed":true}
   ```

4. `--commit --commit-irreversible` で適用すると、WHERE に一致する 1 ファイルではなく
   **フォルダ全体が Drive のゴミ箱へ移動する**。

## 期待する挙動

- REMOVE は WHERE セレクタを子ノードに対して解決し、**一致したファイルのみ**をゴミ箱に入れる。
- ディレクトリノードへの REMOVE は、セレクタ解決の結果が空・不定のときは適用を拒否する
  （フォルダごと消す操作は、明示の別形式でのみ許すべき）。
- プレビュー段階で対象を実際に解決し、`affected` に確定件数と**対象ファイル名の列挙**を出す。
  `affected: "unknown"` のまま不可逆コミットに進めるのは、プレビュー→コミットの安全モデルの穴になる。

## 関連する周辺不具合（同セッションで観測、別チケット化の判断は任せる）

- **単一ファイルパス読みの Unicode 正規化**: Drive 上の NFD 正規化のファイル名（濁点分解）は、
  一覧に出る名前をそのまま NFC でパスに使うと `not_found` になる。パス解決時の正規化吸収が望ましい。
- **パス字句解析**: ファイル名に ASCII `?` や空白を含むと、単一ファイルパスの文が
  `lexing failed: UNEXPECTED_CHAR` で書けない。今回 WHERE 形式の REMOVE に迂回した動機がこれで、
  引用符付きパスセグメント等の受け口が要る。

## 備考

- 発行した文は qfs-gdrive cookbook（skill 0.1.0）の記載例と同型であり、ドキュメントと実装の乖離でもある。
  0.0.71 と 0.1.0 の版差で未実装なら、旧版バイナリ側で該当機能を fail-closed にしてほしい。
- 再現時は必ず捨てフォルダで行うこと（本件は実データで発生し、ゴミ箱復元で回収した）。

## Policies

- `workaholic:implementation` / honest-surfaces — a preview that claims a filtered REMOVE while
  the commit applies an unfiltered one is a dishonest surface; the WHERE must reach the applier
  or the statement must refuse.
- `workaholic:design` / fail-closed irreversible writes — an irreversible effect whose selector
  cannot be resolved must be refused, never widened to the containing node.
- `workaholic:safety` — this is a data-loss class incident (recovered via Drive trash);
  reproduction must only ever run against mock/fixture clients, never live Drive data.

## Quality Gate

1. A hermetic regression test reproduces the incident shape end-to-end through the runtime
   seam (plan node with a `name` selector → `EffectInput` → bridge → gdrive applier over the
   mock client) and proves ONLY the WHERE-matched child is trashed — never the folder node.
2. A REMOVE addressing a folder by name path with no resolvable selector is refused
   (fail-closed); trashing a folder with its subtree requires the explicit id-addressed form.
3. A WHERE filter that cannot be carried to the applier as complete equality keys is refused
   at plan time (never silently dropped into an under-constrained irreversible write), for
   every driver sharing the selector channel (gdrive, sql, …).
4. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`, gen-docs/gen-skills/check-migrations all pass with raw exit 0.
5. The gdrive cookbook article and generated skills teach the corrected REMOVE contract.
