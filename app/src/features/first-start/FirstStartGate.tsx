import { useState, type ReactNode } from "react";
import type { CoreError } from "../../services/ipc";
import { FirstStart } from "./FirstStart";
import { useFirstStart } from "./queries";

/**
 * Stage 15 (decision 0044): the main window shows the guided first start instead of the frame
 * while the core says so (no account, never ended). The answer is read once: an account added
 * on the second step must not end the steps that follow it. If the core cannot answer, the frame
 * shows (the bridge has logged the failure, and the frame shows the core's state and errors).
 */
export function FirstStartGate({ eventsError, children }: { eventsError: CoreError | null; children: ReactNode }) {
  const query = useFirstStart();
  const [phase, setPhase] = useState<"steps" | "frame" | null>(null);
  let current = phase;
  if (current === null) {
    if (query.data) current = query.data.show ? "steps" : "frame";
    else if (query.isError) current = "frame";
  }
  // Fixed once known, so a later answer of the core cannot swap the view under the user.
  if (phase === null && current !== null) setPhase(current);

  if (current === null) return null;
  if (current === "steps") {
    return (
      <FirstStart
        eventsError={eventsError}
        onEnd={() => {
          setPhase("frame");
        }}
      />
    );
  }
  return children;
}
