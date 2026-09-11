import { callCommand } from "./tauri";
import type { ApplicationTemplate } from "@/types/applicationTemplate";

export function listApplicationTemplates(): Promise<ApplicationTemplate[]> {
  return callCommand<ApplicationTemplate[]>("list_application_templates", {});
}

export function saveApplicationTemplate(template: ApplicationTemplate): Promise<ApplicationTemplate> {
  return callCommand<ApplicationTemplate>("save_application_template", { template });
}

export function deleteApplicationTemplate(templateId: string): Promise<void> {
  return callCommand<void>("delete_application_template", { templateId });
}
