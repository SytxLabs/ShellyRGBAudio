/**
 * three.js is about a megabyte of the bundle and only the room tab needs it, so it is fetched when that tab is first opened rather than at startup.
 *
 * The boundary matters as much as the laziness: WebGL can be unavailable — a remote session, a driver that gave up — and a blank rectangle would leave
 * no way to tell that from a bug.
 */

import { Component, Suspense, lazy, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { RoomView as RoomViewType } from "./RoomView";

const RoomView = lazy(() => import("./RoomView").then((module) => ({ default: module.RoomView })));

type Props = Parameters<typeof RoomViewType>[0];

export function LazyRoomView(props: Props) {
  const { t } = useTranslation();
  return (
    <RoomViewBoundary title={t("spatial.viewFailed")} note={t("spatial.viewFailedNote")}>
      <Suspense fallback={<div className="roomview placeholder">{t("spatial.viewLoading")}</div>}>
        <RoomView {...props} />
      </Suspense>
    </RoomViewBoundary>
  );
}

class RoomViewBoundary extends Component<{ children: ReactNode; title: string; note: string }, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="roomview placeholder">
          <div>
            <b>{this.props.title}</b>
            <p className="prose">{this.props.note}</p>
            <p className="prose">{this.state.error.message}</p>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
