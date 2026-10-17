import type { Listener } from "./types";

/** The name people know the owner by: the container's for Docker, the process's otherwise. */
export function ownerName(l: Listener): string {
  return l.container?.name ?? l.process?.name ?? "?";
}
