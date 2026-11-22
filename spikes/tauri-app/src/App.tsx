import { useState, type ReactElement } from "react";
import { IpcView } from "./views/IpcView";

// One entry per view. A later spike adds e.g. { id: "sandbox", title: "HTML sandbox", render: ... }.
const views = [{ id: "ipc", title: "IPC", render: (): ReactElement => <IpcView /> }] as const;

type ViewId = (typeof views)[number]["id"];

export function App(): ReactElement {
  const [active, setActive] = useState<ViewId>("ipc");
  const view = views.find((v) => v.id === active) ?? views[0];

  return (
    <div className="app">
      <nav className="nav" aria-label="Views">
        {views.map((v) => (
          <button
            key={v.id}
            type="button"
            data-testid={`nav-${v.id}`}
            aria-current={v.id === active ? "page" : undefined}
            onClick={() => setActive(v.id)}
          >
            {v.title}
          </button>
        ))}
      </nav>
      <main className="main">{view.render()}</main>
    </div>
  );
}
