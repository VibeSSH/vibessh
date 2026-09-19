import { create } from "zustand";

/**
 * The Node the dashboard is focused on, shared so the sidebar's connection
 * selector and the dashboard's own Servers panel drive the same selection.
 * Picking a Node in the selector scopes the dashboard's tiles and workspace
 * to it rather than opening that Node's console; null means the fleet-wide
 * overview.
 */
interface SelectedNodeState {
  selectedNodeId: string | null;
  setSelectedNodeId: (id: string | null) => void;
}

export const useSelectedNodeStore = create<SelectedNodeState>((set) => ({
  selectedNodeId: null,
  setSelectedNodeId: (id) => set({ selectedNodeId: id }),
}));
