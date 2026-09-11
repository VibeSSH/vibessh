import { AddServerModal } from "@/components/servers/AddServerModal";
import { useServerModalStore } from "@/stores/serverModalStore";

/** Mounted once in AppLayout so Rail's quick-add button (and Servers.tsx's own "Add server" button) can open the same modal from anywhere, controlled by one shared store instead of page-local state. */
export function GlobalServerModal() {
  const isOpen = useServerModalStore((s) => s.isOpen);
  const editingServer = useServerModalStore((s) => s.editingServer);
  const close = useServerModalStore((s) => s.close);

  if (!isOpen) return null;
  return <AddServerModal onClose={close} editingServer={editingServer ?? undefined} />;
}
