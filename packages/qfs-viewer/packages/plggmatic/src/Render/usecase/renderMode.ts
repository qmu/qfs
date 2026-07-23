import { type Html } from "plgg-view";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";
import { type Scene } from "plggmatic/Schedule/model/Scene";
import { type Mode } from "plggmatic/Render/model/mode";
import { multiColumn } from "plggmatic/Render/usecase/multiColumn";
import { singleColumn } from "plggmatic/Render/usecase/singleColumn";

/**
 * The runtime render dispatcher (D10) — the PUBLIC entry a
 * consuming app calls: `renderMode(mode)(scene)`. It
 * exhaustively matches the closed {@link Mode} to a
 * renderer, so adding a mode without a renderer is a `tsc`
 * error here, not a runtime fallback. Because both
 * renderers are pure projections of the SAME scene, a mode
 * flip mid-flow is loss-free by construction: same flow
 * position, selection, query, pending confirmation, and
 * URL — only the projection changes.
 */
export const renderMode =
  (mode: Mode) =>
  (scene: Scene): Html<SchedulerMsg, "div"> => {
    switch (mode) {
      case "multiColumn":
        return multiColumn(scene);
      case "singleColumn":
        return singleColumn(scene);
    }
  };
