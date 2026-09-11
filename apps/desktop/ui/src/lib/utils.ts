import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** shadcn's class helper, required verbatim by every component it ships. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
