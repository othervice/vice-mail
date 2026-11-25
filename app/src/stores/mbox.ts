import { create } from "zustand";
import type { MboxJob, MboxJobId } from "../services/ipc";

/** Stage 19 (decision 0057): the mbox jobs this window shows, newest last. */
interface MboxState {
  jobs: readonly MboxJob[];
  /** A job started here, or `mboxJobChanged` from the core (any window). */
  upsert: (job: MboxJob) => void;
  dismiss: (id: MboxJobId) => void;
}

export const useMboxStore = create<MboxState>((set) => ({
  jobs: [],
  upsert: (job) => {
    set((state) => {
      const index = state.jobs.findIndex((j) => j.id === job.id);
      if (index < 0) return { jobs: [...state.jobs, job].slice(-4) };
      const jobs = [...state.jobs];
      jobs[index] = job;
      return { jobs };
    });
  },
  dismiss: (id) => {
    set((state) => ({ jobs: state.jobs.filter((j) => j.id !== id) }));
  },
}));
