import { EventHandler, ActionCategory } from "./aiEvents/types";
import { colimaRegistry } from "./aiEvents/colima";
import { dockerRegistry } from "./aiEvents/docker";
import { volumesRegistry } from "./aiEvents/volumes";
import { systemRegistry } from "./aiEvents/system";
import { composeRegistry } from "./aiEvents/compose";
import { k8sRegistry } from "./aiEvents/k8s";
import { limaRegistry } from "./aiEvents/lima";
import { configRegistry } from "./aiEvents/config";

import { isDockerOnlyHost, supportsHostVms, supportsKubernetes } from "../store/capabilities.svelte";

export type { ActionCategory, EventHandler };

export const registry: Record<string, EventHandler> = {
  ...colimaRegistry,
  ...dockerRegistry,
  ...volumesRegistry,
  ...systemRegistry,
  ...composeRegistry,
  ...k8sRegistry,
  ...limaRegistry,
  ...configRegistry,
};

function availableEvent(name: string): boolean {
  // These legacy handlers generate commands the Windows sandbox cannot run.
  if (isDockerOnlyHost() && ["build-image", "compose-build", "compose-pull"].includes(name)) return false;
  if (name in k8sRegistry) return supportsKubernetes();
  if (name in colimaRegistry || name in limaRegistry || name.startsWith("model-") || ["list-presets", "save-preset", "delete-preset"].includes(name)) return supportsHostVms();
  return true;
}

export function getCategory(eventName: string): ActionCategory | null {
  return availableEvent(eventName) ? registry[eventName]?.category || null : null;
}

export async function executeEvent(eventName: string, payload: unknown): Promise<string> {
  const handlerInfo = registry[eventName];
  if (!availableEvent(eventName)) throw new Error(`Unavailable on this host: ${eventName}`);
  if (!handlerInfo) {
    throw new Error(`Unknown event: ${eventName}`);
  }
  return await handlerInfo.handler(payload);
}

export function listEvents(): { name: string; category: ActionCategory; description: string }[] {
  return Object.entries(registry).filter(([name]) => availableEvent(name)).map(([name, { category, description }]) => ({
    name, category, description
  }));
}
